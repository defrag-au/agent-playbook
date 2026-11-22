# The rig-eval trial image.
#
# Built by `integrations/rig-eval/container/build.sh`, which stages the context. The layout is
# load-bearing:
#
#   /workspace  the agent-playbook checkout: the toolkit, the plugin, and the baked rig homes
#   /rig        the rig fork: the plugin's dev-dependency and `rig`'s compiled-in checkout
#   /repo       where a trial's snapshot is mounted at run time (empty in the image)
#
# The build script removes the scenario corpus from the context, so the image never holds the gold
# and the containment needs no rule to hide it. Each home is built here, with its target directory
# warm: `rig` runs `cargo build` before every start, and a cold Bevy build per trial would dominate
# the measurement.
#
# The image is the devshell. `direnv exec` and `nix develop -c` are small shims that run the
# command, because the rules name those commands and the toolchain is already on `PATH`.

ARG RUST_IMAGE=rust:1.99-bookworm

FROM node:22-bookworm-slim AS node

FROM ${RUST_IMAGE}

ARG RUST_VERSION=1.99.1
ARG BUN_VERSION=1.2.19
ARG PI_VERSION=1.5.0

ENV RUSTUP_TOOLCHAIN=${RUST_VERSION} \
    CARGO_TERM_COLOR=never \
    CARGO_PROFILE_DEV_DEBUG=0 \
    CARGO_PROFILE_TEST_DEBUG=0 \
    CARGO_INCREMENTAL=0 \
    BUN_INSTALL=/opt/bun \
    BUN_INSTALL_CACHE_DIR=/opt/bun/cache \
    PATH=/opt/playbook/bin:/usr/local/cargo/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin

RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        build-essential \
        ca-certificates \
        git \
        libasound2-dev \
        libssl-dev \
        libudev-dev \
        pkg-config \
    && rm -rf /var/lib/apt/lists/*

# Node runs Pi; bun runs the opencode arm at its pinned package version.
COPY --from=node /usr/local/bin/node /usr/local/bin/node
COPY --from=node /usr/local/lib/node_modules /usr/local/lib/node_modules
RUN ln -s /usr/local/lib/node_modules/npm/bin/npm-cli.js /usr/local/bin/npm \
    && ln -s /usr/local/lib/node_modules/npm/bin/npx-cli.js /usr/local/bin/npx \
    && npm install -g "bun@${BUN_VERSION}" \
    && npm install -g --ignore-scripts "@earendil-works/pi-coding-agent@${PI_VERSION}"

RUN mkdir -p /opt/playbook/bin /opt/opencode /opt/bun /repo

COPY agent-playbook/ /workspace/
COPY rig/ /rig/
COPY homes/ /workspace/.tmp/
COPY shims/ /usr/local/bin/
COPY playbook-eval.json /etc/playbook-eval.json

# Defence in depth: the build script excludes the corpus, and the image refuses to contain it.
RUN chmod 0755 /usr/local/bin/direnv /usr/local/bin/nix \
    && rm -rf /workspace/integrations/rig-eval/scenarios \
    && test ! -d /workspace/integrations/rig-eval/scenarios

# The toolkit the `cli` and `documented` arms have. `/opt` keeps it out of the target directory an
# agent may rebuild.
RUN cd /workspace \
    && cargo build --release \
        -p agent-playbook \
        -p at-cardano \
        -p at-cost \
        -p at-crate \
        -p at-describe \
        -p at-peek \
        -p at-reach \
        -p at-recall \
    && cp target/release/at-peek target/release/at-recall target/release/at-crate \
        target/release/at-describe target/release/at-reach target/release/at-cost \
        target/release/at-cardano target/release/playbook /opt/playbook/bin/

RUN cd /rig \
    && cargo build --release -p rig-cli \
    && install -m 0755 target/release/rig /usr/local/bin/rig \
    && rm -rf /rig/target

# The plugin's own workspace, which `devshell-tests` builds in the snapshot. Its target is linked
# into the snapshot at this path, so the fingerprints match.
RUN cd /workspace/integrations/rig && cargo test --no-run

# One warm agent per baked home. The loop is the label list the build script staged.
RUN for home in /workspace/.tmp/rig-home*; do RIG_HOME="$home" /usr/local/bin/rig build; done

# Warm bunx's package cache; opencode is pinned by the driver, and a trial should not download it.
RUN bunx opencode-ai@1.19.5 --version

RUN useradd --create-home --uid 1000 eval \
    && chown -R eval:eval /workspace /rig /opt /repo /home/eval

WORKDIR /repo
USER eval

CMD ["/bin/bash"]
