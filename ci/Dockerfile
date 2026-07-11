# CI image for grafana-plugin-sdk-rs.
#
# Bakes in everything the pipeline needs so no job has to `apt-get install`,
# `rustup component add` or install tools at run time:
#   - the pinned MSRV toolchain (1.88, default) + the latest stable, for the test
#     matrix, both with rustfmt/clippy,
#   - the C toolchain (cmake, clang) required to build `aws-lc-rs` (automtls),
#   - sccache, used as RUSTC_WRAPPER to cache compilation across jobs/pipelines,
#   - cargo-deny, for the supply-chain (advisories/licenses/bans/sources) job.
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

# MSRV (1.88) stays the default toolchain; add stable for the test matrix.
RUN rustup component add rustfmt clippy \
    && rustup toolchain install stable --profile minimal --component rustfmt --component clippy

ARG SCCACHE_VERSION=v0.8.2
RUN curl -fsSL "https://github.com/mozilla/sccache/releases/download/${SCCACHE_VERSION}/sccache-${SCCACHE_VERSION}-x86_64-unknown-linux-musl.tar.gz" \
        | tar -xz --strip-components=1 -C /usr/local/bin --wildcards '*/sccache' \
    && chmod +x /usr/local/bin/sccache

# cargo-deny (built with stable to satisfy its MSRV, independent of ours).
RUN cargo +stable install cargo-deny --locked

# Fail fast if the toolchain is somehow broken.
RUN rustc --version && cargo --version && sccache --version \
    && cargo-deny --version && cmake --version | head -1
