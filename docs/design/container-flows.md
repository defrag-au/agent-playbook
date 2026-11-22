# Container flows: the agent gets a working tree, not a VCS

This records the decision to run hands-off project work as a container task with a typed handoff,
and the boundary that makes it safe: **the agent's interface is a directory.** Nothing in the
container can stage, commit, push, fork or share.

It is the agent-side half of `core/git-is-the-users-domain`. That rule says the human's index and
history are theirs. This one says the agent should never have had an index in the first place.

## What this decides

1. Why agents get neither `git` nor `delta` as a tool (below).
2. The handoff contract: what the host asks for, what comes back, and who does which step.
3. The container boundary: an image per project, one credential, no repository access.
4. The Delta experiment: the one falsifiable check that decides whether DeltaDB replaces the
   patch handoff, or sits beside it.

## The antipattern

The problem is not that an agent types `git commit`. It is that the agent has an index, refs,
remotes and a reflog at all. With those, every write command is destructive and every read command
is an unbounded side channel.

Three failures, two of them measured in this repository:

- **History is an answer key.** `integrations/rig-eval/src/snapshot.rs` says it directly: the
  corpus is committed, so hiding `scenarios/` in the working tree does not help — `git show` and
  `git diff` read it out of history. The snapshot exists to rewrite history for exactly that
  reason.
- **A command policy does not engage.** The gated arm produced three refusals in ten runs, because
  agents write `cd repo && …` and the Gate judged only the first program of the first segment.
  `read` was outside the policy and was used twice as often as the ungated baseline (31 against
  15). A wrapper loses to a tool the model already uses.
- **The destructive commands are silent.** `git add -A` sweeps in `.env` and build output;
  `restore`, `stash`, `clean` and `reset --hard` delete a human's uncommitted work; `--no-verify`
  gets past a hook; an amend or force-push rewrites shared history. Each is a normal thing to type
  when trying to be helpful.

The distinction worth keeping: **history knowledge is legitimate, VCS interaction is not.** An
agent may ask what changed and why. It may not hold the thing that can also change it. History
questions go through `at-recall`, which is read-only and says what it omitted; they never go
through a shell that can write.

## What the agent gets

A working tree, a toolchain, and bounded answers about the tree. Four ways to build that, in order
of preference:

| Mode | What it buys | What it breaks |
| --- | --- | --- |
| no `.git` | nothing to misuse | crates that shell out to `git describe`, `vergen`, a build script |
| synthetic one-commit repo | `git status`/`describe` work; no history to read; a clean diff base | archaeology tasks |
| read-only `.git` | tools work, writes fail structurally | history is still readable |
| full history | only when a task names real commits (today's eval) | the corpus leak above |

Builder flows take synthetic or none. "What changed" is a harness-provided diff against the base
tree, produced outside the agent's view, not `git diff` inside it.

Things that legitimately need repository metadata are resolved **before** the snapshot: submodules
and Git LFS are checked out host-side; a synthetic repo carries the tag or describe output a build
needs. A task that genuinely needs history gets a bounded `at-recall` answer pinned to the base
commit, never the real `.git`.

## The handoff contract

The eval runner's boundary is the right one and its handoff is deliberately absent: writes to
`/repo` are discarded with the snapshot, and "the answer arrives on the harness's stream, which is
what is scored" (`docs/design/rig-eval.md`). A builder inverts exactly that. The work must survive.

The contract to build against, not yet built:

```rust
/// What the host asks for. One task, one container, one result.
struct TaskSpec {
    name: String,
    /// The checkout the base commit is resolved in, host-side.
    repo: PathBuf,
    /// A commit, never a branch: the task cannot follow a moving ref.
    base: String,
    prompt: String,
    /// Run in order, inside the container, after the agent stops.
    verify: Vec<VerifyStep>,
    /// Paths that must survive; anything else is discarded with the snapshot.
    outputs: Vec<Output>,
    limits: Limits,
}

struct VerifyStep {
    argv: Vec<String>,
    timeout_s: u64,
}

struct Output {
    path: String,
    kind: OutputKind, // Artefact | Report | Patch
}

struct Limits {
    wall_s: u64,
    cpus: u32,
    memory: String,
}

/// What comes back. Written by the runner, never by the agent.
struct Handoff {
    task: String,
    /// Reference and digest, the identity a result is reproducible from.
    image: ImageId,
    base: String,
    /// A git diff against `base`, produced host-side from the snapshot.
    patch: PathBuf,
    /// The agent's answer, and its proposed commit message as text.
    report: PathBuf,
    artefacts: Vec<Artefact>,
    verify: Vec<VerifyOutcome>,
    /// The session's `effects.jsonl`, unchanged.
    effects: PathBuf,
    status: Status, // Succeeded | FailedVerify | FailedLimit | FailedHarness
}
```

Four rules make it a contract rather than a convention:

- **The agent writes files; the runner writes the handoff.** Nothing in the bundle is authored by
  the model except the report body.
- **Verification runs after the agent, in the same container, before the snapshot is torn down.**
  A failed verify fails the run. Without this, "hands off" means "trusted".
- **An output that is git-ignored must be declared.** A produced binary, a coverage report or a
  database file is otherwise invisible: the snapshot is discarded, so it never existed.
- **A commit message is proposed, never applied.** The report carries it; the human commits.

| Step | Where |
| --- | --- |
| image build · base snapshot · patch extraction · applying · commit, push, PR · Delta sync | host |
| edits · builds · tests · verify | container |
| review, edit the message, land it | human |

The effect log remains the canonical trace (`docs/design/rig.md`, decided 2026-11-08). A handoff
points at it; it does not invent a second one.

## The container boundary

The image is the boundary, as it already is for the eval: a per-trial snapshot is mounted at
`/repo`, the corpus is absent, the output directory is not reachable, and the image digest is the
identity recorded in the manifest. `container` 1.6.0 is installed on the workstation and
`playbook-eval/rig-eval:local` exists.

What changes for a builder:

- **One image per project, built from its own `flake.lock`.** The eval's trick — the image *is* the
  devshell, with `direnv exec` and `nix develop -c` as shims — is the right shape. A per-project
  image must be rebuilt when the lock moves and published to a registry, not built per run. The
  eval image is 4.1 GB with two warm rig homes; a real devshell will be larger.
- **Warm targets are load-bearing.** The eval bakes each home with a warm target because `rig` runs
  `cargo build` before every start, and a cold Bevy build would dominate the measurement. A
  per-project image needs the same treatment, or every task pays for the first compile.
- **One secret.** The model key, through a mode-600 `--env-file`, never `--env` on the command
  line. No repository credential enters the container, which is the point: the handoff is a patch,
  not a push.
- **The network is the host's.** `container` does not give an egress policy. A task that must reach
  only the model needs a proxy, and that decision is out of scope here. Say so in the task spec
  rather than implying isolation that is not there.
- **A multi-service task is a different boundary.** A verify step that needs D1, R2 or a chain
  endpoint is not hermetic. That is `container-compose` and a real stack, with a different story
  about what "the container" contains.

## Delta: the recorder, not a tool for the agent

DeltaDB is a CRDT layer over Git, loosely based on diamond-types: a delta carries inserted text,
replaced ranges, and the version it builds on — earlier deltas, or a Git blob for the file's
starting text. Every client holds a replica, concurrent edits converge by actor id, and a fragment
index maps an old position into the current text. `delta worktree mount` gives a checkout you can
edit with any tool; `delta thread fork` branches an approach; `delta --json` puts results on
stdout.

It is not a replacement for Git: the commits, the remote and the pull request stay. And the CLI is
not a headless dependency today — it installs and runs the Delta app (`~/Applications`, or
`~/.local/delta.app` on Linux), sign-in is the supported path, and Claude Code print-mode, SDK and
cloud sessions are explicitly unsupported. Sharing publishes a thread and its files to the whole
organisation; ignored files stay local.

**Why it is worth having anyway:** agents reach for git because uncommitted work is fragile — no
checkpoint, no undo, no way to hand it over. DeltaDB records every step without a commit, so the
motive for the reach disappears. That is the argument, and it is compatible with Git landing the
work.

**Why it is still not an agent tool.** The same antipattern with a different binary: an agent that
can run `delta thread share` can publish to the org. Recording belongs to the harness. The
container gets a directory; the sync client runs on the host.

### The falsifiable experiment

The one check that decides whether DeltaDB replaces the snapshot mount:

1. Mount a Delta worktree read-write at `/repo`; run the sync client on the host.
2. The container holds no `delta` binary and no credential. The agent edits with normal tools and
   the verify step runs as usual.
3. **Pass:** a new delta appears in the thread, the host's replica converges, and the container's
   egress shows no Delta endpoint. `delta worktree mount` behaves as a directory, so the patch
   handoff becomes optional rather than required.
4. **Second half:** fork the thread, mount a second worktree, run two containers on the same base.
   Their edits converge, and `target/`, `.tmp/` and other ignored paths are not uploaded.
5. **Fail:** the worktree needs a sign-in or a running app inside the sandbox, needs a display, or
   uploads ignored files. Then the patch handoff stands and Delta waits.

Adoption gate, so this is not a vibe: steps 1–4 pass on one machine, with the container holding
nothing but the model key. If the only gain turns out to be conversation retention, the effect log
already carries typed facts and gate decisions that a Delta message does not (`decided_by`,
published tool output), so the patch handoff is the better dependency.

## The first project

`chromium-surfaces`, chosen 2026-11-22: its contract half is the cleanest hermetic verify in the
workspace, and its input work (O4) is already in flight.

**The verify is the contract, not the host.** `crates/surface` and `crates/surface-input` are
std-only, so the whole check is three commands:

```sh
cargo test --locked -p surface -p surface-input
cargo fmt --all -- --check
cargo tree -p surface -p surface-input --prefix none   # must name only workspace crates
```

Run on the host on 2026-11-22: 24 tests pass, `fmt` is clean, and `cargo tree` printed only
`surface` and `surface-input`. The container run is not yet verified: `container run` in the agent
shell failed with `XPC connection error: Connection invalid` while `container system status` said
running, so the first run needs an unsandboxed shell (and `container system start` if the service
is stale).

**The boundary is Linux, and the host is macOS.** Apple's `container` runs Linux; the overlay host
is CEF windowless paint driven by `CGEvent` and the macOS run loop. That is the split the handoff
is for: the container verifies the patch, the human runs the platform check.

### What a Linux container checks

| Check | Runs? | Notes |
| --- | --- | --- |
| `surface` + `surface-input` tests | yes | the first verify; std-only |
| `cargo check -p surface-host --features dox` | yes, first time | the runtime is behind `cef-runtime`; `dox` compiles it against generated bindings with no distribution, and this is the first compile of the `cfg(not(target_os = "macos"))` pump branch |
| `cargo clippy/test --workspace` on Linux | yes | Linux-only cfg lints and tests |
| a real winit window, wgpu adapter, CEF overlay | no | no display, no GPU, no libcef; the acceptance in `docs/EXPERIMENTS.md` stays on macOS |
| Windows | no | a Linux container is not a Windows target |

The host devshell cannot stand in for this: `cargo check --target x86_64-unknown-linux-gnu` fails
with `can't find crate for core` because the Linux `std` is not installed in it. The container is
the only Linux compile on this machine, which is why it is worth having before the Linux host is.

The system libraries are the unknown: wgpu and winit generally dlopen their Linux backends, but the
first image either needs no packages or names them, and the first run answers it. Whether `cef/dox`
wants libclang is unverified.

**The first task: a duplicate `Started` is not a second finger.** `Touches::observe` forwards a
second `Started` for a live id as another `Started`, while `PointerAsTouch` drops a second press as
a platform glitch. The invariant is that a client never sees two downs for one finger. The task
adds a test and forwards the duplicate as `Moved` — the finger is still down, and the position is
kept — which is a small, behaviour-changing fix with no spec decision and no dependency. It starts
red, which is what makes it a flow test rather than a demonstration.

Enforcing the required `ping` binding is the obvious second task, and deliberately not the first:
`docs/BRIDGE.md` says `ping` is required, `Bridge::validate` does not check it, and enforcing it
changes an existing test's expectation. That is a contract decision and wants its own run.

**Success is all five:**

1. the patch applies cleanly on the host;
2. the verify exit code is in `Handoff`, and it was the container's own run;
3. the container held no credential but the model key;
4. a second run of the same task is faster than the first;
5. `effects.jsonl` is present and attributes the run.

Not in scope for the first run: push, PR, Delta. The first run proves the boundary and the
handoff; everything above composes on top.

## What this does not decide

- A scheduler or queue for many tasks. One task, one container, one result first.
- Multi-service stacks and their network story; that is `container-compose`.
- Non-Rust projects. The recipe is per-repo and the devshell supplies the toolchain; nothing here
  assumes Cargo except the first project.
- Whether the builder is a new binary or a mode of `rig-eval`. The handoff is the interface either
  way; naming is later.
- Whether a self-hosted Delta sync exists. None is documented; the experiment runs against the
  hosted service and says so.

## What would make us change course

- Per-project image builds costing more than the time they save, with no registry or layer caching
  win. Then a shared base image plus a mounted devshell becomes the cheaper shape.
- Verify steps that need credentials or services, which makes the container non-hermetic and moves
  the task into the multi-service boundary.
- Delta's worktree mount requiring an interactive session, leaving the patch as the only handoff.
- An agent that cannot work without history. The answer is a bounded read-only view through
  `at-recall`, not a shell with a reflog.

## Reading map

- [`rig.md`](rig.md) — the kernel, the plugin, and the effect log as canonical.
- [`rig-eval.md`](rig-eval.md) — the container the runner already uses, and what it deliberately
  throws away.
- [`command-gating.md`](command-gating.md) — containment and liveness, and why a command policy is
  weak against a tool the model already uses.
- `integrations/rig-eval/src/container.rs` — the mounts, the image, and the arms.
- `integrations/rig-eval/src/snapshot.rs` — the snapshot and the history rewrite, and why it exists.
- `https://delta.dev/deltadb` · `https://delta.dev/cli` — the CRDT model and the CLI's current
  limits.
