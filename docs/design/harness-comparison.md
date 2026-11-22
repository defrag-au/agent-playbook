# Comparing the harnesses

The rig-eval runner measures two rig homes. The question that motivated it is wider: does our
harness — rig plus the playbook plugin — do better than the harnesses we might otherwise use?
This is the design for the four-way comparison, the fairness controls it needs, and what each tool
can actually report.

## The arms

| Arm | What runs | How it is driven |
| --- | --- | --- |
| `rig:slim` | `rig` + `SlimPlugin`: `search`, `diff` and `pr`; `cat`, `sed -n`, `head`, `git status`, `git log` and `git diff` are redirected | `rig -p --json` |
| `rig:advertised` | `rig` + `AdvertisedPlugin`: the tools, the redirect and the shell policy, plus advertised descriptions and the tool map | `rig -p --json` |
| `rig:permissive` | `rig` + `PlaybookPlugin`: the `at-*` tools, the redirect, the shell policy | `rig -p --json` |
| `rig:gated` | `rig` + `GatedPlugin`: the same tools, the redirect, the read-refusal Gate | `rig -p --json` |
| `rig:baseline` | `rig` + `ToolsOnlyPlugin`: the same tools, no shell policy | `rig -p --json` |
| `rig:vanilla` | `rig` with no extra plugin | `rig -p --json` |
| `opencode` | opencode 1.19.5 | `opencode run --format json --auto` |
| `pi` | Pi coding agent 1.5.0 | `pi --mode json` |
| `zed` | Zed's built-in agent | interactive; audited from `threads.db` |

`rig:vanilla` is the control that separates "rig" from "our plugin". Without it, a win could be the
harness or the policy layer, and we could not say which. `rig:permissive` and `rig:advertised` are a
paired treatment: the same tools, policy and redirect, with only what the model is told about them
changed. `rig:permissive` and `rig:slim` are the other pair: the same policy and redirect, with the
lightweight reads removed from the tool surface and served from the shell call instead.

## Fairness controls

- **One model.** All arms call the same provider endpoint with the same model string. Rig's
  DeepSeek provider sends `deepseek-flash` to `https://api.deepseek.com/chat/completions`, and the
  catalog resolves `deepseek/deepseek-flash` to that wire model. OpenCode and Pi are configured
  with an OpenAI-compatible provider at `https://api.deepseek.com` and the same model string,
  using the same key. The opencode free tier (`opencode/deepseek-v4.1-flash-free`) is a different
  serving, so it is only a smoke test. Zed threads already record `deepseek/deepseek-flash`,
  which is the same model. **Verified 2026-11-09:** opencode with a `deepseek` provider
  (`@ai-sdk/openai-compatible`, `baseURL https://api.deepseek.com`, model `deepseek-flash`, key
  from `DEEPSEEK_API_KEY`) answered through that endpoint. The probe cost about $0.002.
- **Approval policy is recorded, not equalised.** opencode runs `--auto`; Pi has no permission
  system; Zed is approved interactively; rig's Gate is part of the arm. Those are the harnesses.
- **One corpus.** The five scenarios in `integrations/rig-eval/scenarios`.
- **One snapshot state per run.** Each run executes in a fresh snapshot of the worktree: a fresh
  git history without the corpus, one deterministic uncommitted file, and symlinked warm build
  directories. Arms cannot affect one another, and `--jobs` runs snapshots in parallel. Wall time
  is contended and is not compared across arms.
- **Same instructions.** Every arm reads `AGENTS.md` — opencode and Pi do it by default, and Pi is
  not given `-nc`. The comparison is therefore harness plus enforcement, not knowledge of the
  rules. If the goal later becomes "no rules at all", that is a second run with the context file
  removed from every arm.
- **Same prompt.** The scenario prompt verbatim, with a tag only where a harness needs one to find
  the run afterwards (Zed).

## Isolation is not an arm

None of the harnesses provides an OS-level sandbox in the mode this comparison drives, and the
runner does not ask one to.

- **rig** exposes the seam: every child process starts through `Processes`, so a plugin can wrap
  it, and `corpus/base/src/shell_sandbox.rs` shows the shape (`sandbox-exec` on macOS, `bwrap` on
  Linux). No sandbox plugin is in `built_in_plugins!`, and none of our homes enables one.
  `ApprovalPlugin` and `PreviewPlugin` are policy, and off.
- **opencode** permission rules are an approval policy. `--auto` approves them, and its docs say
  shell runs with the host user's filesystem, process and network authority.
- **Pi** has no permission system or sandbox by design; its docs recommend running the whole
  process in a container.
- **Zed** asks for tool approval interactively; that is not confinement either.

Sandboxing is therefore a deployment concern that sits outside the harness, and outside this
comparison. The seatbelt profile in `sandbox.rs` is not a safety sandbox: it denies reads of the
real checkout's `.git`, the corpus, the output directory and `.tmp/rig-eval`, and each run
executes in a snapshot whose reachable history has the corpus rewritten out. That keeps the gold out
of every arm's reach. It must be identical across arms, and delegating it to each harness is impossible for
Pi and opencode. Equal ambient authority is the only fair baseline: a sandbox enabled on the rig
arm alone would measure refused commands, not a better harness.

An unattended harness would sandbox at launch (container, VM or seatbelt), or inside rig with a
plugin that sets `Processes`. The Gate is policy above either. The container launch is now landed:
`integrations/rig-eval/container/` builds the image and `rig-eval --container` mounts each snapshot
at `/repo` in it. The seatbelt profile remains for a host run.

## What each harness exposes

| Measure | rig | opencode | pi | zed |
| --- | --- | --- | --- | --- |
| Model calls | `usage` events | `step_finish` | `turn_start` | assistant messages |
| Tool calls by name | `tool_call` | `tool_use.part.tool` | `tool_execution_start.toolName` | total only |
| Tool output bytes | `tool_result.output` | `part.state.output` | `result.content[].text` | `tool_output_bytes` |
| Refusals | `tool_result.refused` | — | — | — |
| Compactions | `usage.purpose = compaction` | — | `compaction_*` | `compactions` |
| Wall time | measured | measured | measured | — |
| Final answer | `exit.answer` | `text` events / export | last assistant `message_end` | needs `--answer` |

A dash is not zero. The report prints `—` and the comparison excludes that measure for that arm.
Claiming equality where a harness does not report a number is the failure mode this table exists
to prevent.

## The Zed arm

Zed has no headless prompt. The arm is a **manual run with a recorded protocol**:

1. A new thread, model `deepseek/deepseek-flash`, the same checkout.
2. The prompt starts with `[eval:<scenario>]`; the thread title carries the tag, so the audit can
   find it later without guessing by timestamp.
3. After the five runs, export the audit:
   `zed-thread-audit --json --project agent-playbook --top 0`.
4. The runner reads the thread by tag. `zed-thread-tools` gains `--answer <thread>`, which prints
   the final assistant text and nothing else.

The audit already measures tool calls, tool-output bytes, compactions and the model. It cannot
verify `tool_says` without reading tool output, so those checks are marked unavailable for Zed
rather than failed.

ACP is not the common seam here. Zed is an ACP **client**, not a server; rig serves ACP v1 only;
and the arm we care about for Zed is its own agent, not an external one. The comparison uses each
harness's native headless mode and treats Zed as a recorded manual run.

## The runner

`rig-eval` grows a target layer. The core (`scenario`, `score`, `report`) already knows nothing
about rig; the driver is the only harness-specific part.

- `--target rig:gated` (repeatable), `opencode`, `pi`, `zed`
- `--jobs N` runs N snapshots concurrently
- each driver produces the same `RunResult`; `score` and `report` do not know which harness
  produced it
- `--zed-thread <scenario>=<id|title>` binds a manual thread to a scenario

The drivers:

- **rig** — today's `-p --json` parser, unchanged.
- **opencode** — `opencode run --format json --auto -m <model> <prompt>`. A `tool_use` event carries
  the tool name and `state.output`; `step_finish` carries the tokens and cost; `text` events are
  the answer.
- **pi** — `pi --mode json -m <model> <prompt>`. `tool_execution_start` and `tool_execution_end`
  carry the tool and its result; `turn_start` counts model calls; `message_end` gives the assistant
  text; `compaction_start` / `compaction_end` are the compaction count.
- **zed** — `zed-thread-audit --json` plus `--answer`. No process is spawned; the run is imported.

## What the first comparison will and will not say

It will say, per scenario and per arm: checks passed, tool calls, output bytes, model calls and
wall time. Wall time is reported, but a run with `--jobs > 1` is contended and is not compared
across arms. It will not say "harness X is better" from five scenarios on one model. The value is
the shape of the difference: whether the playbook plugin changes the call count or the output
bytes, whether opencode and Pi reach the same answer with fewer calls, and where Zed's interactive
agent lands.

The result is also allowed to be that the harnesses are within noise of each other. Five scenarios
is a probe, not a benchmark; a difference that does not survive the long-thread corpus is not a
result.

## The first result

2026-11-09, `deepseek/deepseek-flash`, five scenarios × five targets × two repeats, four workers,
50 runs. Every run was clean: no contamination, no inspected-corpus tag, no timeout, no unparsed
stream line. The snapshot history held no corpus, and `draft-pr` still found `3b39d5c` and
`c206304`.

| Target | Median score | Median tool calls | Median output | Median model calls |
| --- | --- | --- | --- | --- |
| opencode | 1.00 | 8 | 34.5 KB | 4 |
| pi | 0.80 | 11 | 28.1 KB | 7 |
| rig:baseline | 1.00 | 10 | 24.5 KB | 8 |
| rig:gated | 1.00 | 11 | 24.9 KB | 8 |
| rig:vanilla | 1.00 | 14 | 45.7 KB | 10 |

The overall medians are not a ranking: the two easy scenarios are 1.00 for every arm. The hard
ones are where the differences are, and they are noisy:

| Scenario | opencode | pi | rig:baseline | rig:gated | rig:vanilla |
| --- | --- | --- | --- | --- | --- |
| `devshell-tests` | 0.88 | 0.88 | 1.00 | 0.88 | 1.00 |
| `pre-edit-state` | 1.00 | 1.00 | 1.00 | 1.00 | 1.00 |
| `draft-pr` | 1.00 | 0.90 | 1.00 | 1.00 | 1.00 |
| `recover-investigation` | 0.88 | 0.62 | 0.62 | 0.88 | 1.00 |
| `trace-redirect` | 0.50 | 0.60 | 0.70 | 0.40 | 0.80 |

The important result is the instability. The previous run — before the snapshot linked the
sibling fork — had `rig:baseline` at 1.00 on `recover-investigation` and 0.80 on `trace-redirect`,
and `rig:vanilla` at 1.00 and 0.80. This run has baseline 0.62 and 0.70, and vanilla 1.00 and
0.80. Same five scenarios, same model, two repeats each; the hard-scenario ranking moved. Two
repeats is not a sample.

What survives:

- **The easy scenarios cannot separate the arms.** They are correctness smoke tests, and their
  medians carry the overall numbers.
- **The `at-*` tools are not a stable correctness win.** `rig:baseline` used fewer calls than
  vanilla on `draft-pr` (5 against 21) and `recover-investigation` (15 against 20), but scored
  0.62 against 1.00 on recover; the extra calls bought citations. On `trace-redirect` both used 12
  calls, and baseline scored 0.70 against 0.80.
- **The Gate is mixed.** It beat baseline on recover (0.88 against 0.62) and lost on trace (0.40
  against 0.70); it spent 13 calls on `draft-pr` against 5. It is neither a consistent saving nor
  a consistent correctness win.
- **opencode is the cheapest; it and the gated arm are weakest on trace** (0.50 and 0.40). Pi
  matches opencode on devshell and is close on draft-pr (0.90 against 1.00), but 0.62 on recover.
- **Wall time is contended** under four workers and is not compared.

The fork path was a snapshot bug: `integrations/rig` resolves its dev-dependencies at `../../../rig`,
and the first run without the sibling link spent 22 tool calls on `devshell-tests` where the fixed
run spends 9. The fixed run is the data; the earlier run is kept as a lesson.

The clean per-run data is in `.tmp/rig-eval/full-3`: `traces.json`, `tools.json`, `checks.json`,
and `observable.md`. The next corpus has to come from the long threads; this one shows why.

## Order

1. **`--answer` in `zed-thread-tools`.** It is needed for any Zed score, and it is small.
2. **The target layer and the opencode driver.** opencode runs through `bunx opencode-ai` and its
   JSON stream is already known; run `opencode` and the three rig arms together.
3. **Pi.** Node 22.19+ is in the devshell now (`pkgs.nodejs_22`, 22.23.3 in the current pin).
   Install Pi into the scratch tree and add the driver:

   ```sh
   npm install -g --prefix .tmp/pi --ignore-scripts @earendil-works/pi-coding-agent
   .tmp/pi/bin/pi --version
   ```
4. **The Zed manual run.** Five threads with the tag, then audit and import.
5. **Compare.** If the shape is interesting, extend the corpus from the long threads.

## Open decisions

- **Pi install.** Node is in the devshell; Pi itself is installed from npm into `.tmp/pi`. The
  alternative is a global npm install, which is less contained but puts `pi` on every shell's
  `PATH`.
- **The provider config.** The endpoint and model are now known (`https://api.deepseek.com`,
  `deepseek-flash`); what remains is writing the provider block for OpenCode and Pi and confirming
  each sends the same model string. A run that reports a different model is discarded.
- **Zed's prompt tags.** The tag is in the prompt the model sees. It is a small cost; the
  alternative is selecting threads by timestamp, which is ambiguous.
