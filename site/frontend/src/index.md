# agent-playbook benchmarks

The `at-*` toolkit is a read layer for a repository: a handful of small, typed tools that answer
"where is this", "what changed", "what does this say" without a shell pipeline. It ships as a CLI
and as a rig plugin.

This site measures whether that helps, and what it costs. Each **arm** is a harness given the
same corpus and the same model. Some arms have the CLI on `PATH`; some also have the toolkit
advertised in the repository; some run the rig plugin. The method page is the contract.

**[How this was measured](./method/)** — the corpus, the checks, the isolation, the cost
normalisation and the limits. Read it before a number.

## Published runs

```js
const runs = FileAttachment("data/runs.json").json();
```

```js
if (runs.length === 0) {
  display(html`<p>No measured run has been published yet.</p>`);
} else {
  display(
    Inputs.table(
      runs.map((run) => ({
        run: html`<a href="./runs/${run.id}/">${run.label}</a>`,
        arms: run.arms.join(", "),
        "pass rate": run.checks
          ? `${Math.round((100 * run.passed) / run.checks)}%`
          : "—",
        cost: run.cost_usd ? `$${run.cost_usd.toFixed(3)}` : "—",
        corpus: run.corpus,
      })),
      {
        header: {
          run: "Run",
          arms: "Arms",
          "pass rate": "Pass rate",
          cost: "Cost",
          corpus: "Corpus",
        },
        sort: "run",
        reverse: true,
      },
    ),
  );
}
```

## What this is not

- **Not a leaderboard.** One model per run, one corpus, one platform. A different model is a
  different experiment.
- **Not a coding benchmark.** The work is reading a repository and answering with citations.
- **Not a claim about your repository.** The corpus is this repository's design notes, rules and
  integration code.

Every figure is generated from a run artefact. The only hand-written thing on a run page is its
claim; if one is missing, the page says so.
