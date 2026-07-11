# Changelog

All notable changes to this project are documented here. The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project adheres
to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

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

[Unreleased]: https://gitlab.com/zornfeuer/grafana-plugin-sdk-rs/-/compare/v0.1.0...main
[0.1.0]: https://gitlab.com/zornfeuer/grafana-plugin-sdk-rs/-/tags/v0.1.0
