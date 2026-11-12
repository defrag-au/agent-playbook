// One Framework project, one page per published run.
//
// A run is a directory under `../artifacts/`; `scripts/sync-runs.mjs` writes its
// pages before `observable build` runs. This config only scans the artefacts for
// the sidebar, so publishing a run never edits this file.
//
// `output: "build"` because `build.sh` copies frontend/build/ to dist/ for the
// worker's assets binding — the same arrangement as cnft.dev-workers
// projects/notebooks.
import { existsSync, readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

const ARTIFACTS = fileURLToPath(new URL("../artifacts/", import.meta.url));

const runs = existsSync(ARTIFACTS)
  ? readdirSync(ARTIFACTS, { withFileTypes: true })
      .filter((entry) => entry.isDirectory())
      .map((entry) => join(ARTIFACTS, entry.name))
      .filter((dir) => existsSync(join(dir, "manifest.json")))
      .map((dir) => {
        const manifest = JSON.parse(readFileSync(join(dir, "manifest.json"), "utf8"));
        return {
          id: dir.slice(ARTIFACTS.length).replace(/\/$/, ""),
          label: `${new Date(manifest.started_ms).toISOString().slice(0, 10)} · ${manifest.model}`,
          started: manifest.started_ms,
        };
      })
      .sort((a, b) => b.started - a.started)
  : [];

export default {
  title: "agent-playbook",
  root: "src",
  output: "build",
  theme: ["air", "near-midnight"],
  head: `<link rel="icon" href="/favicon.svg">
    <style>
      /* Framework renders markdown tables at the page width, so at a narrow width the
         right-hand columns are clipped and numbers wrap mid-cell. Give the table its own
         horizontal scroll box: max-content keeps every column on one line, max-width caps
         the box at the page, and overflow scrolls the rest. */
      main.observablehq > table {
        display: block;
        max-width: 100%;
        overflow-x: auto;
        -webkit-overflow-scrolling: touch;
      }
      /* With the table as its own scroll box the column layout is still capped at the
         page width, which wraps numbers mid-cell. nowrap makes each cell its min-content
         width, so the box scrolls instead. */
      main.observablehq > table th,
      main.observablehq > table td {
        white-space: nowrap;
      }
    </style>`,
  toc: true,
  pager: false,
  pages: [
    // First, deliberately: a reader who wants to know whether to trust a number
    // should not have to hunt for the method or the limits.
    { name: "How this was measured", path: "/method/" },
    { name: "Runs", path: "/runs/" },
    ...runs.map((run) => ({ name: run.label, path: `/runs/${run.id}/` })),
  ],
  footer:
    "Every figure is generated from a rig-eval run artefact. The corpus, model, " +
    "checkout and runner commit are named on each run page; the limits are on the " +
    "method page.",
};
