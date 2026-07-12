use std::{
    convert::Infallible,
    io::{BufRead, BufReader, Read},
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};

use bytes::Bytes;
use grafana_plugin_sdk::{
    backend::{
        self, BoxResourceStream, CallResourceRequest, CheckHealthRequest, CheckHealthResponse,
        CollectMetricsRequest, CollectMetricsResponse, DiagnosticsService, ResourceService,
        ShutdownToken,
    },
    pluginv2,
    prelude::GrafanaPlugin,
};
use http::Response;

#[derive(Clone, Debug, GrafanaPlugin)]
#[grafana_plugin(plugin_type = "app")]
struct HarnessPlugin {
    shutdown: ShutdownToken,
}

#[backend::async_trait]
impl DiagnosticsService for HarnessPlugin {
    type CheckHealthError = Infallible;

    async fn check_health(
        &self,
        _request: CheckHealthRequest<Self>,
    ) -> Result<CheckHealthResponse, Self::CheckHealthError> {
        Ok(CheckHealthResponse::ok(
            "integration harness is healthy".into(),
        ))
    }

    type CollectMetricsError = Infallible;

    async fn collect_metrics(
        &self,
        _request: CollectMetricsRequest<Self>,
    ) -> Result<CollectMetricsResponse, Self::CollectMetricsError> {
        Ok(CollectMetricsResponse::new(None))
    }
}

#[backend::async_trait]
impl ResourceService for HarnessPlugin {
    type Error = Infallible;
    type InitialResponse = Response<Bytes>;
    type Stream = BoxResourceStream<Self::Error>;

    async fn call_resource(
        &self,
        request: CallResourceRequest<Self>,
    ) -> Result<(Self::InitialResponse, Self::Stream), Self::Error> {
        if request.request.uri().path() == "/shutdown" {
            let shutdown = self.shutdown.clone();
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_millis(20)).await;
                shutdown.cancel();
            });
        }
        Ok((
            Response::new(Bytes::from_static(b"resource response")),
            Box::pin(futures::stream::empty()),
        ))
    }
}

fn plugin_context() -> pluginv2::PluginContext {
    pluginv2::PluginContext {
        plugin_id: "integration-test-app".into(),
        plugin_version: env!("CARGO_PKG_VERSION").into(),
        app_instance_settings: Some(pluginv2::AppInstanceSettings {
            json_data: b"{}".to_vec(),
            api_version: "v1".into(),
            ..Default::default()
        }),
        ..Default::default()
    }
}

#[test]
fn plugin_subprocess() {
    if std::env::var_os("GRAFANA_SDK_TEST_SUBPROCESS").is_none() {
        return;
    }

    grafana_plugin_sdk::async_main(async {
        let listener = backend::initialize().await.expect("initialize plugin");
        let shutdown = ShutdownToken::new();
        let worker_stopped = Arc::new(AtomicBool::new(false));
        let worker_flag = Arc::clone(&worker_stopped);
        let worker_shutdown = shutdown.clone();
        let worker = tokio::spawn(async move {
            worker_shutdown.cancelled().await;
            worker_flag.store(true, Ordering::SeqCst);
        });

        let plugin = HarnessPlugin {
            shutdown: shutdown.clone(),
        };
        backend::Plugin::new()
            .diagnostics_service(plugin.clone())
            .resource_service(plugin)
            .shutdown_token(shutdown)
            .start(listener)
            .await
            .expect("serve plugin");

        worker.await.expect("join background worker");
        assert!(worker_stopped.load(Ordering::SeqCst));
    });
}

#[tokio::test]
async fn subprocess_handshake_grpc_and_shutdown() {
    let mut child = Command::new(std::env::current_exe().expect("current test executable"))
        .args([
            "--exact",
            "plugin_subprocess",
            "--nocapture",
            "--test-threads=1",
        ])
        .env("GRAFANA_SDK_TEST_SUBPROCESS", "1")
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("spawn plugin subprocess");

    let stdout = child.stdout.take().expect("subprocess stdout");
    let mut stdout = BufReader::new(stdout);
    let mut handshake = None;
    for line in stdout.by_ref().lines() {
        let line = line.expect("read subprocess output");
        if let Some(start) = line.find("1|2|tcp|") {
            handshake = Some(line[start..].to_owned());
            break;
        }
    }
    let handshake = handshake.expect("go-plugin handshake line");
    let drain_stdout = std::thread::spawn(move || {
        for line in stdout.lines() {
            let _ = line;
        }
    });
    let fields: Vec<_> = handshake.split('|').collect();
    assert_eq!(&fields[..3], &["1", "2", "tcp"]);
    assert_eq!(fields[4], "grpc");
    let endpoint = format!("http://{}", fields[3]);

    let channel = connect_with_retry(endpoint).await;
    let mut diagnostics = pluginv2::diagnostics_client::DiagnosticsClient::new(channel.clone());
    let health = diagnostics
        .check_health(pluginv2::CheckHealthRequest {
            plugin_context: Some(plugin_context()),
            headers: Default::default(),
        })
        .await
        .expect("CheckHealth RPC")
        .into_inner();
    assert_eq!(health.message, "integration harness is healthy");

    let mut resources = pluginv2::resource_client::ResourceClient::new(channel);
    let mut responses = resources
        .call_resource(pluginv2::CallResourceRequest {
            plugin_context: Some(plugin_context()),
            path: "shutdown".into(),
            method: "POST".into(),
            url: "shutdown".into(),
            headers: Default::default(),
            body: Bytes::new(),
        })
        .await
        .expect("CallResource RPC")
        .into_inner();
    let response = responses
        .message()
        .await
        .expect("resource stream")
        .expect("initial resource response");
    assert_eq!(response.code, 200);
    assert_eq!(response.body, Bytes::from_static(b"resource response"));
    drop(responses);

    let status = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Some(status) = child.try_wait().expect("poll plugin subprocess") {
                break status;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap_or_else(|_| {
        child.kill().expect("kill stuck plugin subprocess");
        panic!("plugin subprocess did not stop gracefully");
    });
    assert!(status.success(), "plugin subprocess exited with {status}");
    drain_stdout.join().expect("join stdout drain");
}

async fn connect_with_retry(endpoint: String) -> tonic::transport::Channel {
    let endpoint = tonic::transport::Endpoint::from_shared(endpoint).expect("valid endpoint URI");
    for _ in 0..50 {
        if let Ok(channel) = endpoint.clone().connect().await {
            return channel;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("plugin gRPC server did not become ready");
}
