# Comparing the harnesses

The rig-eval runner measures two rig homes. The question that motivated it is wider: does our
harness — rig plus the playbook plugin — do better than the harnesses we might otherwise use?
This is the design for the four-way comparison, the fairness controls it needs, and what each tool
can actually report.

## The arms

| Arm | What runs | How it is driven |
| --- | --- | --- |
| `rig:gated` | `rig` + `PlaybookPlugin`: the `at-*` tools, the redirect, the Gate | `rig -p --json` |
| `rig:baseline` | `rig` + `ToolsOnlyPlugin`: the same tools, no Gate | `rig -p --json` |
| `rig:vanilla` | `rig` with no extra plugin | `rig -p --json` |
| `opencode` | opencode 1.19.5 | `opencode run --format json --auto` |
| `pi` | Pi coding agent 1.5.0 | `pi --mode json` |
| `zed` | Zed's built-in agent | interactive; audited from `threads.db` |

`rig:vanilla` is the control that separates "rig" from "our plugin". Without it, a win could be the
harness or the policy layer, and we could not say which.

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
- **One worktree state.** The arms run sequentially in the same checkout. `at-recall state` is
  recorded before and after each arm; a harness that writes changes the next arm and its run is
  flagged rather than compared.
- **Same instructions.** Every arm reads `AGENTS.md` — opencode and Pi do it by default, and Pi is
  not given `-nc`. The comparison is therefore harness plus enforcement, not knowledge of the
  rules. If the goal later becomes "no rules at all", that is a second run with the context file
  removed from every arm.
- **Same prompt.** The scenario prompt verbatim, with a tag only where a harness needs one to find
  the run afterwards (Zed).

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
wall time. It will not say "harness X is better" from five scenarios on one model. The value is
the shape of the difference: whether the playbook plugin changes the call count or the output
bytes, whether opencode and Pi reach the same answer with fewer calls, and where Zed's interactive
agent lands.

The result is also allowed to be that the harnesses are within noise of each other. Five scenarios
is a probe, not a benchmark; a difference that does not survive the long-thread corpus is not a
result.

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
