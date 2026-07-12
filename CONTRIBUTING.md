# Contributing

Thanks for your interest in `grafana-plugin-sdk-rs`!

The canonical repository is on
[GitLab](https://gitlab.com/zornfeuer/grafana-plugin-sdk-rs); the GitHub mirror is
read-only. Please open issues and merge requests on GitLab.

## Scope

This is a **generic** SDK — a Rust analogue of `grafana-plugin-sdk-go`. It should
contain only reusable protocol/plumbing primitives. Application- or plugin-specific
logic belongs in the plugin, not here.

## Before opening a merge request

Run the same checks CI runs:

```sh
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings
cargo clippy --all-targets --features data,stream,reqwest,httpadapter,automtls -- -D warnings
cargo test -p grafana-plugin-sdk-rs
cargo test --workspace --features data,stream,reqwest,httpadapter,automtls
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --features data,stream,reqwest,httpadapter,automtls
```

The `automtls` feature builds `aws-lc-rs`, which requires a C compiler and `cmake`.

Notes:

- Keep the **default build lean** — heavy or niche dependencies (Apache Arrow,
  TLS stacks, `axum`, `reqwest`) must sit behind opt-in features.
- New public items need rustdoc (`#![deny(missing_docs)]` is enforced) and,
  where useful, a doctest.
- Commit messages follow [Conventional Commits](https://www.conventionalcommits.org/).

## Protocol changes

The `pluginv2` bindings are generated from `crates/grafana-plugin-sdk-rs/vendor/proto/backend.proto`
and checked in. Regenerate them with the `gen-proto` feature (which supplies `protoc`):

```sh
cargo build -p grafana-plugin-sdk-rs --features gen-proto
```

## License

By contributing you agree that your contributions are dual-licensed under the
MIT and Apache-2.0 licenses, matching the project.
