mod support;

use axum::{
    body::Body,
    http::{Method, Request},
    Router,
};
use chrono::{TimeZone, Utc};
use serde_json::{json, Value};
use sqlx::PgPool;
use subscription::services::calendar::pricing_cycle_bounds;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn catalog_rejects_each_invalid_tier_shape_without_persisting_price() {
    let (router, pool, item) = catalog_fixture().await;
    for tiers in invalid_tier_shapes() {
        let mut price = tiered_price();
        price["tiers"] = tiers;
        let (status, response) = send(&router, Method::POST, &price_path(&item), price).await;
        assert_eq!(status, 422, "invalid tiers: {response}");
        assert_eq!(response["error"]["code"], "invalid_price");
    }
    assert_no_prices(&pool).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn catalog_rejects_invalid_or_unrepresentable_cycles_before_publication() {
    let (router, pool, item) = catalog_fixture().await;
    for rule in [
        "",
        "COUNT=1",
        "FREQ=DAILY;INTERVAL=0",
        "FREQ=DAILY;INTERVAL=1;COUNT=2",
        "FREQ=MONTHLY;INTERVAL=1;UNTIL=20270101",
        "FREQ=MONTHLY;INTERVAL=1;INTERVAL=2",
        "FREQ=DAILY;INTERVAL=4294967295",
        "FREQ=WEEKLY;INTERVAL=4294967295",
        "FREQ=YEARLY;INTERVAL=4294967295",
    ] {
        let mut price = tiered_price();
        price["accumulation_cycle"] =
            json!({"anchor_at":"2026-01-31T10:00:00Z","recurrence_rule":rule});
        let (status, response) = send(&router, Method::POST, &price_path(&item), price).await;
        assert_eq!(status, 422, "rule {rule}: {response}");
        assert_eq!(response["error"]["code"], "invalid_accumulation_cycle");
    }
    assert_no_prices(&pool).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn catalog_missing_cycle_rule_is_rejected_without_persisting_price() {
    let (router, pool, item) = catalog_fixture().await;
    let mut price = tiered_price();
    price["accumulation_cycle"] = json!({"anchor_at":"2026-01-31T10:00:00Z"});
    assert_eq!(
        send(&router, Method::POST, &price_path(&item), price)
            .await
            .0,
        422
    );
    assert_no_prices(&pool).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn published_quarterly_price_preserves_declarative_cycle_and_credit_contract() {
    let (router, _, item) = catalog_fixture().await;
    let mut price = tiered_price();
    price["accumulation_cycle"] =
        json!({"anchor_at":"2026-01-31T10:00:00Z","recurrence_rule":"FREQ=MONTHLY;INTERVAL=3"});
    let (status, created) = send(&router, Method::POST, &price_path(&item), price.clone()).await;
    assert_eq!(status, 201);
    let path = format!(
        "/v1/price-versions/{}/publish",
        created["price_version_id"].as_str().unwrap()
    );
    let (status, published) = send(&router, Method::POST, &path, json!({})).await;
    assert_eq!(status, 200);
    assert_eq!(published["accumulation_cycle"], price["accumulation_cycle"]);
    assert_eq!(published["tiers"], price["tiers"]);
    assert!(published.get("currency").is_none() && published.get("price_amount_minor").is_none());
    assert_quarterly_boundaries();
}

#[test]
fn pricing_calendar_extreme_intervals_return_errors_without_panicking() {
    let anchor = Utc.with_ymd_and_hms(2026, 1, 31, 10, 0, 0).unwrap();
    for frequency in ["DAILY", "WEEKLY", "MONTHLY", "YEARLY"] {
        let rule = format!("FREQ={frequency};INTERVAL={}", i64::MAX);
        assert!(
            pricing_cycle_bounds(anchor, &rule, anchor).is_err(),
            "rule {rule}"
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn catalog_v1_rejects_future_models_without_publication() {
    let (router, pool) = support::setup_router_with_options(false, None).await;
    let (_, product) = send(
        &router,
        Method::POST,
        "/v1/products",
        json!({"name":"Future", "usage_model":"ENTITLEMENT_ONLY"}),
    )
    .await;
    let path = format!("/v1/products/{}", product["product_id"].as_str().unwrap());
    let (status, error) = send(
        &router,
        Method::PATCH,
        &path,
        json!({"status":"ACTIVE","expected_version":1}),
    )
    .await;
    assert_eq!(status, 422);
    assert_eq!(error["error"]["code"], "usage_model_not_publishable");
    assert_eq!(
        send(
            &router,
            Method::POST,
            "/v1/subscriptions",
            json!({"name":"Future","subscription_model":"CREDIT_FLEXIBLE"})
        )
        .await
        .0,
        422
    );
    let active: i64 = sqlx::query_scalar("SELECT count(*) FROM products WHERE status='ACTIVE'")
        .fetch_one(&pool)
        .await
        .unwrap();
    let subscriptions: i64 = sqlx::query_scalar("SELECT count(*) FROM subscriptions")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!((active, subscriptions), (0, 0));
}

fn assert_quarterly_boundaries() {
    let anchor = Utc.with_ymd_and_hms(2026, 1, 31, 10, 0, 0).unwrap();
    let april = Utc.with_ymd_and_hms(2026, 4, 30, 10, 0, 0).unwrap();
    let july = Utc.with_ymd_and_hms(2026, 7, 31, 10, 0, 0).unwrap();
    assert_eq!(
        pricing_cycle_bounds(anchor, "FREQ=MONTHLY;INTERVAL=3", anchor).unwrap(),
        (anchor, april)
    );
    assert_eq!(
        pricing_cycle_bounds(anchor, "FREQ=MONTHLY;INTERVAL=3", april).unwrap(),
        (april, july)
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn legacy_draft_with_impossible_cycle_cannot_be_published() {
    let (router, pool, item) = catalog_fixture().await;
    let initial_scopes: i64 = sqlx::query_scalar("SELECT count(*) FROM catalog_scope_versions")
        .fetch_one(&pool)
        .await
        .unwrap();
    let (_, draft) = send(&router, Method::POST, &price_path(&item), tiered_price()).await;
    let id = draft["price_version_id"].as_str().unwrap();
    sqlx::query("UPDATE price_versions SET accumulation_anchor_at=effective_from, accumulation_recurrence_rule='FREQ=YEARLY;INTERVAL=4294967295' WHERE price_version_id=$1::text::uuid")
        .bind(id).execute(&pool).await.unwrap();
    let (status, error) = send(
        &router,
        Method::POST,
        &format!("/v1/price-versions/{id}/publish"),
        json!({}),
    )
    .await;
    assert_eq!(status, 422, "legacy draft: {error}");
    assert_eq!(error["error"]["code"], "invalid_accumulation_cycle");
    let state: String = sqlx::query_scalar(
        "SELECT state FROM price_versions WHERE price_version_id=$1::text::uuid",
    )
    .bind(id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(state, "DRAFT");
    let scopes: i64 = sqlx::query_scalar("SELECT count(*) FROM catalog_scope_versions")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(scopes, initial_scopes);
}

fn invalid_tier_shapes() -> Vec<Value> {
    vec![
        json!([tier(0, Some(10), 1), tier(11, None, 1)]),
        json!([tier(0, Some(10), 1), tier(9, None, 1)]),
        json!([tier(0, Some(11), 2), tier(11, None, 1)]),
        json!([tier(0, Some(10), 1)]),
        json!([tier(0, None, 1), tier(10, None, 1)]),
        json!([tier(0, Some(0), 1), tier(0, None, 1)]),
        json!([tier(0, Some(10), 1), tier(10, Some(5), 1), tier(5, None, 1)]),
    ]
}

fn tier(start: i64, end: Option<i64>, block: i64) -> Value {
    json!({"from_accumulated_units":start.to_string(),"to_accumulated_units":end.map(|value|value.to_string()),
        "unit_block_size":block.to_string(),"credit_units":"1"})
}

fn tiered_price() -> Value {
    json!({"pricing_model":"tiered","effective_from":"2026-01-31T10:00:00Z",
        "tiers":[tier(0, Some(10), 1), tier(10, None, 2)]})
}

async fn catalog_fixture() -> (Router, PgPool, String) {
    let (router, pool) = support::setup_router_with_options(false, None).await;
    let (status, product) = send(
        &router,
        Method::POST,
        "/v1/products",
        json!({"name":"Metered", "usage_model":"CREDIT_METERED"}),
    )
    .await;
    assert_eq!(status, 201);
    let path = format!(
        "/v1/products/{}/items",
        product["product_id"].as_str().unwrap()
    );
    let (status, item) = send(
        &router,
        Method::POST,
        &path,
        json!({"name":"Requests", "unit_name":"request", "quantity_scale":"1"}),
    )
    .await;
    assert_eq!(status, 201);
    (router, pool, item["item_id"].as_str().unwrap().to_string())
}

async fn send(router: &Router, method: Method, path: &str, body: Value) -> (u16, Value) {
    let request = Request::builder()
        .method(method)
        .uri(path)
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();
    let response = tower::ServiceExt::oneshot(router.clone(), request)
        .await
        .unwrap();
    let status = response.status().as_u16();
    let bytes = support::response_bytes(response).await;
    (
        status,
        serde_json::from_slice(&bytes)
            .unwrap_or_else(|_| json!({"body":String::from_utf8_lossy(&bytes)})),
    )
}

async fn assert_no_prices(pool: &PgPool) {
    let counts: (i64, i64) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM price_versions), (SELECT count(*) FROM price_tiers)",
    )
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(counts, (0, 0));
}

fn price_path(item: &str) -> String {
    format!("/v1/items/{item}/price-versions")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn catalog_unit_prices_preserve_decimal_credit_ratios_and_maximum() {
    let (router, _, item) = catalog_fixture().await;
    for (block, credits) in [(1, 1), (10, 1), (10, 10), (1000, 100), (1, i64::MAX)] {
        let price = json!({"pricing_model":"unit","effective_from":"2026-01-01T00:00:00Z",
            "unit_block_size":block.to_string(),"credit_units":credits.to_string()});
        let (status, response) =
            send(&router, Method::POST, &price_path(&item), price.clone()).await;
        assert_eq!(status, 201);
        assert_eq!(response["unit_block_size"], price["unit_block_size"]);
        assert_eq!(response["credit_units"], price["credit_units"]);
        assert!(response.get("currency").is_none() && response.get("price_amount_minor").is_none());
    }
    let (_, document) = send(&router, Method::GET, "/openapi.json", json!({})).await;
    let responses =
        &document["paths"]["/v1/price-versions/{price_id}/publish"]["post"]["responses"];
    assert!(responses.get("422").is_some());
}
