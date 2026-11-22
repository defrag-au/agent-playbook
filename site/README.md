# The benchmark site

`playbook.defrag.au` — an assets-only Cloudflare Worker serving an Observable Framework project.
Every page is generated from a run artefact; see `docs/design/benchmark-site.md` for the design and
`artifacts/README.md` for the artefact contract.

```sh
# build (installs Framework on the first run)
direnv exec . bash site/build.sh

# preview
cd site/frontend && npm run dev

# deploy
cd site && wrangler deploy --env dev
cd site && wrangler deploy --env production
```

Publishing a run is a `rig-eval` flag:

```sh
cargo run --manifest-path integrations/rig-eval/Cargo.toml -- \
  --out .tmp/rig-eval-ladder --publish site/artifacts ...
```

The publish step refuses a dirty checkout, a contaminated run, or a manifest whose
`schema_version` the site cannot read. `claim.md` is the only file in a run directory a person
writes.
