/*! Serve Grafana `CallResource` requests using an [`axum::Router`].

Grafana delivers resource calls to a plugin as HTTP-shaped gRPC messages. This
module bridges those messages to an ordinary [`axum::Router`]: the incoming
[`CallResourceRequest`] already carries an [`http::Request`], which is run through
the router via [`tower`]'s [`oneshot`][tower::ServiceExt::oneshot], and the
resulting [`http::Response`] is streamed back to Grafana.

This lets a plugin reuse an existing HTTP router — including all of its handlers,
middleware and routing — without rewriting anything for the plugin protocol. It
is the Rust analogue of the Go SDK's `backend/resource/httpadapter`.

# Accessing the plugin context

Before dispatching to the router, the adapter inserts the request's
[`PluginContext`] (as [`AppPluginContext`]) and, when present, the
[`User`][crate::backend::User] into the [`http::Request`]'s extensions. Handlers
can therefore extract them, e.g. with axum's `Extension`:

```rust,ignore
use axum::{Extension, routing::get, Router};
use grafana_plugin_sdk::httpadapter::AppPluginContext;

async fn whoami(Extension(ctx): Extension<AppPluginContext>) -> String {
    ctx.user.map(|u| u.login).unwrap_or_else(|| "anonymous".to_owned())
}

let router = Router::new().route("/whoami", get(whoami));
```
*/
use std::convert::Infallible;

use axum::body::Body;
use http::{Request, Response};
use http_body_util::BodyExt as _;
use prost::bytes::Bytes;
use serde_json::Value;
use tower::ServiceExt as _;

use crate::backend::{
    async_trait, AppInstanceSettings, AppPlugin, BoxResourceStream, CallResourceRequest,
    GrafanaPlugin, PluginContext, ResourceService,
};

/// The [`PluginContext`] type made available to router handlers via request
/// extensions when serving through [`HttpResourceService`].
///
/// App plugins receive [`AppInstanceSettings`], with instance settings and secure
/// settings deserialized as raw [`serde_json::Value`]s.
pub type AppPluginContext = PluginContext<AppInstanceSettings<Value, Value>, Value, Value>;

/// A [`ResourceService`] that serves `CallResource` requests using an
/// [`axum::Router`].
///
/// Construct one with [`HttpResourceService::new`] and register it on a
/// [`Plugin`][crate::backend::Plugin] with
/// [`resource_service`][crate::backend::Plugin::resource_service].
///
/// # Example
///
/// ```rust,no_run
/// use axum::{routing::get, Router};
/// use grafana_plugin_sdk::{backend, httpadapter::HttpResourceService};
///
/// # async fn run() -> Result<(), Box<dyn std::error::Error>> {
/// let router = Router::new().route("/health", get(|| async { "ok" }));
/// let service = HttpResourceService::new(router);
///
/// let listener = backend::initialize().await?;
/// backend::Plugin::new()
///     .resource_service(service)
///     .start(listener)
///     .await?;
/// # Ok(())
/// # }
/// ```
#[derive(Clone)]
pub struct HttpResourceService {
    router: axum::Router,
}

impl HttpResourceService {
    /// Create a new adapter that dispatches resource requests to `router`.
    #[must_use]
    pub fn new(router: axum::Router) -> Self {
        Self { router }
    }
}

impl GrafanaPlugin for HttpResourceService {
    type PluginType = AppPlugin<Value, Value>;
    type JsonData = Value;
    type SecureJsonData = Value;
}

#[async_trait]
impl ResourceService for HttpResourceService {
    // An `axum::Router` (with `()` state) is infallible as a `tower::Service`.
    type Error = Infallible;
    type InitialResponse = Response<Bytes>;
    type Stream = BoxResourceStream<Self::Error>;

    async fn call_resource(
        &self,
        request: CallResourceRequest<Self>,
    ) -> Result<(Self::InitialResponse, Self::Stream), Self::Error> {
        let user = request.plugin_context.user.clone();
        let plugin_context = request.plugin_context;
        let (mut parts, body) = request.request.into_parts();

        // Make the plugin context (and user) available to handlers via request
        // extensions, mirroring the Go SDK's `WithPluginContext`/`WithUser`.
        parts.extensions.insert(plugin_context);
        if let Some(user) = user {
            parts.extensions.insert(user);
        }
        let request = Request::from_parts(parts, Body::from(body));

        let response = self
            .router
            .clone()
            .oneshot(request)
            .await
            .unwrap_or_else(|never: Infallible| match never {});

        let (parts, body) = response.into_parts();
        // Router responses are buffered in memory, so collecting them will not
        // fail in practice; fall back to an empty body if it somehow does.
        let body = body
            .collect()
            .await
            .map(|b| b.to_bytes())
            .unwrap_or_default();

        Ok((
            Response::from_parts(parts, body),
            Box::pin(futures_util::stream::empty()) as Self::Stream,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pluginv2;
    use axum::{routing::get, Extension, Router};

    async fn whoami(Extension(ctx): Extension<AppPluginContext>) -> String {
        ctx.user
            .map(|u| u.login)
            .unwrap_or_else(|| "anonymous".to_owned())
    }

    #[tokio::test]
    async fn routes_request_and_injects_plugin_context() {
        let router = Router::new().route("/whoami", get(whoami));
        let service = HttpResourceService::new(router);

        let proto = pluginv2::CallResourceRequest {
            plugin_context: Some(pluginv2::PluginContext {
                user: Some(pluginv2::User {
                    login: "alice".to_owned(),
                    name: "Alice".to_owned(),
                    email: "alice@example.com".to_owned(),
                    role: "Admin".to_owned(),
                }),
                ..Default::default()
            }),
            method: "GET".to_owned(),
            url: "whoami".to_owned(),
            ..Default::default()
        };
        let request: CallResourceRequest<HttpResourceService> = proto.try_into().unwrap();

        let (response, _stream) = service.call_resource(request).await.unwrap();
        assert_eq!(response.status(), 200);
        assert_eq!(&response.into_body()[..], b"alice");
    }

    #[tokio::test]
    async fn missing_route_returns_404() {
        let service = HttpResourceService::new(Router::new());
        let proto = pluginv2::CallResourceRequest {
            plugin_context: Some(pluginv2::PluginContext::default()),
            method: "GET".to_owned(),
            url: "nope".to_owned(),
            ..Default::default()
        };
        let request: CallResourceRequest<HttpResourceService> = proto.try_into().unwrap();
        let (response, _stream) = service.call_resource(request).await.unwrap();
        assert_eq!(response.status(), 404);
    }
}
