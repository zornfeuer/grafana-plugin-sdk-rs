//! A minimal Grafana **app** backend plugin, end-to-end.
//!
//! It demonstrates the protocol seam the OnCall backend relies on:
//!
//! - a health check ([`DiagnosticsService`]),
//! - resource routing through an [`axum::Router`] via the [`httpadapter`], including
//!   reading the calling user straight from the [`PluginContext`] (no header dance),
//! - automatic mTLS (the `automtls` feature) so it connects to a stock Grafana.
//!
//! Build it, drop the binary into an app plugin directory alongside a `plugin.json`,
//! and load it in Grafana. See the module docs at the bottom of this file for the
//! exact bundling steps.
//!
//! Run with: `cargo run --example app_plugin --features httpadapter,automtls`
//! (Grafana launches the binary itself; running it directly just prints the
//! go-plugin handshake line and serves gRPC.)

use axum::{extract::Extension, http::header, response::IntoResponse, routing::get, Router};
use grafana_plugin_sdk::{
    backend::{
        self, CheckHealthRequest, CheckHealthResponse, CollectMetricsRequest,
        CollectMetricsResponse, DiagnosticsService,
    },
    httpadapter::{AppPluginContext, HttpResourceService},
    prelude::*,
};
use serde_json::json;

/// Build a `200 OK` JSON response without pulling in axum's `json` feature.
fn json_response(value: serde_json::Value) -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "application/json")],
        value.to_string(),
    )
}

/// The plugin. App plugins that only route resources can be a unit struct; hold
/// shared state (DB pools, clients, …) here in a real plugin.
#[derive(Clone, Debug, GrafanaPlugin)]
#[grafana_plugin(plugin_type = "app")]
struct App;

#[backend::async_trait]
impl DiagnosticsService for App {
    type CheckHealthError = std::convert::Infallible;

    async fn check_health(
        &self,
        _request: CheckHealthRequest<Self>,
    ) -> Result<CheckHealthResponse, Self::CheckHealthError> {
        Ok(CheckHealthResponse::ok("plugin is healthy".to_string()))
    }

    type CollectMetricsError = std::convert::Infallible;

    async fn collect_metrics(
        &self,
        _request: CollectMetricsRequest<Self>,
    ) -> Result<CollectMetricsResponse, Self::CollectMetricsError> {
        Ok(CollectMetricsResponse::new(None))
    }
}

/// `GET /ping` — a trivial JSON response.
async fn ping() -> impl IntoResponse {
    json_response(json!({ "status": "ok" }))
}

/// `GET /whoami` — returns the Grafana user that made the request, taken directly
/// from the [`PluginContext`] the adapter injected into the request extensions.
async fn whoami(Extension(ctx): Extension<AppPluginContext>) -> impl IntoResponse {
    let body = match ctx.user {
        Some(user) => json!({
            "login": user.login,
            "name": user.name,
            "email": user.email,
            "role": format!("{:?}", user.role),
            "orgId": ctx.org_id,
        }),
        None => json!({ "user": null, "orgId": ctx.org_id }),
    };
    json_response(body)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let router = Router::new()
        .route("/ping", get(ping))
        .route("/whoami", get(whoami));

    // MUST come before anything else writes to stdout: prints the go-plugin
    // handshake line (and, under automatic mTLS, the server certificate).
    let listener = backend::initialize().await?;
    // Install logging before database connections, migrations, and other
    // bootstrap work so those events are visible in Grafana too.
    backend::init_hclog_subscriber()?;

    let shutdown = backend::ShutdownToken::new();
    // App plugins may expose a separate TCP listener for callbacks that cannot
    // enter through Grafana's authenticated CallResource proxy. Set
    // PLUGIN_HTTP_ADDR=127.0.0.1:3001 to enable this example listener.
    if let Ok(address) = std::env::var("PLUGIN_HTTP_ADDR") {
        let sidecar_listener = tokio::net::TcpListener::bind(&address).await?;
        let sidecar_shutdown = shutdown.clone();
        let sidecar_router = Router::new().route("/webhook", get(ping));
        tokio::spawn(async move {
            if let Err(error) = axum::serve(sidecar_listener, sidecar_router)
                .with_graceful_shutdown(async move { sidecar_shutdown.cancelled().await })
                .await
            {
                tracing::error!(%error, "sidecar HTTP listener stopped");
            }
        });
    }

    let worker_shutdown = shutdown.clone();
    tokio::spawn(async move {
        worker_shutdown.cancelled().await;
        // Finish in-flight work and close pools here.
    });

    backend::Plugin::new()
        .diagnostics_service(App)
        .resource_service(HttpResourceService::new(router))
        .shutdown_token(shutdown)
        // Safe after the explicit early initialization above.
        .init_subscriber(true)
        .start(listener)
        .await?;
    Ok(())
}

/*
Bundling this as a Grafana app plugin
=====================================

1. Build the binary with the features Grafana needs:

       cargo build --release --example app_plugin --features httpadapter,automtls

   The binary will be at `target/release/examples/app_plugin`.

2. Create a plugin directory, e.g. `rust-sdk-poc-app/`, containing:

   - the binary, renamed to match the `executable` field below and suffixed with
     the target, e.g. `gpx_poc_app_linux_amd64` (Grafana appends
     `_<os>_<arch>` and, on Windows, `.exe`);
   - a `plugin.json`:

         {
           "type": "app",
           "name": "Rust SDK PoC",
           "id": "rust-sdk-poc-app",
           "backend": true,
           "executable": "gpx_poc_app",
           "info": { "version": "0.3.0", "updated": "2026-07-12" },
           "dependencies": { "grafanaDependency": ">=10.0.0" }
         }

3. Point Grafana at it and allow it to load unsigned:

       [paths]
       plugins = /path/to/plugins        # parent of rust-sdk-poc-app/

       [plugins]
       allow_loading_unsigned_plugins = rust-sdk-poc-app

4. Enable the app (Administration → Plugins → Rust SDK PoC → Enable), then hit its
   resource endpoints (these go over gRPC/CallResource, not the network):

       GET /api/plugins/rust-sdk-poc-app/resources/ping     -> {"status":"ok"}
       GET /api/plugins/rust-sdk-poc-app/resources/whoami    -> the calling user

   The health check surfaces at:

       GET /api/plugins/rust-sdk-poc-app/health

If Grafana runs with automatic mTLS (the default), the `automtls` feature is what
lets the connection succeed. If you see the plugin fail to start with a TLS error,
capture Grafana's server logs and the plugin's stderr — that is the one remaining
interop point (Grafana accepting our self-signed server certificate) to confirm.
*/
