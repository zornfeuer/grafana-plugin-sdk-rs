# Changelog

All notable changes to this project are documented here. The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project adheres
to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.4.0] - 2026-07-13

### Added

- Optional `opentelemetry` feature: incoming gRPC metadata is extracted through
  the global OpenTelemetry propagator and attached as the parent of every plugin
  request span. W3C, Jaeger, and composite formats remain application-selected.
- Unit and subprocess integration coverage for W3C `traceparent`/`tracestate`
  propagation, including invalid-context handling.

## [0.3.0] - 2026-07-12

### Added

- `backend::init_hclog_subscriber()` for idempotent Grafana-compatible logging
  before plugin-specific bootstrap work.
- `backend::ShutdownToken` and `Plugin::shutdown_token()` for coordinated gRPC,
  background-task, SIGINT, and SIGTERM shutdown.
- Optional `prometheus` feature with
  `CollectMetricsResponse::from_prometheus_registry()`.
- A documented sidecar HTTP-listener pattern sharing the plugin shutdown token.
- A subprocess integration harness covering the go-plugin handshake,
  `CheckHealth`, streaming `CallResource`, background workers, and graceful exit.

### Changed

- `gen-proto` now supplies a vendored `protoc`, so standard all-features builds
  do not depend on a system installation.
- Quickstart, app-plugin example, feature documentation, and CI cache keys were
  updated for the current workspace layout and release.

- Dropped two unmaintained dependencies (flagged by `cargo-deny`): `rustls-pemfile`
  (the `automtls` PEM parsing now uses `rustls-pki-types`' `PemObject`) and, in
  tests, `paste` (replaced by the maintained `pastey`).

### Internal

- Added a `cargo-deny` supply-chain audit (advisories/licenses/bans/sources) and a
  CI test matrix over the MSRV (1.88) and the latest stable toolchain.

## [0.2.0] - 2026-07-11

### Added

- Synced the vendored `pluginv2` protobuf to the authoritative upstream version and
  regenerated the bindings. This surfaces fields the older snapshot was missing:
  `PluginContext.namespace` (now on the SDK `PluginContext`), a data-frame `format`
  on `QueryDataRequest`/`DataResponse`, and `headers` on the streaming requests.
- `Data.QueryChunkedData`: the checked-in `pluginv2` bindings were regenerated to
  add the chunked-query RPC, and the `DataService` gRPC bridge now serves it by
  streaming each Arrow-encoded frame produced by `query_data` as a separate chunk.
  Plugins get chunked support for free from their existing `query_data`.
- `admission` feature: Kubernetes-style admission control (`AdmissionService`) and
  resource conversion (`ConversionService`) services, with the SDK types and tonic
  bridges, closing the remaining pluginv2 service-parity gap. The `Plugin` builder
  gains `admission_service`/`conversion_service`, and `#[main]` supports
  `services(admission, conversion)`.

## [0.1.0] - 2026-07-11

Initial release: a lean, protocol-first fork of `grafana-plugin-sdk-rust`, focused
on the surface a resource/health app plugin needs.

### Added

- go-plugin handshake and gRPC serving (`tonic`) for the `pluginv2` protocol.
- `Diagnostics.CheckHealth` and `CollectMetrics`.
- `Resource.CallResource` with an `http`-native request/response model.
- `httpadapter` feature: serve `CallResource` through an `axum::Router` via
  `oneshot`, injecting `PluginContext`/`User` into the request extensions.
- `automtls` feature: go-plugin automatic mTLS (`rustls` + `aws-lc-rs`, a pinned
  client-certificate verifier, ECDSA P-521 support, and the server certificate
  advertised in the handshake line). Verified end-to-end against a live Grafana.
- `PluginContext`, `User`, `AppInstanceSettings`, and `GrafanaConfig` with
  `app_url`, `feature_toggles`, and `plugin_app_client_secret` accessors.
- `instancemgmt`: `InstanceManager` / `InstanceProvider` / `Instance` (with
  dispose), plus the `app_instance_key` / `app_needs_update` helpers.
- `buildinfo` module and the `build_info!` macro.
- `reqwest` feature: the `httpclient` builder and an `IntoHttpResponse`
  implementation for `reqwest::Response`.
- hclog-compatible `tracing` layer (`backend::layer`).
- Optional dataframe (`data`) and Grafana Live streaming (`stream`) support,
  behind opt-in features so Apache Arrow stays out of the default build.

### Notes

- Forked from [`grafana-plugin-sdk-rust`](https://github.com/grafana/grafana-plugin-sdk-rust)
  by Ben Sully, dual-licensed MIT/Apache-2.0. See [NOTICE](NOTICE).

[Unreleased]: https://gitlab.com/zornfeuer/grafana-plugin-sdk-rs/-/compare/v0.4.0...main
[0.4.0]: https://gitlab.com/zornfeuer/grafana-plugin-sdk-rs/-/compare/v0.3.0...v0.4.0
[0.3.0]: https://gitlab.com/zornfeuer/grafana-plugin-sdk-rs/-/compare/v0.2.0...v0.3.0
[0.2.0]: https://gitlab.com/zornfeuer/grafana-plugin-sdk-rs/-/compare/v0.1.0...v0.2.0
[0.1.0]: https://gitlab.com/zornfeuer/grafana-plugin-sdk-rs/-/tags/v0.1.0
