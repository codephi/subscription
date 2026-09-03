mod support;

use axum::{
    body::Body,
    http::{Method, Request, StatusCode},
};
use serde_json::Value;
use tower::ServiceExt;
use tracing::info_span;

use support::{
    flush_telemetry, response_bytes, response_json, setup_router, setup_router_with_mcp,
};

#[cfg(feature = "mcp")]
const MCP_PROTOCOL_VERSION: &str = "2025-11-25";

#[cfg(feature = "mcp")]
fn mcp_request(method: &str, id: i64, params: Value) -> Request<Body> {
    Request::builder()
        .method(Method::POST)
        .uri("/mcp")
        .header("host", "127.0.0.1")
        .header("content-type", "application/json")
        .header("accept", "application/json, text/event-stream")
        .header("mcp-protocol-version", MCP_PROTOCOL_VERSION)
        .body(Body::from(
            serde_json::json!({
                "jsonrpc": "2.0", "id": id, "method": method, "params": params,
            })
            .to_string(),
        ))
        .expect("build mcp request")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn health_returns_status() {
    let _span = info_span!("integration_test", test = "health").entered();
    let response = setup_router()
        .await
        .oneshot(
            Request::builder()
                .uri("/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("request failed");
    assert_eq!(response.status(), StatusCode::OK);
    let body = response_json(response).await;
    assert_eq!(body["status"], "ok");
    assert_eq!(body["version"], env!("CARGO_PKG_VERSION"));
    flush_telemetry().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn echo_routes_reflect_request() {
    let _span = info_span!("integration_test", test = "echo").entered();
    let router = setup_router().await;
    let methods = [
        Method::GET,
        Method::POST,
        Method::PUT,
        Method::PATCH,
        Method::DELETE,
        Method::OPTIONS,
    ];
    for method in methods {
        let request = Request::builder()
            .method(method.clone())
            .uri("/echo")
            .header("x-test", "value")
            .body(Body::from("payload"))
            .expect("build request");
        let response = router
            .clone()
            .oneshot(request)
            .await
            .expect("request failed");
        assert_eq!(response.status(), StatusCode::OK);
        if method == Method::OPTIONS {
            let body = response_bytes(response).await;
            if !body.is_empty() {
                let payload: Value = serde_json::from_slice(&body).expect("parse echo response");
                assert_eq!(payload["method"], method.as_str());
            }
            continue;
        }
        let payload = response_json(response).await;
        assert_eq!(payload["method"], method.as_str());
        assert_eq!(payload["path"], "/echo");
    }
    let response = router
        .oneshot(
            Request::builder()
                .method(Method::HEAD)
                .uri("/echo")
                .body(Body::empty())
                .expect("build request"),
        )
        .await
        .expect("request failed");
    assert_eq!(response.status(), StatusCode::OK);
    flush_telemetry().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mcp_endpoint_is_absent_when_disabled() {
    let response = setup_router()
        .await
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/mcp")
                .body(Body::from("{}"))
                .expect("build request"),
        )
        .await
        .expect("request failed");
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[cfg(feature = "mcp")]
async fn mcp_initialize_and_tools_work_over_http() {
    let router = setup_router_with_mcp(true).await;
    let initialize = router
        .clone()
        .oneshot(mcp_request(
            "initialize",
            1,
            serde_json::json!({
                "protocolVersion": MCP_PROTOCOL_VERSION,
                "capabilities": {},
                "clientInfo": { "name": "integration-test", "version": "1.0" }
            }),
        ))
        .await
        .expect("initialize request failed");
    assert_eq!(initialize.status(), StatusCode::OK);
    assert_eq!(
        response_json(initialize).await["result"]["protocolVersion"],
        MCP_PROTOCOL_VERSION
    );
    let tools = router
        .oneshot(mcp_request("tools/list", 2, serde_json::json!({})))
        .await
        .expect("tools/list request failed");
    let body = response_json(tools).await;
    let tools = body["result"]["tools"].as_array().expect("tools array");
    assert!(tools.iter().any(|tool| tool["name"] == "health_check"));
    assert!(tools.iter().any(|tool| tool["name"] == "echo_request"));
    flush_telemetry().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[cfg(feature = "mcp")]
async fn mcp_get_is_rejected_in_stateless_mode() {
    let response = setup_router_with_mcp(true)
        .await
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri("/mcp")
                .header("host", "127.0.0.1")
                .header("accept", "text/event-stream")
                .body(Body::empty())
                .expect("build request"),
        )
        .await
        .expect("request failed");
    assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
    flush_telemetry().await;
}
