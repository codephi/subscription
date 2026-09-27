use std::{sync::Arc, time::Duration};

use axum::{body::Bytes, extract::State, http::HeaderMap, routing::post, Router};
use opentelemetry::trace::{Span, Tracer, TracerProvider};
use opentelemetry_otlp::{Protocol, WithExportConfig};
use opentelemetry_sdk::trace::SdkTracerProvider;
use tokio::{net::TcpListener, sync::Mutex, task::JoinHandle};

use super::{http_export_timeout, http_exporter_builder};

#[derive(Clone, Debug)]
struct CapturedOtlpRequest {
    content_type: String,
    body: Bytes,
}

struct FakeOtlpCollector {
    endpoint: String,
    requests: Arc<Mutex<Vec<CapturedOtlpRequest>>>,
    server: JoinHandle<()>,
}

impl FakeOtlpCollector {
    async fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}/v1/traces", listener.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let router = Router::new()
            .route("/v1/traces", post(capture_otlp_request))
            .with_state(requests.clone());
        let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        Self {
            endpoint,
            requests,
            server,
        }
    }
}

impl Drop for FakeOtlpCollector {
    fn drop(&mut self) {
        self.server.abort();
    }
}

async fn capture_otlp_request(
    State(requests): State<Arc<Mutex<Vec<CapturedOtlpRequest>>>>,
    headers: HeaderMap,
    body: Bytes,
) -> &'static str {
    requests.lock().await.push(CapturedOtlpRequest {
        content_type: headers["content-type"].to_str().unwrap().to_owned(),
        body,
    });
    ""
}

fn http_provider(endpoint: &str, protocol: Protocol, use_simple: bool) -> SdkTracerProvider {
    let exporter = http_exporter_builder(protocol, use_simple, Duration::from_secs(2))
        .unwrap()
        .with_endpoint(endpoint)
        .build()
        .unwrap();
    let builder = SdkTracerProvider::builder();
    if use_simple {
        return builder.with_simple_exporter(exporter).build();
    }
    builder.with_batch_exporter(exporter).build()
}

async fn assert_http_exports(protocol: Protocol, content_type: &str, use_simple: bool) {
    let collector = FakeOtlpCollector::start().await;
    let provider = http_provider(&collector.endpoint, protocol, use_simple);
    let tracer = provider.tracer("subscription-telemetry-regression");
    for span_name in ["first-batch", "second-batch"] {
        tracer.start(span_name).end();
        provider.force_flush().expect("export must keep working");
    }
    tracer.start("shutdown-batch").end();
    provider
        .shutdown()
        .expect("flush pending spans on shutdown");
    let requests = collector.requests.lock().await;
    assert_eq!(requests.len(), 3);
    for request in requests.iter() {
        assert_eq!(request.content_type, content_type);
        assert!(!request.body.is_empty());
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn batch_http_protobuf_exports_repeatedly_and_flushes_on_shutdown() {
    assert_http_exports(Protocol::HttpBinary, "application/x-protobuf", false).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn batch_http_json_exports_repeatedly_and_flushes_on_shutdown() {
    assert_http_exports(Protocol::HttpJson, "application/json", false).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn simple_http_protobuf_exports_inside_tokio() {
    assert_http_exports(Protocol::HttpBinary, "application/x-protobuf", true).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn simple_http_json_exports_inside_tokio() {
    assert_http_exports(Protocol::HttpJson, "application/json", true).await;
}

#[test]
fn trace_http_timeout_takes_precedence_over_global_timeout() {
    assert_eq!(
        http_export_timeout(Some("250"), Some("1000")),
        Duration::from_millis(250)
    );
}

#[test]
fn missing_or_invalid_trace_http_timeout_uses_global_timeout() {
    for trace_timeout in [None, Some("invalid"), Some("-1")] {
        assert_eq!(
            http_export_timeout(trace_timeout, Some("1000")),
            Duration::from_secs(1)
        );
    }
}

#[test]
fn missing_or_invalid_http_timeouts_use_sdk_default() {
    for timeout in [None, Some("invalid"), Some("-1")] {
        assert_eq!(
            http_export_timeout(timeout, timeout),
            opentelemetry_otlp::OTEL_EXPORTER_OTLP_TIMEOUT_DEFAULT
        );
    }
}
