---
id: cnft-d1-write-economics
title: D1 reads are cheap, writes are expensive
layer: project
activation: project:cnft-dev-workers
priority: 54
overrides:
targets:
---

## Directive

D1 billing: reads are cheap, writes are expensive.

- Check existence before writing → a write that would be a no-op never happens, and CONFLICT
  handling is not paid for.
- Avoid triggers (they turn one write into several) · avoid foreign keys (they add write
  complexity) · batch deliberately (many small writes cost more than one larger write).
- Always use the `query!` macro for D1 operations — never manual parameter binding (D1 has
  strict JavaScript type requirements, and the macro is what enforces them).

## Rationale

**The cost model is not the one most engineers assume.** "Write the row and let the database
deduplicate" is the natural instinct from Postgres, and it is the expensive choice here. A
rule that is counter-intuitive is a rule that needs writing down, because the intuitive
answer will be reached independently by every agent that touches this code.

**Why this is a project rule and not an org rule.** D1 is used here. It is not used in
`shared-crates`, so as an org rule it would have been a false positive on every non-worker
repo in the ecosystem. When a second repo starts using D1, move this up to `rules/org/defrag/`
and change `activation:` to `org:defrag`.
