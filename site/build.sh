#!/bin/bash
set -e

# agent-playbook site — Observable Framework → dist/, assets-only worker.
#
# Mirrors cnft.dev-workers projects/notebooks/build.sh; kept local for the same
# reason, so a change to this site never touches a script another worker uses.

cd "$(dirname "$0")/frontend"

if [ ! -d node_modules ]; then
    echo "📦 installing Observable Framework..."
    npm ci --no-audit --no-fund
fi

# The loaders read `../artifacts/`. Observable caches a loader's output and
# invalidates on the loader script, never on what the loader reads, so
# regenerating a run would leave a stale page and the build would still report
# success. The loaders are a file read and a JSON parse: always re-run them.
rm -rf src/.observablehq/cache/data

echo "📓 turning run artefacts into pages..."
node scripts/sync-runs.mjs

echo "📊 building the site..."
npm run build

cd ..
rm -rf dist
cp -r frontend/build dist
echo "✅ site built to dist/"
