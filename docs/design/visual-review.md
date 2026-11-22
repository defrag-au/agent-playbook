# Visual review automation

The storybook already has a deterministic visual gate: goldens for change, and a
text report for text that is off its surface, cut, too small or overlapping. The
model half answers a different question: *what does this picture look like to a
reviewer?* This is the design for running the corpus through models, not the
corpus itself, and it is deliberately a benchmark harness rather than an agent.

The corpus lives in the sibling `macroquad-kit` checkout, at
`ui/macroquad-storybook/review`; it is data. The runner lives here, under
`integrations/`, because it is an evaluation of models and harnesses, not part
of the installed agent toolkit.

## What this decides

- **The harness owns the loop.** One model call per case. The model does not
  choose cases, call tools, retry itself, or read `expected.toml`.
- **Typed both ways.** The corpus is TOML, the answer is JSON, and both map to
  concrete structs. A parse error is a recorded miss, not a crash.
- **Local models are budgeted.** One local model resident at a time, with a
  memory and wall-clock ceiling. A model that does not fit is refused before the
  run starts.
- **Frontier models are audited.** Every call has a token and cost ceiling, and
  the run prints the plan before it spends anything.
- **The corpus stays model-agnostic.** Adding a model is a registry entry, not a
  change to a prompt or a case.

## The corpus

```
review/cases/<slug>/
  case.toml      metadata: story, arch, os, intent, image, optional crop
  expected.toml  scorer-facing: defect or clean, boxes, severity
  full.png
  crop.png       optional magnification, generated with image-shot
review/RUBRIC.md the fixed prompt
review/README.md layout, provenance, scoring
```

The runner reads `case.toml` and the images, sends `RUBRIC.md` + `intent` +
images, parses the answer, then reads `expected.toml` to score it. The expected
file must never be in the prompt. The rubric is versioned; edit it only with the
intention of invalidating comparisons.

A corpus is one architecture and one OS unless a comparison says otherwise.
The storybook keeps `tests/goldens-<arch>/<slug>/<os>.png`, and a case records
both, so an aarch64 macOS image is never scored against an x86_64 Linux one.

## Constraints

### 1. One call, no agent loop

A case is a prompt and an answer. There are no tools, no mid-run steering and no
second chance with a different crop. If the model cannot answer from the images
it was given, that is the measurement.

This is the opposite of the coding-agent runs in `rig-eval.md`, and it is why
this runner depends on `rig-core` rather than `rig-harness` or the playbook
plugin. A future experiment where the model chooses what to crop is a different
harness, with its own design.

### 2. Typed data, no free-form JSON

`Case`, `Expected`, `Defect`, `Answer`, `Score` and `RunResult` are structs with
serde derives. No `serde_json::Value`, no `json!`. The corpus is TOML; the answer
is JSON with a fixed shape; an answer that does not parse is recorded with the
reason and counts as a miss.

### 3. The provider seam

`rig-core` is the client. Local models use the Ollama provider; frontier arms use
the provider for that vendor. The same `Case` runs through either, and the run
manifest records:

- provider and wire model string;
- the model digest and quantisation (the `/api/show` reply), not just the tag;
- context length, temperature, seed, `think` setting;
- for frontier arms, input/output tokens and cost.

A model that does not advertise vision is refused. The runner sends the PNGs as
rig's multimodal `Image` content; the provider encodes them for its wire, and a
provider that cannot read images is a capability error, not a reason to fall
back to text. That fallback is a failure mode rig already models by downgrading
images a provider cannot read.

### 4. Local memory and time

The machine is a 48 GB M5 Pro, aarch64 macOS. One local model resident at a
time; `keep_alive` is bounded and a model is unloaded before the next one is
loaded. The registry carries a `max_resident_gb` and the runner refuses a model
whose resident size plus context exceeds it.

The measured local calibration, from `integrations/review-bench` over the
13-case corpus, three runs per case (39 calls):

| Model | Resident | Mean per case | Result |
| --- | --- | --- | --- |
| `qwen3-vl:30b-a3b-instruct` (Q4_K_M) | 20 GB | 4.0 s | 6/15 defect rows detected (2 of 5 cases), 24/24 clean rows false-positive, mean IoU 0.13 |
| `qwen3-vl:32b-instruct-q4_K_M` (dense) | 21 GB | 16–52 s | interrupted at 11/13; memory pressure; not scored |

The 30B's answers were identical across all three runs, so no case is flagged
unstable — the failure is systematic, not noisy. Every one of the 39 answers was
a single report of content clipped at the top or bottom edge of the image,
including all 24 clean rows. It described the one real right-edge cut
(`stick-before`, IoU 0.24) and did not name the defect on the other four defect
cases; its `holder-before` "detection" is a bottom-edge box at IoU 0.01, which
the intersection-only match rule counts. It is not usable as a routine reviewer.

The pre-fix run's `stick-clean` false positive was attributed to the crop edge.
After the crop fix the same case is still reported, now as "the bottom of the
screen is clipped, cutting off the lower part of the instruction text" — the
image shows that text complete, with empty space below it. The crop was not the
cause: the model asserts an edge cut on every screen, clean or not.

The conclusion is not "buy a bigger local model". The 30B's failure is not
resolution or capacity; it does not check the screen against the intent. Until a
local model scores, the deterministic report carries the routine pass and the
corpus selects between frontier arms. The dense 32B is an occasional, flagged
run (`--allow-heavy`), not a baseline; on this machine it is slower than
reviewing the thirteen cases by hand.

### 5. Reproducibility

Temperature 0.1, top-p 0.9, fixed seed, thinking off unless the case is being
deliberately compared with it on. Record the model tag and the hashes of the
corpus, rubric and image files. Three runs per model, every run reported beside
the aggregate, and a case whose answer changes between runs flagged unstable —
kind or box, not the note's wording. A model that is right once in three is not
reliable.

### 6. Images and coordinates

Send the full image at 1:1; send the crop as a second image only when it helps.
Never downscale before sending: a 1 px artifact is the thing under test.

The model's answer uses `[x0, y0, x1, y1]` in 0–1000, relative to the **full
image**; the scorer converts to pixels and compares with `expected.toml`'s
`[x, y, w, h]`. This is Qwen-VL's native convention, and the first run showed
what happens when the prompt asks for pixels instead: the model returns 0–1000
boxes and the scorer silently compares the wrong space.

**A crop edge is not a screen edge.** A crop must not cut through content except
at the screen edge under test. The rubric warns against reporting a defect that
exists only because the crop is a crop, and the corpus's README carries the
stricter rule. The first run reported a clean stick as clipped because the crop
cut the top of its circle; the crop was the defect.

### 7. Scoring

- Detection: a reported defect matches when its kind matches (or `other` with a
  note that names the expected defect) and its box intersects the expected box.
- Localisation: IoU of the best matching box; mean over detected cases. A
  detection with no overlap is a false positive, not a miss.
- False positives: every report on a `clean` case, and every unmatched report on
  a defect case. The count matters as much as the cases.
- Severity: `must` is 1.0 and `should` is 0.5; report raw and weighted.
- Parse errors, refusals and timeouts are counted separately and never dropped.
- Three runs; a case that changes its answer between runs is unstable.

The first local run's misses are the useful part: it returned `{"defects": []}`
on `art-before` and `holder-before`, so the corpus is measuring a real
limitation, not a prompt quirk.

### 8. Cost and failure semantics

- Local: wall clock and memory only. A per-case timeout is a recorded failure,
  not a retry.
- Frontier: a per-case output-token limit and a per-run cost ceiling. `--dry-run`
  prints the number of calls and the estimated cost and sends nothing. Log usage
  per case; no hidden retries.
- Every failure is a row: parse error, request error, timeout, missing image,
  capability mismatch. The run continues and the summary says how many were not
  scored.
- Keys come from the environment, never from `models.toml` or the corpus.

### 9. Artifacts

```
.tmp/review-runs/<model>/<run>/<slug>.json
.tmp/review-runs/<model>/summary.json
.tmp/review-runs/<model>/summary.txt
```

Each case artifact holds the slug, provider, model, run, elapsed time, prompt
and image hashes, the typed answer, the score and any error. The summary is a
table: per case detection, IoU, false positives, latency, parse error; then the
aggregate. The corpus stays tracked and model-agnostic; the answers stay in
`.tmp`.

The model is named by its registry tag, not its digest: a tag can be repointed
(`qwen3-vl:30b` was retagged `-instruct`), so a run's weights are only as pinned
as the tag. Capturing the provider's digest is a follow-up.

### 10. Crate boundary

`playbook` keeps its empty `[dependencies]`. The runner is a separate package
under `integrations/`, next to `rig-eval`, and may depend on `rig-core`, serde,
toml and tokio. It is not a dependency of `playbook` or of the agent toolkit,
and it is not installed into every devshell.

`integrations/` rather than `tools/` because this is an experiment with heavy
dependencies, not a daily tool. If it becomes a routine gate, moving it to
`tools/` is a deliberate decision with its own cost.

### 11. No Python

The Python reference runner that produced the first results was a stopgap and is
removed. Model I/O, image handling, parsing and scoring are Rust through
`rig-core`. Introspection (digest, quantisation, resident size) goes through the
Ollama HTTP API, not by parsing CLI output.

### 12. Architecture and platform are provenance

The renderer differs by architecture and OS, and the storybook keeps
`tests/goldens-<arch>/<slug>/<os>.png`. A case records `arch` and `os`; a corpus
is one architecture and one OS unless a comparison says otherwise. Do not mix
aarch64 and x86_64 images, or macOS and Linux ones, in one run and call the
difference a model result.

## The runner shape

```sh
# from the agent-playbook repo root
cargo run --manifest-path integrations/review-bench/Cargo.toml -- \
  --corpus ../macroquad-kit/ui/macroquad-storybook/review \
  --model local-30b --runs 3 --out .tmp/review-runs
```

- `--model` resolves a registry entry; `--runs` defaults to 1 for a probe and 3
  for a comparison.
- `--only` filters a slug; `--dry-run` prints the plan; `--allow-heavy` permits
  the dense 32B.
- The registry lives in `integrations/review-bench/models.toml`. A model entry
  carries the provider, wire model, context, sampling, thinking, resident budget
  and, for frontier arms, the token/cost ceiling.
- The output is `summary.json` and `summary.txt`; the per-case answers are JSON
  files beside them.

## Non-goals

- Not a merge gate. Goldens and the text report are the gate; a model verdict is
  a review aid.
- Not an agent. It does not decide which cases matter, read the answers, or tune
  the prompt until it passes.
- Not a model leaderboard. Thirteen cases on one machine is a probe; a ranking
  needs a corpus that grows with the storybook.
- Not a replacement for the text report. The report catches what the model
  missed in the first run, including the spacing and fragment cases.
- Not a reason to keep a heavy local model resident. The 32B dense is a
  deliberate one-off.

## What would make this not worth it

- The local 30B stays at two detections in five, with every clean screen
  reported, and the frontier arm costs more than a human review for the same
  stories. Then the answer is the deterministic report plus a human pass, and the
  corpus remains a model-selection tool.
- The corpus does not predict new stories. A model that wins here and misses the
  next composition is a corpus problem; extend the corpus before trusting the
  number.
- The provider's image path downscales or drops images. A local result compared
  against a frontier result that saw a different image is not a comparison.
- The models' boxes are noise. If IoU is near zero across the board, score
  detection only and say so.

## Open decisions

- **A minimum overlap for a detection.** The rule as written counts any
  intersection, so `holder-before` scores as detected at IoU 0.01 and a box over
  the whole screen would match everything. The candidate rule is IoU ≥ 0.1, or a
  report whose centre falls inside the expected box when that box is small.
  Decide before comparing models across runs.
- **`other` matching.** Accept `other` for any expected kind, or require its note
  to name the expected defect. The first run did not exercise it; decide when a
  model uses it.
- **The 32B.** Keep it behind `--allow-heavy`, or drop it. It is not a local
  baseline on this machine.
- **The shell nav band.** The 34-point band under the bottom bar is a design
  question, not a rendering defect; it becomes a case when it is decided.
- **Where the runner lives long-term.** `integrations/` is the experiment shelf;
  a routine gate belongs in `tools/` with a dependency decision.
- **Frontier arms.** DeepSeek Flash is the baseline used by `rig-eval`;
  Anthropic and OpenAI are the same seam. Record the vendor and price tier, and
  do not compare a local run against a different serving of the same name.

## Reading map

- `~/code/defrag/macroquad-kit/ui/macroquad-storybook/review/README.md` — corpus
  layout, provenance and scoring.
- `~/code/defrag/macroquad-kit/ui/macroquad-storybook/review/RUBRIC.md` — the
  fixed prompt.
- `rig-eval.md` — the coding-agent runner whose `RunResult` and report shape this
  should mirror.
- `harness-comparison.md` — the fairness controls for comparing harnesses.
- `../local-models.md` — the measured local-model table, and why context and
  keep-alive dominate.
- `rig.md` — the rig seam and the three layers this runner deliberately does not
  use.
