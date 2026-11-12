# Runs

Every published measurement, newest first. A run page carries the generated tables; the notebook
beside it is the same data as an interactive page.

```js
const runs = FileAttachment("../data/runs.json").json();
```

```js
if (runs.length === 0) {
  display(html`<p>No measured run has been published yet.</p>`);
} else {
  display(
    Inputs.table(
      runs.map((run) => ({
        run: html`<a href="./${run.id}/">${run.label}</a>`,
        model: run.model,
        arms: run.arms.join(", "),
        runs: run.runs,
        "pass rate": run.checks
          ? `${Math.round((100 * run.passed) / run.checks)}%`
          : "—",
        cost: run.cost_usd ? `$${run.cost_usd.toFixed(3)}` : "—",
        corpus: run.corpus,
        notebook: html`<a href="./${run.id}/notebook">notebook</a>`,
      })),
      {
        header: {
          run: "Run",
          model: "Model",
          arms: "Arms",
          runs: "Runs",
          "pass rate": "Pass rate",
          cost: "Cost",
          corpus: "Corpus",
          notebook: "",
        },
        sort: "run",
        reverse: true,
      },
    ),
  );
}
```
