use axum::{
    body::Body,
    http::{Request, StatusCode},
    Router,
};
use opentelemetry::global;
use opentelemetry_sdk::{propagation::TraceContextPropagator, trace::SdkTracerProvider};
use serde_json::Value;
use sqlx::postgres::PgPoolOptions;
use tower::ServiceExt;
use tracing::{instrument::WithSubscriber, Dispatch};

use super::ecs_tests::{ecs_dispatch, traced_dispatch, FakeLogWriter};
use crate::{
    config::{AppConfig, CorsConfig, McpConfig},
    repositories::database::DatabaseRepository,
    routes::create_router,
    state::AppState,
};

fn api_log_config(otel_enabled: bool) -> AppConfig {
    AppConfig {
        database_url: "postgres://unused:unused@localhost/unused".into(),
        host: "127.0.0.1".parse().unwrap(),
        port: 8080,
        cors: CorsConfig::Permissive,
        body_limit_bytes: 8,
        otel_enabled,
        mcp: McpConfig {
            enabled: cfg!(feature = "mcp"),
            path: "/mcp".into(),
            cors: CorsConfig::Permissive,
        },
        accounts_webhook_secret: None,
        outbound_event_webhook: None,
        public_api_base_url: None,
    }
}

fn api_log_router(otel_enabled: bool) -> Router {
    let config = api_log_config(otel_enabled);
    let pool = PgPoolOptions::new()
        .connect_lazy(&config.database_url)
        .unwrap();
    create_router(AppState::new(DatabaseRepository::new(pool)), &config)
}

fn request_events(writer: &FakeLogWriter) -> Vec<Value> {
    writer
        .events()
        .into_iter()
        .filter(|event| event["event"]["action"] == "http_request")
        .collect()
}

fn access_request(path: &str, oversized: bool) -> Request<Body> {
    let body = if oversized {
        Body::from("oversized body")
    } else {
        Body::empty()
    };
    Request::builder()
        .uri(format!("{path}?token=hidden"))
        .header("authorization", "Bearer hidden")
        .header("cookie", "session=hidden")
        .body(body)
        .unwrap()
}

async fn logged_status(router: Router, request: Request<Body>, dispatch: Dispatch) -> StatusCode {
    router
        .oneshot(request)
        .with_subscriber(dispatch)
        .await
        .unwrap()
        .status()
}

fn assert_http_status(event: &Value, expected: u16) {
    assert_eq!(event["http"]["response"]["status_code"], expected);
    assert_eq!(event["http"]["request"]["method"], "GET");
    assert!(event["event"]["duration"].is_u64());
    assert!(!event.to_string().contains("hidden"));
}

#[tokio::test]
async fn api_access_logs_cover_health_missing_routes_and_body_limit_without_otel() {
    let writer = FakeLogWriter::default();
    let router = api_log_router(false);
    for (path, expected) in [("/health", 200), ("/missing", 404), ("/echo", 413)] {
        let request = access_request(path, expected == 413);
        let dispatch = ecs_dispatch(writer.clone());
        assert_eq!(
            logged_status(router.clone(), request, dispatch)
                .await
                .as_u16(),
            expected
        );
    }
    let events = request_events(&writer);
    assert_eq!(events.len(), 3);
    for (event, status) in events.iter().zip([200, 404, 413]) {
        assert_http_status(event, status);
    }
}

fn traced_request() -> Request<Body> {
    let mut request = access_request("/echo", false);
    request.headers_mut().insert(
        "traceparent",
        "00-0123456789abcdef0123456789abcdef-0123456789abcdef-01"
            .parse()
            .unwrap(),
    );
    request
}

#[tokio::test]
async fn api_access_logs_share_incoming_trace_context_and_omit_query_strings() {
    global::set_text_map_propagator(TraceContextPropagator::new());
    let writer = FakeLogWriter::default();
    let provider = SdkTracerProvider::builder().build();
    let dispatch = traced_dispatch(writer.clone(), &provider);
    let status = logged_status(api_log_router(true), traced_request(), dispatch).await;
    assert_eq!(status, StatusCode::OK);
    let events = request_events(&writer);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0]["trace"]["id"], "0123456789abcdef0123456789abcdef");
    assert_eq!(events[0]["url"]["path"], "/echo");
    assert_eq!(events[0]["span"]["id"].as_str().unwrap().len(), 16);
    assert!(!events[0].to_string().contains("hidden"));
}

#[tokio::test]
async fn api_access_logs_include_cors_preflight_responses() {
    let writer = FakeLogWriter::default();
    let request = Request::builder()
        .method("OPTIONS")
        .uri("/echo")
        .header("origin", "https://example.test")
        .header("access-control-request-method", "POST")
        .body(Body::empty())
        .unwrap();
    let dispatch = ecs_dispatch(writer.clone());
    let status = logged_status(api_log_router(false), request, dispatch).await;
    assert_eq!(status, StatusCode::OK);
    let events = request_events(&writer);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0]["http"]["request"]["method"], "OPTIONS");
}

#[cfg(feature = "mcp")]
#[tokio::test]
async fn api_access_logs_include_mcp_responses() {
    let writer = FakeLogWriter::default();
    let request = Request::builder()
        .method("POST")
        .uri("/mcp")
        .header("content-type", "application/json")
        .body(Body::from("{}"))
        .unwrap();
    let dispatch = ecs_dispatch(writer.clone());
    let status = logged_status(api_log_router(false), request, dispatch).await;
    let events = request_events(&writer);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0]["url"]["path"], "/mcp");
    assert_eq!(
        events[0]["http"]["response"]["status_code"],
        status.as_u16()
    );
}
