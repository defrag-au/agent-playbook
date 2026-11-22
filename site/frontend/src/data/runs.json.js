// Every published run, newest first, for the home and runs pages.
//
// This is a data loader: Framework runs it at build time and exposes the result
// as `data/runs.json`. It scans `artifacts/` rather than reading a checked-in
// index, so publishing a run never edits a file here.
import { existsSync, readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

const SCHEMA = 1;
const ARTIFACTS = fileURLToPath(new URL("../../../artifacts/", import.meta.url));

const runs = existsSync(ARTIFACTS)
  ? readdirSync(ARTIFACTS, { withFileTypes: true })
      .filter((entry) => entry.isDirectory())
      .map((entry) => entry.name)
      .map((id) => ({ id, dir: join(ARTIFACTS, id) }))
      .filter(({ dir }) => existsSync(join(dir, "manifest.json")))
      .map(({ id, dir }) => {
        const manifest = JSON.parse(readFileSync(join(dir, "manifest.json"), "utf8"));
        if (manifest.schema_version !== SCHEMA) {
          throw new Error(
            `${id}: manifest schema_version ${manifest.schema_version}, this page reads ${SCHEMA}`,
          );
        }
        return {
          id,
          label: `${new Date(manifest.started_ms).toISOString().slice(0, 10)} · ${manifest.model}`,
          started_ms: manifest.started_ms,
          model: manifest.model,
          corpus: manifest.corpus.fingerprint.slice(0, 7),
          checkout: manifest.checkout.head?.slice(0, 7) ?? "unknown",
          arms: manifest.targets.map((arm) => arm.label),
          runs: manifest.totals.runs,
          passed: manifest.totals.passed,
          checks: manifest.totals.checks,
          cost_usd: manifest.totals.cost_usd,
        };
      })
      .sort((a, b) => b.started_ms - a.started_ms)
  : [];

process.stdout.write(JSON.stringify(runs));
