/*! A lean, protocol-first SDK for building Grafana backend plugins in Rust.

Backend plugins communicate with Grafana via gRPC (the `pluginv2` protocol),
run as a [go-plugin] subprocess. This crate implements that protocol.

The default build is intentionally small and dependency-light — it implements
only what a resource/health plugin needs: custom HTTP-style requests
(`CallResource`) and health checks (`CheckHealth`). Dataframe queries and
streaming are opt-in behind cargo features (see below).

The crate is organised into:

- [`backend`] contains the traits a plugin implements (e.g.
  [`ResourceService`][backend::ResourceService] and
  [`DiagnosticsService`][backend::DiagnosticsService]) and the
  [`Plugin`][backend::Plugin] type that serves them.
- [`live`] contains channel/path types used by [Grafana Live].
- `data` (feature `data`) contains the fundamental dataframe structures
  (`Frame`, `Field`) and their metadata.

The [`prelude`] re-exports the [`GrafanaPlugin`] derive macro and, with the
`data` feature, the traits for building frames and fields.

The low-level generated structs are exposed in the [`pluginv2`] module as an
escape hatch, if required.

See the docs on [backend plugins on grafana.com] for an introduction to backend
Grafana plugins.

# Logging

Emit logs with the [`tracing`] macros; install [`backend::layer`], call
[`backend::init_hclog_subscriber`] before bootstrap work, or enable
[`Plugin::init_subscriber`][backend::Plugin::init_subscriber] so they are written
in the [hclog] format Grafana understands. This is the idiomatic replacement for
the Go SDK's `backend/log` logger.

# Build information

[`build_info!`] captures the plugin's [`BuildInfo`][buildinfo::BuildInfo] (id and
version) from its Cargo metadata.

# Feature flags

- `automtls` — go-plugin automatic mTLS, required to connect to a Grafana instance
  that serves backend plugins with AutoMTLS (the default). Opt-in.
- `data` — dataframe support: the `data` module (`Frame`/`Field`),
  `DataService`/`QueryData`, and Arrow IPC serialization. Pulls in Apache Arrow.
- `stream` — Grafana Live `StreamService` (implies `data`).
- `httpadapter` — a [`ResourceService`][backend::ResourceService] that serves
  `CallResource` requests by running them through an `axum::Router`.
- `reqwest` — adds the `httpclient` module and an
  [`IntoHttpResponse`][backend::IntoHttpResponse] implementation for `reqwest::Response`.
- `prometheus` — encodes a `prometheus::Registry` directly into
  [`CollectMetricsResponse`][backend::CollectMetricsResponse].
- `gen-proto` — regenerate the protobuf bindings at build time (requires `protoc`).

[hclog]: https://github.com/hashicorp/go-hclog

[Backend plugins on grafana.com]: https://grafana.com/docs/grafana/latest/developers/plugins/backend/
[Grafana Live]: https://grafana.com/docs/grafana/latest/live/
[go-plugin]: https://github.com/hashicorp/go-plugin
*/
#![cfg_attr(docsrs, feature(doc_notable_trait))]
#![deny(missing_docs)]

/// Re-export of the arrow crate depended on by this crate.
///
/// We recommend that you use this re-export rather than depending on arrow
/// directly to ensure compatibility; otherwise, rustc/cargo may emit mysterious
/// error messages.
///
/// Only available when the `data` feature is enabled.
#[cfg(feature = "data")]
pub use arrow;

#[doc(hidden)]
pub use serde_json;

#[cfg(feature = "reqwest")]
extern crate reqwest_lib as reqwest;

#[allow(
    missing_docs,
    clippy::all,
    clippy::nursery,
    clippy::pedantic,
    rustdoc::all
)]
pub mod pluginv2 {
    //! The low-level structs generated from protocol definitions.
    include!("pluginv2/pluginv2.rs");
}

pub mod backend;
pub mod buildinfo;
#[cfg(feature = "data")]
pub mod data;
#[cfg(feature = "httpadapter")]
pub mod httpadapter;
#[cfg(feature = "reqwest")]
pub mod httpclient;
pub mod live;

/// Contains useful helper traits, in particular the [`GrafanaPlugin`] derive macro
/// and (with the `data` feature) the traits for constructing `Field`s and `Frame`s.
pub mod prelude {
    pub use grafana_plugin_sdk_macros::GrafanaPlugin;

    #[cfg(feature = "data")]
    pub use crate::data::{ArrayIntoField, FromFields, IntoField, IntoFrame, IntoOptField};
}

#[doc(inline)]
pub use grafana_plugin_sdk_macros::*;

/// WARNING: Do not use this method outside of the SDK.
#[doc(hidden)]
pub fn async_main<R>(fut: impl std::future::Future<Output = R> + Send) -> R {
    tokio::runtime::Builder::new_multi_thread()
        .thread_name("grafana-plugin-worker-thread")
        .enable_all()
        .build()
        .expect("create tokio runtime")
        .block_on(fut)
}
