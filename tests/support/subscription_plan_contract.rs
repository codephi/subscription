use axum::{
    body::Body,
    http::{Method, Request, StatusCode},
    Router,
};
use serde_json::Value;
use tower::ServiceExt;

use super::support::{response_bytes, response_json};

pub async fn assert_plan_swagger(router: &Router) {
    let openapi = get_json(router, "/openapi.json").await;
    for path in [
        "/v1/subscriptions",
        "/v1/accounts/{account_id}/customer-plans",
        "/v1/accounts/{account_id}/customer-plans/{customer_plan_id}/plan-transitions",
        "/v1/admin/accounts/{account_id}/customer-plans/{customer_plan_id}/revoke",
        "/v1/admin/subscription-cycles/run",
    ] {
        assert!(
            openapi["paths"].get(path).is_some(),
            "missing OpenAPI path {path}"
        );
    }
    assert!(openapi["components"]["schemas"]
        .get("CustomerPlanResponse")
        .is_some());
    assert!(openapi["components"]["schemas"]
        .get("PlanTransitionResponse")
        .is_some());
    let docs = get_response(router, "/docs/").await;
    assert_eq!(docs.status(), StatusCode::OK);
    assert!(String::from_utf8_lossy(&response_bytes(docs).await).contains("Swagger UI"));
}

pub async fn get_json(router: &Router, uri: &str) -> Value {
    let response = get_response(router, uri).await;
    assert_eq!(response.status(), StatusCode::OK);
    response_json(response).await
}

async fn get_response(router: &Router, uri: &str) -> axum::response::Response {
    router
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri(uri)
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response")
}
