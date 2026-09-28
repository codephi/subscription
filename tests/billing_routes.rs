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

#[tokio::test]
async fn customer_payment_method_contract_hides_integration_identifiers() {
    let router = test_router();
    let response = router
        .oneshot(Request::get("/openapi.json").body(Body::empty()).unwrap())
        .await
        .unwrap();
    let document = response_json(response).await;
    assert!(document["paths"]
        .get("/v1/workspaces/{workspace_id}/payment-method-setup-sessions")
        .is_some());
    assert!(document["paths"]
        .get("/v1/workspaces/{workspace_id}/payment-methods/from-card")
        .is_some());
    assert!(document["paths"]
        ["/v1/workspaces/{workspace_id}/payment-method-bindings/{binding_id}"]
        .get("delete")
        .is_some());
    let card_request = &document["components"]["schemas"]["CreatePaymentMethodFromCardRequest"];
    assert!(card_request["properties"].get("card_name").is_some());
    let binding = &document["components"]["schemas"]["CreatePaymentMethodBindingRequest"];
    assert!(binding["properties"]
        .get("payment_method_setup_id")
        .is_some());
    assert!(binding["properties"].get("billing_connection_id").is_none());
    let response = &document["components"]["schemas"]["CustomerPaymentMethodBindingResponse"];
    assert!(response["properties"]
        .get("billing_connection_id")
        .is_none());
    assert!(response["properties"]
        .get("provider_payment_method_reference")
        .is_none());
    assert!(response["properties"].get("display_name").is_some());
}

#[tokio::test]
async fn transparent_checkout_is_documented_and_disabled_without_sandbox_config() {
    let router = test_router();
    let workspace_id = Uuid::new_v4();
    let response = router
        .clone()
        .oneshot(
            Request::post(format!("/v1/workspaces/{workspace_id}/checkouts"))
                .header("content-type", "application/json")
                .header("idempotency-key", "checkout-route-test")
                .body(Body::from(
                    json!({
                        "customer_plan_id": Uuid::new_v4(),
                        "checkout_kind": "INITIAL",
                        "on_demand_plan_id": null,
                        "transaction_id": "checkout-route-test"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), 503);
    assert_eq!(
        response_json(response).await["error"]["code"],
        "billing_checkout_disabled"
    );

    let openapi = router
        .oneshot(Request::get("/openapi.json").body(Body::empty()).unwrap())
        .await
        .unwrap();
    let document = response_json(openapi).await;
    assert!(document["paths"]
        .get("/v1/workspaces/{workspace_id}/checkouts")
        .is_some());
    assert!(document["paths"]
        .get("/v1/workspaces/{workspace_id}/checkouts/{checkout_id}")
        .is_some());
    assert!(document["paths"]
        .get("/v1/billing/webhooks/stripe")
        .is_some());
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
