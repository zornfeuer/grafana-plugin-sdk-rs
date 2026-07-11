/*! A configurable HTTP client for calling Grafana or other services.

A lightweight analogue of the Go SDK's `backend/httpclient`: [`new_client`]
builds a [`reqwest::Client`] from a set of [`Options`] (timeouts, TLS, default
headers and basic authentication).

Unlike the Go SDK this deliberately omits the Grafana-internal middleware chain
(datasource metrics, response limits, header forwarding, …); compose your own
[`tower`]/`reqwest` middleware if you need it.

```rust,no_run
use std::time::Duration;
use grafana_plugin_sdk::httpclient::{self, Options, TimeoutOptions};

# fn main() -> Result<(), httpclient::Error> {
let client = httpclient::new_client(&Options {
    timeouts: TimeoutOptions { timeout: Duration::from_secs(10), ..Default::default() },
    headers: vec![("X-Plugin".to_owned(), "oncall".to_owned())],
    ..Default::default()
})?;
# let _ = client;
# Ok(())
# }
```
*/
use std::time::Duration;

use base64::Engine as _;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue, AUTHORIZATION};

/// Timeout and connection-pool options.
#[derive(Clone, Debug)]
pub struct TimeoutOptions {
    /// Total request timeout.
    pub timeout: Duration,
    /// Timeout for establishing a connection.
    pub connect_timeout: Duration,
    /// How long an idle connection is kept in the pool.
    pub idle_timeout: Duration,
    /// Maximum idle connections kept per host (`0` = unlimited).
    pub max_idle_per_host: usize,
}

impl Default for TimeoutOptions {
    // Defaults mirror the Go SDK's `DefaultTimeoutOptions`.
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(30),
            connect_timeout: Duration::from_secs(10),
            idle_timeout: Duration::from_secs(90),
            max_idle_per_host: 100,
        }
    }
}

/// HTTP basic authentication credentials.
#[derive(Clone, Debug)]
pub struct BasicAuth {
    /// The username.
    pub user: String,
    /// The password.
    pub password: String,
}

impl BasicAuth {
    /// Create basic authentication credentials.
    pub fn new(user: impl Into<String>, password: impl Into<String>) -> Self {
        Self {
            user: user.into(),
            password: password.into(),
        }
    }
}

/// TLS options.
#[derive(Clone, Debug, Default)]
pub struct TlsOptions {
    /// Do not verify the server's certificate. **Insecure** — for development only.
    pub insecure_skip_verify: bool,
    /// An additional CA certificate (PEM) to trust.
    pub ca_certificate: Option<String>,
}

/// Options for building an HTTP client.
#[derive(Clone, Debug, Default)]
pub struct Options {
    /// Timeout and connection-pool options.
    pub timeouts: TimeoutOptions,
    /// Basic authentication applied to every request, if set.
    pub basic_auth: Option<BasicAuth>,
    /// TLS options.
    pub tls: TlsOptions,
    /// Default headers added to every request.
    pub headers: Vec<(String, String)>,
}

/// Errors that can occur while building an HTTP client.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// A default header name or value was invalid.
    #[error("invalid default header: {0}")]
    Header(String),
    /// The configured client could not be built (e.g. an invalid CA certificate).
    #[error("could not build HTTP client: {0}")]
    Build(#[from] reqwest::Error),
}

/// Build a [`reqwest::Client`] from the given [`Options`].
pub fn new_client(options: &Options) -> Result<reqwest::Client, Error> {
    let mut headers = HeaderMap::with_capacity(options.headers.len() + 1);
    for (name, value) in &options.headers {
        let name = HeaderName::from_bytes(name.as_bytes())
            .map_err(|e| Error::Header(format!("{name}: {e}")))?;
        let value =
            HeaderValue::from_str(value).map_err(|e| Error::Header(format!("{name}: {e}")))?;
        headers.insert(name, value);
    }
    if let Some(auth) = &options.basic_auth {
        let token = base64::engine::general_purpose::STANDARD
            .encode(format!("{}:{}", auth.user, auth.password));
        let mut value = HeaderValue::from_str(&format!("Basic {token}"))
            .map_err(|e| Error::Header(format!("authorization: {e}")))?;
        value.set_sensitive(true);
        headers.insert(AUTHORIZATION, value);
    }

    let mut builder = reqwest::Client::builder()
        .timeout(options.timeouts.timeout)
        .connect_timeout(options.timeouts.connect_timeout)
        .pool_idle_timeout(options.timeouts.idle_timeout)
        .default_headers(headers);
    if options.timeouts.max_idle_per_host > 0 {
        builder = builder.pool_max_idle_per_host(options.timeouts.max_idle_per_host);
    }
    if options.tls.insecure_skip_verify {
        builder = builder.danger_accept_invalid_certs(true);
    }
    if let Some(ca) = &options.tls.ca_certificate {
        builder = builder.add_root_certificate(reqwest::Certificate::from_pem(ca.as_bytes())?);
    }

    Ok(builder.build()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_client_with_options() {
        let client = new_client(&Options {
            basic_auth: Some(BasicAuth::new("user", "pass")),
            headers: vec![("X-Test".to_owned(), "1".to_owned())],
            tls: TlsOptions {
                insecure_skip_verify: true,
                ..Default::default()
            },
            ..Default::default()
        });
        assert!(client.is_ok());
    }

    #[test]
    fn rejects_invalid_header() {
        let result = new_client(&Options {
            headers: vec![("bad header name".to_owned(), "v".to_owned())],
            ..Default::default()
        });
        assert!(matches!(result, Err(Error::Header(_))));
    }
}
