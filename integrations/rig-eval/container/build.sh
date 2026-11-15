#!/bin/sh
# Build the rig-eval trial image.
#
#   integrations/rig-eval/container/build.sh
#   integrations/rig-eval/container/build.sh --image playbook-eval/rig-eval:local --homes "gated baseline"
#
# The context is staged under `.tmp/rig-eval/container-context`: the repository without its corpus,
# the rig fork without its history, one `settings.toml` per requested home, and the metadata the
# runner compares an arm against. Building from `.tmp` keeps `git status` clean and makes it obvious
# what the image is made of.
#
# The image digest is what a run records. `rig-eval` reads it from `container image inspect`; this
# script only prints the tag it built, so a caller can name a run by the same tag.

set -eu

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
root=$(CDPATH= cd -- "$script_dir/../../.." && pwd)
fork=$(CDPATH= cd -- "$root/../rig" 2>/dev/null && pwd) || {
	echo "no rig checkout beside the repository (expected $root/../rig)" >&2
	exit 1
}
context="$root/.tmp/rig-eval/container-context"

image="playbook-eval/rig-eval:local"
homes="gated baseline"

usage() {
	cat <<'EOF'
usage: build.sh [--image TAG] [--homes "LABEL ..."] [-h]

  --image TAG     image reference to build (default: playbook-eval/rig-eval:local)
  --homes LABELS  rig homes to bake, space-separated (default: "gated baseline")
                  Known labels: gated, baseline, advertised, slim, permissive, terse, lean, vanilla

The image can only run an arm whose home it baked. A run with a label the image lacks refuses at
startup rather than measuring a different harness.
EOF
}

while [ $# -gt 0 ]; do
	case "$1" in
	--image)
		image=$2
		shift 2
		;;
	--homes)
		homes=$2
		shift 2
		;;
	-h | --help)
		usage
		exit 0
		;;
	*)
		echo "build.sh: unknown argument \`$1\`" >&2
		usage >&2
		exit 2
		;;
	esac
done

settings_source() {
	if [ "$1" = "gated" ]; then
		echo "$root/.tmp/rig-home/settings.toml"
	else
		echo "$root/.tmp/rig-home-$1/settings.toml"
	fi
}

# The image mirrors the host's `.tmp/rig-home[-<label>]` depth, so a settings file's
# `../../integrations/rig` reaches the image's checkout without being rewritten.
home_dir_name() {
	if [ "$1" = "gated" ]; then
		echo "rig-home"
	else
		echo "rig-home-$1"
	fi
}

echo "staging $context"
rm -rf "$context"
mkdir -p "$context/homes" "$context/shims"

rsync -a \
	--exclude '.git/' \
	--exclude '.direnv/' \
	--exclude '.tmp/' \
	--exclude 'node_modules/' \
	--exclude 'target/' \
	--exclude 'integrations/rig-eval/scenarios/' \
	"$root/" "$context/agent-playbook/"

rsync -a --exclude '.git/' --exclude 'target/' "$fork/" "$context/rig/"

cp "$script_dir/shims/direnv" "$script_dir/shims/nix" "$context/shims/"
chmod 0755 "$context/shims/direnv" "$context/shims/nix"

settings=""
count=0
for home in $homes; do
	case "$home" in
	*[!a-z0-9-]* | '')
		echo "build.sh: \`$home\` is not a home label" >&2
		exit 2
		;;
	esac
	settings_path=$(settings_source "$home")
	if [ ! -f "$settings_path" ]; then
		echo "build.sh: no settings for \`$home\` at $settings_path" >&2
		exit 1
	fi
	mkdir -p "$context/homes/$(home_dir_name "$home")"
	cp "$settings_path" "$context/homes/$(home_dir_name "$home")/settings.toml"
	blob=$(git -C "$root" hash-object --no-filters -- "$settings_path")
	settings="$settings\"$home\":\"$blob\","
	count=$((count + 1))
done
settings=${settings%,}
head=$(git -C "$root" rev-parse HEAD)
printf '{"head":"%s","settings":{%s}}\n' "$head" "$settings" >"$context/playbook-eval.json"

# A corpus in the context is a corpus in the image, whatever the runner's mounts say.
if [ -d "$context/agent-playbook/integrations/rig-eval/scenarios" ]; then
	echo "build.sh: the staging context contains the scenario corpus" >&2
	exit 1
fi

echo "building $image ($count home(s))"
container build \
	--tag "$image" \
	--file "$script_dir/Dockerfile" \
	--cpus 8 \
	--memory 16G \
	--progress plain \
	"$context"

echo "built $image"
echo "run with: --container $image"
