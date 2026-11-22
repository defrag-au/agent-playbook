# The verify image: the toolchain the repository's rules name, including the components the
# official rust image's minimal profile omits. The agent image is the harness; this is the check.
FROM rust:1.99-bookworm
RUN rustup component add rustfmt clippy
