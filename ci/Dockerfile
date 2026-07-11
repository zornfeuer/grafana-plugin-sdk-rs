# CI image for grafana-plugin-sdk-rs.
#
# Bakes in everything the pipeline needs so no job has to `apt-get install` or
# `rustup component add` at run time:
#   - the pinned Rust toolchain + rustfmt/clippy,
#   - the C toolchain (cmake, clang) required to build `aws-lc-rs` (automtls),
#   - sccache, used as RUSTC_WRAPPER to cache compilation across jobs/pipelines.
#
# Rebuilt by CI only when this file changes; otherwise reused via the `:latest`
# tag in the project container registry.
FROM rust:1.88-bookworm

RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        cmake \
        clang \
        libclang-dev \
        pkg-config \
    && rm -rf /var/lib/apt/lists/*

RUN rustup component add rustfmt clippy

ARG SCCACHE_VERSION=v0.8.2
RUN curl -fsSL "https://github.com/mozilla/sccache/releases/download/${SCCACHE_VERSION}/sccache-${SCCACHE_VERSION}-x86_64-unknown-linux-musl.tar.gz" \
        | tar -xz --strip-components=1 -C /usr/local/bin --wildcards '*/sccache' \
    && chmod +x /usr/local/bin/sccache

# Fail fast if the toolchain is somehow broken.
RUN rustc --version && cargo --version && sccache --version && cmake --version | head -1
