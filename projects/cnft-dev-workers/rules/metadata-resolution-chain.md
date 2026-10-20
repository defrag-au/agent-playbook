---
id: cnft-metadata-resolution-chain
title: Metadata resolves through the chain, never through collection-ownership
layer: project
activation: project:cnft-dev-workers
priority: 55
overrides:
targets:
---

## Directive

**A metadata consumer resolves through the shared chain — `asset-metadata` over an index and its
chain fallback — never through the collection-ownership DO.**

- The IIIF worker, viewers and anything else that needs an image URI, media type or traits call
  `asset-metadata`, which dispatches by `AssetKey`/`TokenRef`.
- The collection-ownership DO is an ownership projection and a *consumer* of metadata. It is
  not a metadata source; no reader depends on its tables or on `/api/bundle`.
- Each chain gets the same shape: an index first, then a chain fallback. Cardano is
  `assets-resolve` → `asset-db` → Koios. EVM is an EVM metadata index → an EVM RPC or OpenSea.
  Solana and Midnight get their own index-plus-fallback arms, not a shared ownership read.
- A chain with no source bound is an explicit `NoSource` error. It is not a reason to read
  another worker's projection.

## Why

On 2026-10-18 the first EVM adapter in the IIIF worker read the marketplace image URL from the
collection-ownership DO's `/api/bundle`. That coupled a renderer to an ownership projection: the
image existed only if the collection had been onboarded and its DO had run a metadata pass, and
the read could not answer for a chain the ownership worker had not imported. The Cardano path
already had the right shape — `asset-metadata` over `assets-resolve` and Koios — and the EVM path
should have mirrored it. The correction: consumers depend on the metadata chain, and ownership
consumes it too.

## Applies to

`cnft.dev-workers`. The rule is project-scoped because the chain's crates and service bindings
are local to this workspace; if another repo starts consuming `asset-metadata`, move it to
`rules/org/defrag/` and change `activation:`.
