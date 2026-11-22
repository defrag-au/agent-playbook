#!/bin/sh
# One task, one container, one handoff. The smallest end-to-end run of
# docs/design/container-flows.md: snapshot a repo at a commit, let rig edit it in the eval image,
# verify the result in a fresh container, and keep the patch, report, effect log and manifest.
#
#   integrations/rig-eval/container/task-run.sh \
#     --task integrations/rig-eval/tasks/chromium-surfaces-duplicate-started.json \
#     --out .tmp/container-flows/chromium-surfaces-duplicate-started
#
# The host's checkout is never written to. The patch is checked against it with `git apply
# --check`, which reads only.

set -eu

ROOT=$(cd "$(dirname "$0")/../../.." && pwd)
TASK=
OUT=
ENV_FILE="$ROOT/.tmp/rig-home/model.env"

while [ $# -gt 0 ]; do
    case "$1" in
        --task) TASK=$2; shift 2 ;;
        --out) OUT=$2; shift 2 ;;
        --env) ENV_FILE=$2; shift 2 ;;
        *) echo "unknown argument: $1" >&2; exit 2 ;;
    esac
done

[ -n "$TASK" ] || { echo "--task is required" >&2; exit 2; }
[ -n "$OUT" ] || { echo "--out is required" >&2; exit 2; }

case "$TASK" in
    /*) ;;
    *) TASK="$PWD/$TASK" ;;
esac
TASK=$(cd "$(dirname "$TASK")" && pwd)/$(basename "$TASK")

command -v jq >/dev/null 2>&1 || { echo "jq is required" >&2; exit 2; }
command -v container >/dev/null 2>&1 || { echo "Apple's container CLI is required" >&2; exit 2; }
[ -f "$ENV_FILE" ] || { echo "no model env at $ENV_FILE (pass --env)" >&2; exit 2; }

name=$(jq -r '.name' "$TASK")
base=$(jq -r '.base' "$TASK")
prompt=$(jq -r '.prompt' "$TASK")
model=$(jq -r '.model' "$TASK")
image=$(jq -r '.image' "$TASK")
verify_image=$(jq -r '.verify_image // .image' "$TASK")
cpus=$(jq -r '.limits.cpus' "$TASK")
memory=$(jq -r '.limits.memory' "$TASK")
wall=$(jq -r '.limits.wall_s' "$TASK")

repo_spec=$(jq -r '.repo' "$TASK")
case "$repo_spec" in
    /*) repo=$repo_spec ;;
    *) repo="$ROOT/$repo_spec" ;;
esac
repo=$(cd "$repo" && pwd)
base_commit=$(git -C "$repo" rev-parse "$base")

case "$OUT" in
    /*) ;;
    *) OUT="$PWD/$OUT" ;;
esac
mkdir -p "$OUT"
OUT=$(cd "$OUT" && pwd)

echo "task       $name"
echo "repo       $repo"
echo "base       $base_commit"
echo "image      $image"
echo "model      $model"
echo "out        $OUT"

digest=$(container image inspect "$image" 2>/dev/null |
    jq -r '.[0].configuration.descriptor.digest // .[0].id')
verify_digest=$(container image inspect "$verify_image" 2>/dev/null |
    jq -r '.[0].configuration.descriptor.digest // .[0].id')

# A synthetic one-commit repo: the agent gets a working tree, `git status` and `git diff`, and no
# history, refs or remotes. The host's `.git` never enters the container.
rm -rf "$OUT/repo" "$OUT/sessions" "$OUT/handoff"
mkdir -p "$OUT/repo" "$OUT/sessions" "$OUT/handoff"
git -C "$repo" archive "$base_commit" | tar -x -C "$OUT/repo"
git -C "$OUT/repo" init -q
git -C "$OUT/repo" add -A
git -C "$OUT/repo" -c user.name=container-flow -c user.email=flow@example.invalid \
    commit -qm "base $base_commit"
snapshot_base=$(git -C "$OUT/repo" rev-parse HEAD)

# The model key is read by the container from a mode-600 file, never from an argument.
umask 077
{
    echo "RIG_HOME=/workspace/.tmp/rig-home"
    grep -v '^[[:space:]]*#' "$ENV_FILE" | grep -v '^[[:space:]]*$' || true
} > "$OUT/env"

container_name="container-flow-$$-$(printf '%s' "$name" | tr -c 'a-zA-Z0-9-' '-' | cut -c1-40)"
echo "container  $container_name"
printf '%s\n' "$container_name" > "$OUT/container.name"
agent_exit=0
if command -v timeout >/dev/null 2>&1; then
    timeout "$wall" container run --rm --name "$container_name" \
        --cpus "$cpus" --memory "$memory" \
        --workdir /repo \
        --volume "$OUT/repo:/repo" \
        --volume "$OUT/sessions:/workspace/.tmp/rig-home/sessions" \
        --env-file "$OUT/env" \
        "$image" /usr/local/bin/rig -p --json "$prompt" --model "$model" \
        > "$OUT/agent.jsonl" 2> "$OUT/agent.err" || agent_exit=$?
else
    container run --rm --name "$container_name" \
        --cpus "$cpus" --memory "$memory" \
        --workdir /repo \
        --volume "$OUT/repo:/repo" \
        --volume "$OUT/sessions:/workspace/.tmp/rig-home/sessions" \
        --env-file "$OUT/env" \
        "$image" /usr/local/bin/rig -p --json "$prompt" --model "$model" \
        > "$OUT/agent.jsonl" 2> "$OUT/agent.err" || agent_exit=$?
fi

if [ "$agent_exit" -eq 124 ]; then
    container kill "$container_name" >/dev/null 2>&1 || true
    container delete "$container_name" >/dev/null 2>&1 || true
fi

# The CLI's own SIGWINCH and progress chatter is not the harness's output.
if [ -f "$OUT/agent.err" ]; then
    grep -Ev 'failed to send signal|^\[[0-9]+/[0-9]+\]' "$OUT/agent.err" \
        > "$OUT/agent.err.filtered" || true
    mv "$OUT/agent.err.filtered" "$OUT/agent.err"
fi

session=$(jq -rs 'map(select(.type == "exit")) | last | .session // empty' \
    "$OUT/agent.jsonl" 2>/dev/null || true)
if [ -z "$session" ]; then
    session=$(jq -rs 'map(select(.type == "ready")) | last | .session // empty' \
        "$OUT/agent.jsonl" 2>/dev/null || true)
fi
answer=$(jq -rs 'map(select(.type == "exit")) | last | .answer // empty' \
    "$OUT/agent.jsonl" 2>/dev/null || true)
if [ -z "$answer" ]; then
    answer=$(jq -rs 'map(select(.type == "turn_end")) | last | .answer // empty' \
        "$OUT/agent.jsonl" 2>/dev/null || true)
fi

effects=
if [ -n "$session" ] && [ -f "$OUT/sessions/$session/files/effects.jsonl" ]; then
    cp "$OUT/sessions/$session/files/effects.jsonl" "$OUT/handoff/effects.jsonl"
    effects=effects.jsonl
fi

jq -r '.verify[]' "$TASK" > "$OUT/verify-commands"
: > "$OUT/verify.tsv"
index=0
while IFS= read -r command; do
    index=$((index + 1))
    if container run --rm --user 1000:1000 --workdir /repo --volume "$OUT/repo:/repo" \
        -e CARGO_HOME=/tmp/cargo -e HOME=/tmp \
        "$verify_image" /bin/sh -c "$command" > "$OUT/handoff/verify-$index.log" 2>&1; then
        status=ok
    else
        status=failed
    fi
    grep -Ev 'failed to send signal|^\[[0-9]+/[0-9]+\]' "$OUT/handoff/verify-$index.log" \
        > "$OUT/handoff/verify-$index.log.filtered" || true
    mv "$OUT/handoff/verify-$index.log.filtered" "$OUT/handoff/verify-$index.log"
    printf '%s\t%s\t%s\n' "$index" "$status" "$command" >> "$OUT/verify.tsv"
done < "$OUT/verify-commands"

if grep -q 'failed' "$OUT/verify.tsv"; then
    verify_passed=false
else
    verify_passed=true
fi
verify_json=$(jq -R -s 'split("\n") | map(select(length > 0) | split("\t") |
    {index: (.[0] | tonumber), status: .[1], command: .[2]})' "$OUT/verify.tsv")

# The patch is the base against the working tree, so a commit the agent makes does not hide it.
git -C "$OUT/repo" add -N . >/dev/null 2>&1 || true
git -C "$OUT/repo" diff "$snapshot_base" > "$OUT/handoff/result.patch"
patch_bytes=$(wc -c < "$OUT/handoff/result.patch" | tr -d ' ')

applies=false
if [ -s "$OUT/handoff/result.patch" ] &&
    git -C "$repo" apply --check "$OUT/handoff/result.patch" >/dev/null 2>&1; then
    applies=true
fi

{
    printf '# %s\n\n' "$name"
    printf -- '- base: `%s`\n' "$base_commit"
    printf -- '- snapshot: `%s`\n' "$snapshot_base"
    printf -- '- image: `%s@%s`\n' "$image" "$digest"
    printf -- '- verify image: `%s@%s`\n' "$verify_image" "$verify_digest"
    printf -- '- model: `%s`\n' "$model"
    printf -- '- agent exit: %s\n' "$agent_exit"
    printf -- '- verify: %s\n\n' "$verify_passed"
    printf '## Answer\n\n%s\n' "${answer:-(no answer in the stream)}"
} > "$OUT/handoff/report.md"

jq -n \
    --arg task "$name" \
    --arg repo "$repo" \
    --arg base "$base_commit" \
    --arg snapshot_base "$snapshot_base" \
    --arg image "$image" \
    --arg digest "$digest" \
    --arg verify_image "$verify_image" \
    --arg verify_digest "$verify_digest" \
    --arg model "$model" \
    --arg session "$session" \
    --arg agent_exit "$agent_exit" \
    --arg verify_passed "$verify_passed" \
    --arg patch_bytes "$patch_bytes" \
    --arg effects "$effects" \
    --argjson verify "$verify_json" \
    --argjson applies "$applies" \
    '{
        task: $task,
        repo: $repo,
        base: $base,
        snapshot_base: $snapshot_base,
        image: { reference: $image, digest: $digest },
        verify_image: { reference: $verify_image, digest: $verify_digest },
        model: $model,
        session: (if $session == "" then null else $session end),
        agent_exit: ($agent_exit | tonumber),
        verify: $verify,
        verify_passed: ($verify_passed == "true"),
        patch: { path: "result.patch", bytes: ($patch_bytes | tonumber), applies_on_host: $applies },
        effects: (if $effects == "" then null else "effects.jsonl" end)
    }' > "$OUT/handoff/manifest.json"

echo
echo "handoff    $OUT/handoff"
echo "patch      $patch_bytes bytes (applies on host: $applies)"
echo "verify     $verify_passed"
echo "session    ${session:-none}"
echo "effects    ${effects:-none}"
echo "status     $([ "$verify_passed" = true ] && echo ok || echo failed)"
[ "$verify_passed" = true ]
