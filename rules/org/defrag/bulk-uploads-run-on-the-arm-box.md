---
id: defrag-bulk-uploads-run-on-the-arm-box
title: Bulk uploads to R2 run on the arm box, not the workstation
layer: org
activation: org:defrag
priority: 45
overrides:
targets:
---

## Directive

**Build and verify on the workstation; run the transfer on the arm box.** The workstation's
uplink sets the ceiling for a GB-scale write to R2, so a large upload from here is a validation
step and never the production one.

| step | where |
| --- | --- |
| build the artifact · dry-run the publish · check the bytes | workstation |
| the `rclone`/`publish` run that moves GB to R2 | arm box |

- `--run`-gated publishers (`mithril-snapshot publish`, `tx-index publish`) print their exact
  `rclone` invocation without `--run` · copy that line to the box rather than re-running it here.
- The box already holds the chunk store, so an artifact derived from chunks is **rebuilt** there,
  never copied to it — copying it spends the same uplink twice.
- Native build on the box, not `aarch64-unknown-linux-musl`: the musl build measured 2.2× slower
  on one thread and *slower with eight than with one*.

## Rationale

Measured 2026-09-27, uploading `tx-index`'s `base.idx` (2.9 GiB) from the workstation to
`r2:mithril-snapshots`:

- **2.68 MiB/s (~21 Mbps)** — the home uplink, not R2. ETA 16 minutes, stopped at 348 MiB.
- The stop left **no object** (R2 replaces objects atomically) but an **unfinished multipart
  upload**: `rclone backend list-multipart-uploads r2:mithril-snapshots` showed
  `index/tx/v1/9201/base.idx`, initiated `2026-09-26T22:53:04Z`. `rclone copyto` **restarts** a
  multipart upload rather than resuming it, so an interrupted run costs everything it had already
  sent, and its parts are billed until aborted.

  ⚠️ **Aborting one is not the obvious command.** `rclone cleanup` and `rclone backend cleanup`
  both exit 0 having done nothing, because they **ignore any upload younger than 24 h** — the log
  says so and nothing else does (`ignoring pending multipart upload … (35m13s ago)`). The working
  form is `rclone backend cleanup -o max-age=0 <remote>:<bucket>`. Confirmed on the one left by
  this incident: the same key, then zero pending uploads, with the mirror's 27,612 objects and
  216.547 GiB unchanged.
- The hazard is specific to derived artifacts: the base is 2.9 GiB, above rclone's 200 MiB
  `upload_cutoff`, so it is always a multipart upload, while every chunk-mirror object is under
  128 MiB and is a single PUT.

The arm box is a Netcup ARM VPS (12× aarch64, 24 GB) that already holds the chunk store at
`/opt/market-ledger/snapshot-full/db/immutable` — 214.86 GiB at the §5b measurement, 217 GiB and
chunks `00000`–`09203` on 2026-09-27 — read at **725 MB/s**, with the note "disk is not the
constraint on this box". Rebuilding an index there costs minutes of its CPU and uploads on its
uplink, at no point crossing the workstation's.

**Measured 2026-09-27, the same work done both ways.** A 2.9 GiB `tx-index` base:

| | workstation | arm box |
|---|---|---|
| upload | 2.68 MiB/s · stopped at 348 MiB after 2 min | **73.65 MiB/s · 2.835 GiB in 46.8 s** |
| build | 11m52s (extraction 2m, compaction 540 s) | 12m37s (extraction 3m33s, compaction 543 s) |

The box's build is not faster — its single-thread compaction matched the workstation's to within
0.5%, which is the useful surprise. **The upload is the whole difference: 27× the throughput.**

⚠️ That same section is why the build must be native there: the `aarch64-unknown-linux-musl`
cross-build was **slower with eight threads than with one** — 150 chunks in 31.7 s → 70.9 s,
against glibc's 14.2 s → 1.9 s — with 370k context switches/s at 98% idle disk, i.e. allocator
contention. The allocator is part of the measurement.
