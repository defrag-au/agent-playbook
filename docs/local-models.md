# Local models for Rust work

Measured on `defrag-primary` (M5 Pro, 20-core GPU, 48 GB unified, macOS 27.0.1) with Ollama
0.34.0 from `nixpkgs`, weights in `~/models/ollama`, 32K context. Kept because the numbers
decided a purchase-sized decision (what fits, what is worth carrying offline) and because
"which local model" is otherwise re-litigated from memory every time.

## Measured

| Model | Resident | Decode, short prompt | Decode, 15k prompt | Prefill, 15k |
| --- | --- | --- | --- | --- |
| `glm-4.7-flash` (30B MoE, 3B active) | 20 GB | 56–61 tok/s | 23.6 tok/s | 231.7 tok/s (64.6 s) |
| `qwen3.8:27b` (27B dense) | 17 GB | 13.8–14.8 tok/s | 14.8 tok/s | 237.6 tok/s (64.9 s) |
| `Qwen3-Coder-REAP-25B-A3B-Rust` (25B MoE, 3B active) | 16 GB | 92 tok/s | 48.9 tok/s | 577.1 tok/s (26.1 s) |

All three run 100% on the GPU. Two results are worth remembering because they invert the
intuition:

- **Context collapses the MoE advantage.** `glm-4.7-flash` is 4× faster than the dense 27B on a
  short prompt and 1.6× faster at 15k tokens: decode reads only 3B of weights, so the KV cache
  becomes a large share of the bytes moved. The dense model is unaffected by context (14.6 → 14.8).
- **Prefill is architecture-dependent, not a fixed ceiling.** `glm-4.7-flash` and `qwen3.8:27b`
  both land at ~235 tok/s, but the pruned `Qwen3-Coder-REAP-25B-A3B` prefills at 577 tok/s — 2.5×.
  The earlier guess that ~235 was an Ollama-side limit was wrong; the reduced expert set is doing
  the work.

A 15k-token prompt is the real cost driver, and it is a **once-per-model-load** cost: the cached
prefix took the 64.6 s ingest to 0.16 s (15,414 of 15,418 tokens). So `OLLAMA_KEEP_ALIVE=30m`
matters more than which model is chosen. Thinking is the other wall-clock cost — GLM answered a
one-sentence question in 0.79 s with `"think": false` against 30 s of generation with it on.

## The multi-root workspace is the real ceiling

Zed loads one instruction file per **worktree root**, and a workspace with 23 roots sums them. The
`cnft.dev-workers` workspace carries **552 KB of rules files across its roots** — the generated
blocks repeat the same core and org rules in every repository — which is ~138,700 tokens at the
3.98 bytes/token this tree tokenises at. Add Zed's tool definitions and agent prompt and a fresh
agent thread sent **149,696 tokens before the user typed anything**, against a 32,768 window.
Three identical retries proved it was the system prompt rather than thread history: a growing
conversation would have climbed between attempts.

This is invisible on a hosted model with a 200k window and fatal on every local one. Fixes, in
order: open a **single root** for local-model work (15k of rules instead of 138k); set
`language_models.ollama.context_window` explicitly — Zed sends it as `num_ctx` per request and
otherwise falls back to a default in the low thousands; and, if local work becomes routine, emit a
compact block for local models rather than inlining the full rules in every repository.

## Rust-specialised weights

The category exists but is shallow: a few thousand downloads, almost no independent evaluation,
and most entries are unbenchmarked one-person fine-tunes.

| Model | Shape | q4 | Base | Why it is or is not interesting |
| --- | --- | --- | --- | --- |
| `Em-80/Qwen3-coder-REAP-25B-A3B-Rust` | 25B MoE, 3B active | 13.4 GB (IQ4_XS) | Qwen3-Coder-30B-A3B → REAP-pruned | Not a fine-tune; a Rust-calibrated quantisation. No behaviour change, so no tool-calling risk. |
| `Fortytwo-Network/Strand-Rust-Coder-14B-v1` | 14B dense | 9.0 GB | Qwen2.5-Coder-14B | Only one with a published eval; base predates edition 2024. |
| `Convence/Aroow-Rust-Coder-9B` | 9B dense | ~5.5 GB | unclear | Thin card, no eval. |
| `Tesslate/Tessa-Rust-T1-7B` | 7B dense | ~4.5 GB | Qwen2.5-Coder | Scores *below* the un-tuned 14B base on its own family's table. |

The lesson from that last row is the one to keep: a Rust fine-tune can be worse than the general
model it came from. A narrow SFT also tends to cost tool calling, which is what an editor agent
needs most.

The REAP build wins on every axis measured here — 16 GB resident against GLM's 20, 92 tok/s
against 56, 577 tok/s prefill against 232 — and it emits well-formed `tool_calls`, so it can drive
an agent panel. It is not in Ollama's library: download the GGUF, write a Modelfile whose `FROM`
names the local file, and `ollama create reap-rust:25b -f Modelfile`. The chat template, tool
format included, comes through from the GGUF metadata.

The reason it is fast is worth remembering when shopping for others: it is a *pruned* 25B of
Qwen3-Coder's original 30B, so prefill and decode both move fewer bytes than the unpruned MoE —
for free, because the pruning removed experts a Rust-heavy calibration set did not need.

## Strandset-Rust-v1 — the open question

`Fortytwo-Network/Strandset-Rust-v1` is the dataset behind `Strand-Rust-Coder-14B-v1`, published
on Hugging Face.

- ~191k examples across 15 task categories: generation, completion, bug detection, refactoring,
  optimisation, docstrings, code review, summarisation, test generation, naming, API usage
  prediction, search.
- Built from 2,383 crates.io crates with compile checks; the card claims a 94.3% compile rate,
  73.2% consensus acceptance and coverage of 89% of Rust language features.
- Used to LoRA-tune `Qwen2.5-Coder-14B-Instruct` (r=64, α=16, 3 epochs, completion-only loss,
  32k context).

Why it is on the list at all: every off-the-shelf option stops where the real work starts. Their
`AGENTS.md` is 15k tokens of house rules, the crates are named `at-peek` and `asset-metadata`, and
no downloaded model has ever seen either. A LoRA over *their* crates plus this dataset is the only
version of "Rust-specialised" that would know the conventions — and 48 GB with MLX is enough to
train it locally.

What to check before committing any time to it:

1. **Dataset licence.** The derived model is Apache 2.0; the dataset's own terms were not verified.
   Matters for anything published, not for an internal experiment.
2. **Whether the win is real.** Fortytwo's benchmark is hold-out data from the same synthetic
   corpus, so it measures the dataset. The only honest test is their own: revert a completed
   refactor and score `cargo check` against the best off-the-shelf model (currently
   `reap-rust:25b`).
3. **Tooling cost.** MLX LoRA on a 14B at 4-bit in 48 GB is feasible; expect hours per run, plus
   the evaluation harness, plus a way to hold out a repo cleanly.
4. **What it would actually displace.** A fine-tune at 14B will still lose to a 27B-class model on
   reasoning. Its value is style, API recall and their own idioms — not the borrow checker.

Decision trigger: only worth starting if the off-the-shelf models fail the `cargo check` A/B on
their own code. If `reap-rust:25b` with the compiler in the loop already produces edits that
compile, the dataset is a research project rather than a fix.
