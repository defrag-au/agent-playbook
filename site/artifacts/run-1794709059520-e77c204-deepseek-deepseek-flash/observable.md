# rig-eval · Observable

A published run is three arrays of records and the charts that read them.

- `traces.json` — one row per run: `outcome`, `work`, `usage`, `toolkit`, `status`.
- `tools.json` — one row per `(scenario, target, repeat, tool)` with `count`.
- `checks.json` — one row per check: `required`, `source`, `found`.

```js
const traces = FileAttachment("traces.json").json();
const tools = FileAttachment("tools.json").json();
const checks = FileAttachment("checks.json").json();
```

```js
const runs = traces.map((d) => ({
  scenario: d.scenario,
  target: d.target,
  repeat: d.repeat,
  score: d.outcome.value,
  passed: d.outcome.passed,
  checks: d.outcome.total,
  solved: d.outcome.total > 0 && d.outcome.passed === d.outcome.total,
  model_calls: d.work.model_calls,
  tool_calls: d.work.tool_calls,
  wall_ms: d.work.wall_ms,
  compactions: d.work.compactions,
  unparsed: d.status.unparsed_lines,
  output_bytes: d.usage.output_bytes,
  input_tokens: d.usage.input_tokens,
  output_tokens: d.usage.output_tokens,
  cached_input_tokens: d.usage.cached_input_tokens,
  cost_usd: d.usage.cost_usd,
  plugin_calls: d.toolkit.plugin_calls,
  used_plugin: d.toolkit.used_plugin,
  used_cli: d.toolkit.used_cli,
  cli_calls: d.toolkit.cli_calls,
  loaded_skill: d.toolkit.loaded_skill,
  // Billed dollars per passed check, and the share of input served from cache. Both are null
  // where they cannot be stated, so a missing price is not plotted as a free run.
  cost_per_check:
    d.outcome.passed > 0 && d.usage.cost_usd > 0
      ? (1e6 * d.usage.cost_usd) / d.outcome.passed
      : null,
  cache_pct:
    d.usage.input_tokens > 0
      ? (100 * d.usage.cached_input_tokens) / d.usage.input_tokens
      : null,
  tags: d.tags,
}));

// The ladder order is the run's own arm order — cheapest intervention first. Every chart takes
// its domain from here, so a colour means the same arm on every figure.
const arms = [...new Set(runs.map((d) => d.target))];
const label = (target) => target.replace(/^opencode:/, "oc:");
const scenarios = [...new Set(runs.map((d) => d.scenario))];
```

```js
const perArm = arms.map((target) => {
  const arm = runs.filter((d) => d.target === target);
  const solved = arm.filter((d) => d.solved).length;
  const passed = d3.sum(arm, (d) => d.passed);
  const checks = d3.sum(arm, (d) => d.checks);
  const cost = d3.sum(arm, (d) => d.cost_usd);
  return {
    target,
    runs: arm.length,
    solved,
    solvedPct: arm.length ? (100 * solved) / arm.length : null,
    passPct: checks ? (100 * passed) / checks : null,
    costPerSolved: solved ? (1e6 * cost) / solved : null,
    costPerCheck: passed ? (1e6 * cost) / passed : null,
  };
});

const contaminated = runs.filter((d) => d.tags.length > 0);
const unparsed = d3.sum(runs, (d) => d.unparsed);
const oneRepeat = d3.min(runs, (d) => d.repeat) === d3.max(runs, (d) => d.repeat);
```

```js
if (contaminated.length > 0 || unparsed > 0 || oneRepeat) {
  display(
    html`<div class="note" style="border-left: 4px solid var(--theme-foreground-focus); padding-left: 1rem;">
      <strong>Read the limits before the figures.</strong>
      ${contaminated.length > 0
        ? html`<div>${contaminated.length} run(s) are contaminated and are excluded from the tables; they are still plotted here, tagged.</div>`
        : ""}
      ${unparsed > 0
        ? html`<div>${unparsed} stream line(s) did not parse, so a count on this page may be short.</div>`
        : ""}
      ${oneRepeat ? html`<div>One run per arm: this is a smoke test, not a rate.</div>` : ""}
    </div>`,
  );
}
```

## What a solved run cost

Billed microdollars per run that passed every scored check, with the arm's solved count. Bar
length is the invoice; the count is the denominator.

```js
Plot.plot({
  width,
  marginLeft: 100,
  marginRight: 98,
  height: 40 + arms.length * 34,
  x: {label: "µ$ per solved run (1 µ$ = $0.000001)", grid: true, tickFormat: d3.format(",")},
  y: {label: null, domain: arms.map(label)},
  color: {domain: arms},
  marks: [
    Plot.barX(perArm, {x: "costPerSolved", y: (d) => label(d.target), fill: "target", rx: 2}),
    Plot.text(perArm, {
      x: "costPerSolved",
      y: (d) => label(d.target),
      dx: 6,
      textAnchor: "start",
      fontSize: 11,
      text: (d) =>
        d.costPerSolved == null
          ? "no solved run"
          : `$${(d.costPerSolved / 1e6).toFixed(3)} · ${d.solved}/${d.runs}`,
    }),
    Plot.ruleX([0]),
  ],
})
```

## Every run, one dot

The bars above are aggregates. This is the evidence under them: one dot per run, cost per passed
check against score. An arm whose dots sit high and left is cheaper for the same work; an arm
whose dots are spread is noisy, and its aggregate should be read with that in mind.

```js
Plot.plot({
  width,
  height: 300,
  marginLeft: 60,
  grid: true,
  x: {label: "µ$ per passed check", tickFormat: d3.format(",")},
  y: {label: "score", domain: [0, 1.08]},
  color: {domain: arms, legend: true, tickFormat: label},
  marks: [
    Plot.ruleY([1], {stroke: "var(--theme-foreground-faintest)", strokeDasharray: "2,3"}),
    Plot.dot(runs.filter((d) => d.cost_per_check != null), {
      x: "cost_per_check",
      y: "score",
      fill: "target",
      r: 4,
      fillOpacity: 0.6,
      stroke: "var(--theme-background)",
      strokeWidth: 0.6,
      tip: true,
      channels: {scenario: "scenario", repeat: "repeat"},
    }),
  ],
})
```

## Where the ladder separates

One row per scenario, one column per arm, sorted by the spread between arms. A full dot is every
run solved; a small, faint dot is none. Rows at the top are the ones this corpus can tell apart;
rows at the bottom score the same everywhere and separate nothing.

```js
const cells = arms.flatMap((target) =>
  scenarios.map((scenario) => {
    const cell = runs.filter((d) => d.target === target && d.scenario === scenario);
    const solved = cell.filter((d) => d.solved).length;
    return {
      target,
      scenario,
      runs: cell.length,
      solved,
      solvedRate: cell.length ? solved / cell.length : null,
      score: d3.mean(cell, (d) => d.score) ?? null,
    };
  }),
);

const scenarioOrder = Array.from(d3.group(cells, (d) => d.scenario), ([scenario, group]) => ({
  scenario,
  spread: (d3.max(group, (d) => d.score) ?? 0) - (d3.min(group, (d) => d.score) ?? 0),
}))
  .sort((a, b) => d3.descending(a.spread, b.spread) || d3.ascending(a.scenario, b.scenario))
  .map((d) => d.scenario);
```

```js
Plot.plot({
  width,
  marginLeft: 158,
  marginTop: 28,
  height: 60 + scenarioOrder.length * 24,
  x: {label: null, axis: null},
  y: {label: null, domain: scenarioOrder},
  color: {domain: arms, legend: true, tickFormat: label},
  marks: [
    Plot.dot(cells, {
      x: (d) => label(d.target),
      y: "scenario",
      r: (d) => (d.runs ? 2.5 + 7 * d.solvedRate : 0),
      fill: "target",
      fillOpacity: (d) => (d.runs ? 0.3 + 0.7 * d.solvedRate : 0),
      stroke: "var(--theme-background)",
      strokeWidth: 1,
      tip: true,
      channels: {
        solved: (d) => (d.runs ? `${d.solved}/${d.runs} solved` : "not run"),
        mean_score: (d) => (d.score == null ? "—" : d.score.toFixed(2)),
      },
    }),
  ],
})
```

## How the arms reached the toolkit

Share of runs in which the arm used the rig plugin, attempted an `at-*` CLI call, or loaded a
skill. The `absent` arm attempts the CLI and gets exit 127 — an attempt is a reach, not a
success — which is why its CLI bar is full and its plugin bar is empty.

```js
const adoption = arms.flatMap((target) => {
  const arm = runs.filter((d) => d.target === target);
  const share = (predicate) => (arm.length ? (100 * arm.filter(predicate).length) / arm.length : null);
  return [
    {target, channel: "plugin", rate: share((d) => d.used_plugin)},
    {target, channel: "CLI attempt", rate: share((d) => d.used_cli)},
    {target, channel: "skill", rate: share((d) => d.loaded_skill)},
  ];
});
```

```js
Plot.plot({
  width,
  marginTop: 30,
  marginLeft: 100,
  height: 220,
  x: {label: null, axis: "top", domain: ["plugin", "CLI attempt", "skill"]},
  y: {label: null, domain: arms.map(label)},
  marks: [
    Plot.cell(adoption, {
      x: "channel",
      y: (d) => label(d.target),
      fill: (d) => (d.rate >= 50 ? "#2b5c8a" : "#e6edf4"),
      inset: 1,
    }),
    Plot.text(adoption, {
      x: "channel",
      y: (d) => label(d.target),
      text: (d) => (d.rate == null ? "\u2014" : `${Math.round(d.rate)}%`),
      fill: (d) => (d.rate >= 50 ? "white" : "#0b0b0b"),
      fontSize: 11,
    }),
  ],
})
```

## What the tools were

Every tool call, split into the `at_*` plugin tools and everything else. This is where a plugin
arm spends its calls, and whether a CLI arm's calls went through a shell at all.

```js
const toolMix = arms.flatMap((target) => {
  const rows = tools.filter((d) => d.target === target);
  const total = d3.sum(rows, (d) => d.count);
  const plugin = d3.sum(
    rows.filter((d) => d.tool.startsWith("at_")),
    (d) => d.count,
  );
  return [
    {target, part: "at-* plugin", count: plugin},
    {target, part: "everything else", count: total - plugin},
  ];
});
```

```js
Plot.plot({
  width,
  marginLeft: 100,
  height: 40 + arms.length * 34,
  x: {label: "tool calls", grid: true, tickFormat: d3.format(",")},
  y: {label: null, domain: arms.map(label)},
  color: {domain: ["at-* plugin", "everything else"], legend: true},
  marks: [
    Plot.barX(toolMix, Plot.stackX({x: "count", y: (d) => label(d.target), fill: "part"})),
    Plot.ruleX([0]),
  ],
})
```

## What the model read

Input tokens across the arm's runs, split into the part the provider served from its prompt cache
and the part it billed fresh. Cache share is why a model-call count is not a cost: two arms can
make the same calls and pay very differently.

```js
const tokens = arms.flatMap((target) => {
  const arm = runs.filter((d) => d.target === target);
  const total = d3.sum(arm, (d) => d.input_tokens);
  const cached = d3.sum(arm, (d) => d.cached_input_tokens);
  return [
    {target, part: "cached", tokens: cached},
    {target, part: "fresh", tokens: total - cached},
  ];
});
```

```js
Plot.plot({
  width,
  marginLeft: 100,
  height: 40 + arms.length * 34,
  x: {label: "input tokens", grid: true, tickFormat: d3.format("~s")},
  y: {label: null, domain: arms.map(label)},
  color: {
    domain: ["cached", "fresh"],
    range: ["var(--theme-foreground-faint)", "var(--theme-foreground)"],
    legend: true,
  },
  marks: [
    Plot.barX(tokens, Plot.stackX({x: "tokens", y: (d) => label(d.target), fill: "part"})),
    Plot.ruleX([0]),
  ],
})
```

## What was missed

Scored checks only — tool evidence is not a score. Counts are the runs that missed the check, so
a row of zeros is a check every run passed and is not listed.

```js
const missed = Array.from(
  d3.rollup(
    checks.filter((d) => !d.found && d.source !== "tool"),
    (rows) => rows.length,
    (d) => `${d.scenario}\u0000${d.required}`,
    (d) => d.target,
  ),
  ([key, byTarget]) => {
    const [scenario, check] = key.split("\u0000");
    return {
      scenario,
      check,
      missed: d3.sum(Array.from(byTarget.values())),
      ...Object.fromEntries(arms.map((arm) => [arm.split(":").pop(), byTarget.get(arm) ?? 0])),
    };
  },
).sort((a, b) => d3.descending(a.missed, b.missed));
```

```js
// The arm columns are short labels; give them a width each or Inputs.table squeezes the
// header to "abs..." and leaves the numbers readable but the column unidentified.
const missWidths = Object.fromEntries([
  ["scenario", 170],
  ["check", 120],
  ["missed", 55],
  ...arms.map((arm) => [arm.split(":").pop(), 55]),
]);
```

```js
Inputs.table(missed, {
  sort: "missed",
  reverse: true,
  width: missWidths,
})
```

The generated tables — pass rate, score spread, cost matrices, diagnostics and the limits — are
on the [tables page](./tables).
