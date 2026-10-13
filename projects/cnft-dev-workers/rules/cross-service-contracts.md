---
id: cnft-cross-service-contracts
title: Cross-service contracts live in code — read the crate, not a doc
layer: project
activation: project:cnft-dev-workers
priority: 45
overrides:
targets:
---

## Directive

Before changing anything another service consumes, read the contract where it is enforced:

- **Wire types and protocols** → the shared crate that defines them in
  `~/code/defrag/shared-crates` (`chain-heartbeat`, `chain-live`, `ownership-client`,
  `gateway-wiring`, `ui-flow-protocol`, `authorizations`, …). Every consumer pins a rev, so a
  rename or a callback-arity change fails to compile downstream — those compile errors and
  test failures *are* the migration list. Grep the consuming repos for the old name.
- **Design intent and the edges** → the repo's own `docs/design/`, the design books in
  `~/code/defrag/docs-and-strategy` (published to docs.augmint.bot), and the `wrangler.toml`
  configs the hostnames and bindings live in.

A change that crosses a service boundary → the update is the rev bump plus every consumer it
compiles against. Do not look for, or write, a central prose copy: the schema source is the
crate, and a prose copy only drifts from it.

`~/code/defrag/ecosystem-docs/` was retired 2026-10-13 (docs-and-strategy
`books/strategy/decisions/0002-retire-ecosystem-docs.md`): unversioned, dungeons-v1-era, and
wrong about the contracts that had actually changed. Do not resurrect it, or anything shaped
like it.

## Rationale

Incident, 2026-10-12: the shared-crates `e0b776d2` bump (#123) renamed a required wire field
(`BundleEntry.owner_stake` → `owner_account`, no serde alias) and changed
`on_action_complete`'s callback arity. Call sites were missed across augminted-bots and only
the cross-repo compile/test pass found them. The folder that was supposed to record the
rename never did — its files were written for `dungeons-v1`, and one of the design docs that
planned the rename still carried an "ecosystem-docs contract update" step that never landed.
The contracts that hold are the ones the compiler checks.
