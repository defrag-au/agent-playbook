# Runs

Every published measurement, newest first. A run page leads with its charts; the generated
tables are a second page beside it.

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
        id: run.id,
        date: run.label.split(" · ")[0],
        model: run.label.split(" · ")[1],
        arms: run.arms.length,
        runs: run.runs,
        "pass rate": run.checks ? `${Math.round((100 * run.passed) / run.checks)}%` : "—",
        cost: run.cost_usd ? `$${run.cost_usd.toFixed(3)}` : "—",
      })),
      {
        columns: ["date", "model", "arms", "runs", "pass rate", "cost"],
        width: {date: 100, model: 210, arms: 55, runs: 55, "pass rate": 80, cost: 75},
        header: {
          date: "Date",
          model: "Model",
          arms: "Arms",
          runs: "Runs",
          "pass rate": "Pass rate",
          cost: "Cost",
        },
        // A cell that holds a DOM node is appended as-is only through a formatter; the
        // default formatter stringifies it, which renders a link as its href.
        format: {
          model: (label, i, data) =>
            html`<a href="./${data[i].id}/">${label}</a>
              <a class="small" href="./${data[i].id}/tables">tables</a>`,
        },
        sort: "date",
        reverse: true,
      },
    ),
  );
}
```
