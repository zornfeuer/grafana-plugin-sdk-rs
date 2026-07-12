# grafana-plugin-sdk-rs

[![crates.io](https://img.shields.io/crates/v/grafana-plugin-sdk-rs.svg)](https://crates.io/crates/grafana-plugin-sdk-rs)
[![docs.rs](https://img.shields.io/docsrs/grafana-plugin-sdk-rs)](https://docs.rs/grafana-plugin-sdk-rs)
[![pipeline](https://gitlab.com/zornfeuer/grafana-plugin-sdk-rs/badges/main/pipeline.svg)](https://gitlab.com/zornfeuer/grafana-plugin-sdk-rs/-/pipelines)
[![license](https://img.shields.io/crates/l/grafana-plugin-sdk-rs.svg)](#license)

A modern Rust SDK for building [Grafana backend plugins][backend-plugins] —
`CheckHealth`, `CallResource`, `QueryData`, `QueryChunkedData`, Grafana Live
streaming, automatic mTLS, admission/conversion, and more.

Grafana talks to backend plugins as a [go-plugin] subprocess over gRPC
(`pluginv2`). This crate implements that protocol — including go-plugin's
automatic mTLS — so you can write the plugin backend entirely in Rust, with no Go
in the stack.

> **Status:** early development (`0.x`). Protocol-first and dependency-lean by
> default — the core build pulls in no Apache Arrow — with an `axum` resource
> adapter, automatic mTLS, instance management, and admission/conversion
> services on top. APIs may change before `1.0`.
>
> **Repositories:** the canonical repository is on
> [GitLab](https://gitlab.com/zornfeuer/grafana-plugin-sdk-rs); the
> [GitHub mirror](https://github.com/zornfeuer/grafana-plugin-sdk-rs) is read-only.

## Design goals

- **Protocol-first & lean by default.** The default build implements only what a
  resource/health plugin needs — `CallResource` and `CheckHealth` — and pulls in
  no Apache Arrow. Dataframes and streaming are opt-in.
- **`http`-native resources.** `CallResource` requests/responses are modelled as
  `http::Request`/`http::Response`, ready to bridge to a `tower`/`axum` router.

## Quickstart

```toml
[dependencies]
grafana-plugin-sdk-rs = { version = "0.1", features = ["httpadapter", "automtls"] }
axum = "0.8"
tokio = { version = "1", features = ["rt-multi-thread", "macros"] }
```

```rust,no_run
use axum::{routing::get, Router};
use grafana_plugin_sdk::{backend, httpadapter::HttpResourceService};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let router = Router::new().route("/ping", get(|| async { "pong" }));

    // Must run before anything else writes to stdout.
    let listener = backend::initialize().await?;
    // Install hclog before plugin-specific bootstrap work so its logs are kept.
    backend::init_hclog_subscriber()?;
    let shutdown = backend::ShutdownToken::new();

    backend::Plugin::new()
        .resource_service(HttpResourceService::new(router))
        .shutdown_token(shutdown)
        .init_subscriber(true)
        .start(listener)
        .await?;
    Ok(())
}
```

A complete, runnable app plugin (health check, resource routing, reading the
calling user from the plugin context) with bundling instructions is in
[`examples/app_plugin.rs`](crates/grafana-plugin-sdk-rs/examples/app_plugin.rs):

```sh
cargo run --example app_plugin --features httpadapter,automtls
```

The crate is published as `grafana-plugin-sdk-rs` and imported as
`grafana_plugin_sdk`.

## Cargo features

| Feature       | Default | Enables |
|---------------|:-------:|---------|
| _(core)_      |    ✓    | `CheckHealth`, `CallResource`, `PluginContext`, `GrafanaConfig`, `instancemgmt`, serve loop |
| `automtls`    |         | go-plugin automatic mTLS (needed for a stock Grafana). Pulls in `rustls`/`aws-lc-rs`. |
| `httpadapter` |         | Serve `CallResource` through an `axum::Router`. |
| `reqwest`     |         | The `httpclient` builder + `IntoHttpResponse` for `reqwest::Response`. |
| `data`        |         | Dataframes (`Frame`/`Field`), `QueryData`/`DataService`, Arrow IPC. Pulls in Apache Arrow. |
| `stream`      |         | Grafana Live `StreamService` (implies `data`). |
| `admission`   |         | Kubernetes-style admission control and resource conversion services (experimental). |
| `gen-proto`   |         | Regenerate protobuf bindings at build time (requires `protoc`). |

> A plugin talking to a real Grafana almost always wants `automtls` +
> `httpadapter`. `data`/`stream` (and their Apache Arrow dependency) stay out of
> the default build.

## Roadmap

Working towards feature parity with the Go SDK, roughly in priority order:

- [x] `CheckHealth`, `CallResource` (+ `axum` adapter), `QueryData`/dataframes, Grafana Live streaming
- [x] Automatic mTLS, instance management, `GrafanaConfig`/feature toggles, `httpclient`, build info
- [x] Admission control & resource conversion services (`admission` feature)
- [x] `Data.QueryChunkedData` (chunked query responses, derived from `query_data`)
- [ ] OpenTelemetry trace-context propagation across the gRPC boundary
- [ ] Fuller `httpclient` middleware; datasource instance-management helpers
- [ ] Hygiene: `cargo-deny`, a CI toolchain matrix, more examples (datasource, streaming)

Contributions welcome — see [CONTRIBUTING.md](CONTRIBUTING.md).

## History

This crate began as a fork of Ben Sully's [`grafana-plugin-sdk-rust`][upstream]
and has since diverged substantially: a re-focused, feature-gated lean core, an
`axum` resource adapter, automatic mTLS, instance management, admission and
conversion services, and a protocol synced against the current `pluginv2`. It's
developed independently from here — see [NOTICE](NOTICE) for attribution of the
original code it's derived from.

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or
[MIT license](LICENSE-MIT) at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in this crate by you, as defined in the Apache-2.0 license, shall be
dual licensed as above, without any additional terms or conditions.

[backend-plugins]: https://grafana.com/docs/grafana/latest/developers/plugins/backend/
[go-plugin]: https://github.com/hashicorp/go-plugin
[upstream]: https://github.com/grafana/grafana-plugin-sdk-rust
