use axum::{body::Body, http::Request};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use subscription::{
    config::{AppConfig, CorsConfig, McpConfig, DEFAULT_BODY_LIMIT_BYTES, DEFAULT_MCP_PATH},
    repositories::database::DatabaseRepository,
    routes::create_router,
    state::AppState,
};
use tower::ServiceExt;
use uuid::Uuid;

#[tokio::test]
async fn regularization_route_requires_idempotency_and_is_in_openapi() {
    let router = test_router();
    let path = format!(
        "/v1/workspaces/{}/customer-plans/{}/renewal-regularizations",
        Uuid::new_v4(),
        Uuid::new_v4()
    );
    let response = router
        .clone()
        .oneshot(
            Request::post(&path)
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "payment_method_binding_id": Uuid::new_v4(),
                        "transaction_id": "regularization-route"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), 422);
    assert_eq!(
        response_json(response).await["error"]["code"],
        "idempotency_key_required"
    );

    let openapi = router
        .oneshot(Request::get("/openapi.json").body(Body::empty()).unwrap())
        .await
        .unwrap();
    let document = response_json(openapi).await;
    assert!(document["paths"]
        .get("/v1/workspaces/{workspace_id}/customer-plans/{customer_plan_id}/renewal-regularizations")
        .is_some());
}

#[tokio::test]
async fn stripe_provider_is_advertised_without_database_access() {
    let router = test_router();
    let response = router
        .oneshot(
            Request::get("/v1/admin/integrations/providers")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let providers = response_json(response).await;
    assert_eq!(providers[0]["provider"], "STRIPE");
    assert_eq!(providers[0]["available"], true);
}

fn test_router() -> axum::Router {
    let pool = sqlx::postgres::PgPoolOptions::new()
        .connect_lazy("postgres://postgres:postgres@127.0.0.1:1/postgres")
        .unwrap();
    let state = AppState::new(DatabaseRepository::new(pool));
    create_router(state, &test_config())
}

fn test_config() -> AppConfig {
    AppConfig {
        database_url: String::new(),
        host: "127.0.0.1".parse().unwrap(),
        port: 0,
        cors: CorsConfig::Permissive,
        body_limit_bytes: DEFAULT_BODY_LIMIT_BYTES,
        otel_enabled: false,
        mcp: McpConfig {
            enabled: false,
            path: DEFAULT_MCP_PATH.to_string(),
            cors: CorsConfig::Permissive,
        },
        accounts_webhook_secret: None,
        outbound_event_webhook: None,
        public_api_base_url: None,
    }
}

async fn response_json(response: axum::response::Response) -> Value {
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}
