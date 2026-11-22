# Published runs

Each directory is one measured run, written by:

```sh
cargo run --manifest-path integrations/rig-eval/Cargo.toml -- \
  --out .tmp/rig-eval-<name> --publish site/artifacts ...
```

The directory name is the run id: `run-<started_ms>-<corpus7>-<model-slug>`.

| File | What it is |
| --- | --- |
| `manifest.json` | identity: corpus fingerprint, checkout, snapshot, arms, model, totals, `schema_version` |
| `summary.md` | the generated tables |
| `traces.json` | one row per run, public projection: no answers, no session ids |
| `tools.json` | one row per `(scenario, arm, repeat, tool)` |
| `checks.json` | one row per check |
| `observable.md` | the notebook source |
| `claim.md` | authored, optional; copied by the publish step when present |

`claim.md` is the only file here a person writes. If it is absent, the run page says so.

A run whose checkout was dirty, or whose result was contaminated, is not published. Those are
development runs; the site states what it measured or it does not publish.

The raw streams and effect logs are **not** here. They stay in the private run directory; the site
publishes the scored rows only.
