# rig-eval · deepseek/deepseek-flash

Corpus `e77c204b536b87bc75367da1c9ed1da7dcd6c007` (13 files) · checkout `c0bcfec` on `damon-mcp` · snapshot `e037546` (advertised) · `506dccb` (unadvertised) · runner rig-eval 0.1.0

Isolation container `playbook-eval/rig-eval:local` (sha256:823e47f93f048c78f4ac8da9ea4a8221d1d3edb5f8feaf7adb801cc2978c2979) · image built from `c0bcfec`

104 runs · 4 arms · 656/728 checks passed (90%) · $0.599 · 13.8M input (88% cached)

`Cost/check` is billed microdollars per passed check; `Cost/solved` is billed microdollars per run that passed every check. `Cache` is the share of input tokens read from the provider's prompt cache. A dash means it cannot be stated. A range is min–max across the arm's runs.

## By arm

| Arm | Runs | Pass rate | Solved | Score | Cost/check (µ$) | Cost/solved (µ$) | Cache |
| --- | --- | --- | --- | --- | --- | --- | --- |
| rig:gated | 26 | 92% | 17/26 (65%) | 1.00 [0.50–1.00] | 619 [162–6180] | 7329 | 90% |
| rig:baseline | 26 | 89% | 18/26 (69%) | 1.00 [0.50–1.00] | 580 [239–3472] | 6331 | 90% |
| opencode:documented | 26 | 87% | 15/26 (58%) | 1.00 [0.33–1.00] | 678 [102–3573] | 7222 | 86% |
| pi:documented | 26 | 93% | 16/26 (62%) | 1.00 [0.75–1.00] | 1261 [277–8630] | 15750 | 83% |

## Work

| Arm | Model calls | Tool calls | Input | Output | Wall (median) | Precision |
| --- | --- | --- | --- | --- | --- | --- |
| rig:gated | 9 | 13 | 116k | 39.7 KB | 25.4s | 0% |
| rig:baseline | 8 | 11 | 91k | 40.7 KB | 24.7s | 0% |
| opencode:documented | 5 | 9 | 112k | 60.4 KB | 17.1s | 0% |
| pi:documented | 7 | 11 | 89k | 58.9 KB | 21.3s | 0% |

## Diagnostics

| Arm | Refused | Failed | Cancelled | Compactions | Unparsed |
| --- | --- | --- | --- | --- | --- |
| rig:gated | 8 | 12 | 0 | 0 | 0 |
| rig:baseline | 0 | 6 | 0 | 0 | 0 |
| opencode:documented | 0 | 0 | 0 | 0 | 0 |
| pi:documented | 0 | 2 | 0 | 0 | 0 |

`Unparsed` counts stream lines the parser did not understand; a non-zero value means the counts are incomplete. `Refused`, `Failed` and `Cancelled` are tool-result counts.

## Adoption

| Arm | Runs | Plugin tools | Plugin calls/run | CLI attempts | CLI attempts/run | Skill |
| --- | --- | --- | --- | --- | --- | --- |
| rig:gated | 26 | 26/26 (100%) | 5 | 4/26 (15%) | 0 | 0/26 (0%) |
| rig:baseline | 26 | 26/26 (100%) | 4 | 3/26 (12%) | 0 | 0/26 (0%) |
| opencode:documented | 26 | 0/26 (0%) | 0 | 4/26 (15%) | 0 | 0/26 (0%) |
| pi:documented | 26 | 0/26 (0%) | 0 | 5/26 (19%) | 0 | 0/26 (0%) |

## Score by scenario

| Scenario | rig:gated | rig:baseline | opencode:documented | pi:documented |
| --- | --- | --- | --- | --- |
| advertising-over-gating | 1.00 | 1.00 | 0.90 [0.80–1.00] | 0.90 [0.80–1.00] |
| devshell-tests | 0.75 [0.50–1.00] | 1.00 | 1.00 | 1.00 |
| draft-pr | 1.00 | 0.90 [0.80–1.00] | 0.80 | 0.80 |
| eval-contamination | 0.86 [0.82–0.91] | 0.77 [0.73–0.82] | 0.86 [0.73–1.00] | 0.95 [0.91–1.00] |
| facts-without-subprocess | 1.00 | 0.81 [0.62–1.00] | 1.00 | 1.00 |
| otel-or-duckdb | 0.94 [0.89–1.00] | 1.00 | 1.00 | 0.94 [0.89–1.00] |
| pre-edit-state | 1.00 | 1.00 | 1.00 | 1.00 |
| recover-investigation | 0.75 [0.50–1.00] | 0.75 | 0.88 [0.75–1.00] | 1.00 |
| redirect-declines | 0.88 [0.83–0.92] | 0.62 [0.50–0.75] | 0.58 [0.33–0.83] | 0.83 [0.75–0.92] |
| rig-vs-factotum | 1.00 | 1.00 | 1.00 | 1.00 |
| the-skipped-rule | 1.00 | 1.00 | 1.00 | 0.92 [0.83–1.00] |
| trace-redirect | 1.00 | 1.00 | 0.75 | 0.75 |
| zed-thread-tools-not-at | 0.77 [0.64–0.91] | 1.00 | 0.82 | 1.00 |

## Solved by scenario

A cell is the runs that passed every scored check over the runs that carried one; the median score above can hide a bimodal arm that this table separates.

| Scenario | rig:gated | rig:baseline | opencode:documented | pi:documented |
| --- | --- | --- | --- | --- |
| advertising-over-gating | 2/2 | 2/2 | 1/2 | 1/2 |
| devshell-tests | 1/2 | 2/2 | 2/2 | 2/2 |
| draft-pr | 2/2 | 1/2 | 0/2 | 0/2 |
| eval-contamination | 0/2 | 0/2 | 1/2 | 1/2 |
| facts-without-subprocess | 2/2 | 1/2 | 2/2 | 2/2 |
| otel-or-duckdb | 1/2 | 2/2 | 2/2 | 1/2 |
| pre-edit-state | 2/2 | 2/2 | 2/2 | 2/2 |
| recover-investigation | 1/2 | 0/2 | 1/2 | 2/2 |
| redirect-declines | 0/2 | 0/2 | 0/2 | 0/2 |
| rig-vs-factotum | 2/2 | 2/2 | 2/2 | 2/2 |
| the-skipped-rule | 2/2 | 2/2 | 2/2 | 1/2 |
| trace-redirect | 2/2 | 2/2 | 0/2 | 0/2 |
| zed-thread-tools-not-at | 0/2 | 2/2 | 0/2 | 2/2 |

## Cost per check by scenario (µ$)

| Scenario | rig:gated | rig:baseline | opencode:documented | pi:documented |
| --- | --- | --- | --- | --- |
| advertising-over-gating | 409 [398–420] | 366 [343–388] | 252 [174–330] | 750 [620–881] |
| devshell-tests | 2672 [1853–3490] | 1219 [1185–1252] | 2751 [1929–3573] | 5369 [2108–8630] |
| draft-pr | 678 [633–722] | 584 [529–639] | 577 [442–712] | 1962 [1955–1969] |
| eval-contamination | 752 [604–901] | 580 [512–647] | 606 [545–667] | 1203 [1044–1361] |
| facts-without-subprocess | 1028 [992–1064] | 1367 [1025–1708] | 657 [613–700] | 1892 [1372–2412] |
| otel-or-duckdb | 270 [267–274] | 263 [255–271] | 196 [183–210] | 646 [385–908] |
| pre-edit-state | 188 [162–214] | 260 [239–281] | 111 [102–121] | 292 [277–306] |
| recover-investigation | 4369 [2557–6180] | 3374 [3277–3472] | 1800 [1233–2368] | 5124 [3522–6726] |
| redirect-declines | 490 [425–554] | 632 [581–684] | 674 [362–987] | 846 [765–928] |
| rig-vs-factotum | 387 [289–484] | 477 [451–502] | 546 [292–799] | 1037 [913–1161] |
| the-skipped-rule | 611 [503–719] | 600 [580–619] | 1245 [1148–1342] | 1722 [1517–1927] |
| trace-redirect | 1516 [1393–1639] | 1337 [1313–1360] | 1694 [1623–1765] | 4534 [3322–5746] |
| zed-thread-tools-not-at | 539 [328–750] | 461 [431–490] | 548 [407–689] | 699 [645–752] |

## Misses

Scored checks only; tool evidence is below. A cell is the runs that missed the check over the runs that carried it.

| Scenario | Arm | Check | Source | Missed |
| --- | --- | --- | --- | --- |
| advertising-over-gating | opencode:documented | `docs/design/rig.md:138` | Gold | 1/2 (50%) |
| advertising-over-gating | opencode:documented | `docs/design/rig.md:145` | Gold | 1/2 (50%) |
| advertising-over-gating | pi:documented | `docs/design/rig.md:138` | Gold | 1/2 (50%) |
| advertising-over-gating | pi:documented | `docs/design/rig.md:145` | Gold | 1/2 (50%) |
| devshell-tests | rig:gated | `0 failed` | Answer | 1/2 (50%) |
| draft-pr | opencode:documented | `tools/at-peek/src/verbs/search.rs` | Path | 2/2 (100%) |
| draft-pr | pi:documented | `tools/at-peek/src/verbs/search.rs` | Path | 2/2 (100%) |
| draft-pr | rig:baseline | `tools/at-peek/src/verbs/search.rs` | Path | 1/2 (50%) |
| eval-contamination | opencode:documented | `docs/design/rig-eval.md:417` | Gold | 1/2 (50%) |
| eval-contamination | opencode:documented | `docs/design/rig-eval.md:418` | Gold | 1/2 (50%) |
| eval-contamination | opencode:documented | `docs/design/rig-eval.md:419` | Gold | 1/2 (50%) |
| eval-contamination | pi:documented | `inspected-corpus` | Answer | 1/2 (50%) |
| eval-contamination | rig:baseline | `integrations/rig-eval/src/sandbox.rs:7` | Gold | 2/2 (100%) |
| eval-contamination | rig:baseline | `docs/design/rig-eval.md:417` | Gold | 1/2 (50%) |
| eval-contamination | rig:baseline | `inspected-corpus` | Answer | 1/2 (50%) |
| eval-contamination | rig:baseline | `integrations/rig-eval/src/sandbox.rs:10` | Gold | 1/2 (50%) |
| eval-contamination | rig:gated | `integrations/rig-eval/src/sandbox.rs:7` | Gold | 2/2 (100%) |
| eval-contamination | rig:gated | `docs/design/rig-eval.md:417` | Gold | 1/2 (50%) |
| facts-without-subprocess | rig:baseline | `integrations/rig/src/tools.rs:310` | Gold | 1/2 (50%) |
| facts-without-subprocess | rig:baseline | `integrations/rig/src/tools.rs:313` | Gold | 1/2 (50%) |
| facts-without-subprocess | rig:baseline | `integrations/rig/src/tools.rs:411` | Gold | 1/2 (50%) |
| otel-or-duckdb | pi:documented | `docs/design/rig.md:199` | Gold | 1/2 (50%) |
| otel-or-duckdb | rig:gated | `docs/design/rig.md:199` | Gold | 1/2 (50%) |
| recover-investigation | opencode:documented | `docs/design/rig.md:136` | Gold | 1/2 (50%) |
| recover-investigation | rig:baseline | `docs/design/rig.md:136` | Gold | 2/2 (100%) |
| recover-investigation | rig:gated | `docs/design/rig.md:136` | Gold | 1/2 (50%) |
| recover-investigation | rig:gated | `integrations/rig/README.md:14` | Gold | 1/2 (50%) |
| redirect-declines | opencode:documented | `integrations/rig/src/redirect.rs:378` | Gold | 2/2 (100%) |
| redirect-declines | opencode:documented | `integrations/rig/src/redirect.rs:406` | Gold | 2/2 (100%) |
| redirect-declines | opencode:documented | `integrations/rig/src/redirect.rs:424` | Gold | 1/2 (50%) |
| redirect-declines | opencode:documented | `integrations/rig/src/redirect.rs:564` | Gold | 1/2 (50%) |
| redirect-declines | opencode:documented | `integrations/rig/src/redirect.rs:585` | Gold | 1/2 (50%) |
| redirect-declines | opencode:documented | `integrations/rig/src/redirect.rs:622` | Gold | 1/2 (50%) |
| redirect-declines | opencode:documented | `integrations/rig/src/redirect.rs:653` | Gold | 1/2 (50%) |
| redirect-declines | opencode:documented | `pipeline` | Answer | 1/2 (50%) |
| redirect-declines | pi:documented | `integrations/rig/src/redirect.rs:378` | Gold | 2/2 (100%) |
| redirect-declines | pi:documented | `integrations/rig/src/redirect.rs:424` | Gold | 1/2 (50%) |
| redirect-declines | pi:documented | `integrations/rig/src/redirect.rs:653` | Gold | 1/2 (50%) |
| redirect-declines | rig:baseline | `integrations/rig/src/redirect.rs:378` | Gold | 2/2 (100%) |
| redirect-declines | rig:baseline | `integrations/rig/src/redirect.rs:406` | Gold | 1/2 (50%) |
| redirect-declines | rig:baseline | `integrations/rig/src/redirect.rs:424` | Gold | 1/2 (50%) |
| redirect-declines | rig:baseline | `integrations/rig/src/redirect.rs:564` | Gold | 1/2 (50%) |
| redirect-declines | rig:baseline | `integrations/rig/src/redirect.rs:585` | Gold | 1/2 (50%) |
| redirect-declines | rig:baseline | `integrations/rig/src/redirect.rs:622` | Gold | 1/2 (50%) |
| redirect-declines | rig:baseline | `integrations/rig/src/redirect.rs:653` | Gold | 1/2 (50%) |
| redirect-declines | rig:baseline | `pipeline` | Answer | 1/2 (50%) |
| redirect-declines | rig:gated | `integrations/rig/src/redirect.rs:378` | Gold | 2/2 (100%) |
| redirect-declines | rig:gated | `pipeline` | Answer | 1/2 (50%) |
| the-skipped-rule | pi:documented | `secret-shaped` | Answer | 1/2 (50%) |
| trace-redirect | opencode:documented | `integrations/rig/src/redirect.rs:95` | Gold | 2/2 (100%) |
| trace-redirect | pi:documented | `integrations/rig/src/redirect.rs:95` | Gold | 2/2 (100%) |
| zed-thread-tools-not-at | opencode:documented | `tools/zed-thread-tools/README.md:75` | Gold | 2/2 (100%) |
| zed-thread-tools-not-at | opencode:documented | `whole-grammar` | Answer | 2/2 (100%) |
| zed-thread-tools-not-at | rig:gated | `docs/inspection-tools.md:875` | Gold | 1/2 (50%) |
| zed-thread-tools-not-at | rig:gated | `docs/inspection-tools.md:876` | Gold | 1/2 (50%) |
| zed-thread-tools-not-at | rig:gated | `docs/inspection-tools.md:877` | Gold | 1/2 (50%) |
| zed-thread-tools-not-at | rig:gated | `tools/zed-thread-tools/README.md:75` | Gold | 1/2 (50%) |
| zed-thread-tools-not-at | rig:gated | `whole-grammar` | Answer | 1/2 (50%) |

## Tool evidence

| Arm | Tool checks seen |
| --- | --- |
| rig:gated | 20/20 (100%) |
| rig:baseline | 20/20 (100%) |
| opencode:documented | 20/20 (100%) |
| pi:documented | 19/20 (95%) |

Tool checks are evidence the work happened; they are not part of the score.

## Limits

- One model: `deepseek/deepseek-flash`. A different model is a different experiment.
- `Cost/check` divides by partial credit; `Cost/solved` divides the arm's total bill by runs that passed every check. They diverge when an arm trades completeness for a smaller bill.
- `CLI attempts` counts a shell call that named an `at-*` binary, not one that succeeded; the `absent` arm is expected to attempt and fail.
- `Skill` counts a harness `skill` call. A non-documented arm that loads one is documented in practice: the repository advertisement is removed, not the harness's own skill store.
- 2 runs per scenario and arm; each cell is the median, with min–max where the runs disagree.
- 2 of 13 scenarios scored 1.00 for every arm; they separate nothing here: pre-edit-state, rig-vs-factotum.
- 2 jobs ran in parallel: wall time is not comparable across arms.

## Inspected the corpus

- devshell-tests · opencode:documented · run 2: a tool call named the sealed corpus; the read was refused.
- recover-investigation · pi:documented · run 1: a tool call named the sealed corpus; the read was refused.

Streams, stderr and effect logs are beside this file as `<scenario>-<target>-r<n>.*`.
