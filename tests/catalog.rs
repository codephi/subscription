mod support;

use axum::{
    body::Body,
    http::{Method, Request, StatusCode},
    Router,
};
use serde_json::{json, Value};
use sqlx::PgPool;
use subscription::{repositories::database::DatabaseRepository, services::catalog};
use tower::ServiceExt;
use uuid::Uuid;

use support::response_json;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn published_catalog_scope_is_reproducible() {
    let (router, pool) = setup().await;
    let product = create_product(&router, "CREDIT_METERED").await;
    let product_id = uuid(&product, "product_id");
    let item = create_item(&router, product_id).await;
    let item_id = uuid(&item, "item_id");
    let price = create_price(&router, item_id, unit_price()).await;
    let price_id = uuid(&price, "price_version_id");

    let published = post_empty(&router, &format!("/v1/price-versions/{price_id}/publish")).await;
    assert_eq!(published["state"], "ACTIVE");
    patch_status(&router, "products", product_id, "ACTIVE").await;
    patch_status(&router, "items", item_id, "ACTIVE").await;

    let first_scope = get(&router, "/v1/catalog-scope/current").await;
    assert_eq!(first_scope["items"][0]["item_id"], item_id.to_string());
    assert_eq!(
        first_scope["items"][0]["price_version_id"],
        price_id.to_string()
    );
    let second_scope = get(&router, "/v1/catalog-scope/current").await;
    assert_eq!(first_scope, second_scope);
    assert_scope_can_leave_and_reenter(&router, item_id, &first_scope).await;

    assert_published_price_is_immutable(&router, &pool, price_id).await;
    assert_overlapping_price_is_rejected(&router, item_id).await;
    assert_entitlement_only_cannot_be_activated(&router).await;
    assert_openapi_contains_catalog(&router).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn price_version_validation_and_overflow() {
    let (router, _) = setup().await;
    let product = create_product(&router, "CREDIT_METERED").await;
    let item = create_item(&router, uuid(&product, "product_id")).await;
    let item_id = uuid(&item, "item_id");

    let tiered = json!({
        "pricing_model": "tiered",
        "effective_from": "2026-09-01T00:00:00Z",
        "accumulation_cycle": {
            "anchor_at": "2026-01-01T00:00:00Z",
            "recurrence_rule": "FREQ=MONTHLY;INTERVAL=3"
        },
        "tiers": [
            {"from_accumulated_units":"0","to_accumulated_units":"10","unit_block_size":"1","credit_units":"1"},
            {"from_accumulated_units":"10","to_accumulated_units":null,"unit_block_size":"2","credit_units":"1"}
        ]
    });
    let response = request(&router, Method::POST, &price_path(item_id), tiered).await;
    assert_eq!(response.status(), StatusCode::CREATED);
    let body = response_json(response).await;
    assert_eq!(body["tiers"][1]["unit_block_size"], "2");

    assert_invalid_price(&router, item_id, invalid_gap_price()).await;
    assert_invalid_price(&router, item_id, overflowing_tier_price()).await;
    assert_invalid_cycle(&router, item_id).await;
    assert_decimal_contract_rejects_numbers(&router, item_id).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn concurrent_price_publication_allows_one_period() {
    let (_, pool) = setup().await;
    let repository = DatabaseRepository::new(pool);
    let product = catalog::create_product(
        &repository,
        serde_json::from_value(json!({
            "name":"Concurrent product","description":null,"usage_model":"CREDIT_METERED"
        }))
        .expect("product request"),
    )
    .await
    .expect("create product");
    let item = catalog::create_item(
        &repository,
        product.product_id,
        serde_json::from_value(json!({
            "name":"Concurrent item","parent_item_id":null,"unit_name":"request","quantity_scale":"1"
        }))
        .expect("item request"),
    )
    .await
    .expect("create item");
    let first = create_price_service(&repository, item.item_id).await;
    let second = create_price_service(&repository, item.item_id).await;

    let (first_result, second_result) = tokio::join!(
        catalog::publish_price_version(&repository, first),
        catalog::publish_price_version(&repository, second)
    );
    assert_eq!(
        usize::from(first_result.is_ok()) + usize::from(second_result.is_ok()),
        1
    );
    let error = first_result
        .err()
        .or_else(|| second_result.err())
        .expect("one conflict");
    assert_eq!(error.code(), "price_period_overlap");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn catalog_migration_round_trips() {
    let (_, pool) = setup().await;
    let mut connection = pool.acquire().await.expect("test connection");
    sqlx::query("DROP SCHEMA IF EXISTS catalog_migration_round_trip CASCADE")
        .execute(&mut *connection)
        .await
        .expect("reset isolated schema");
    sqlx::query("CREATE SCHEMA catalog_migration_round_trip")
        .execute(&mut *connection)
        .await
        .expect("create isolated schema");
    sqlx::query("SET search_path TO catalog_migration_round_trip")
        .execute(&mut *connection)
        .await
        .expect("select isolated schema");
    apply_migration(
        &mut connection,
        include_str!("../migrations/202601020000_init.sql"),
    )
    .await;
    apply_migration(
        &mut connection,
        include_str!("../migrations/202609030001_foundation.up.sql"),
    )
    .await;
    apply_migration(
        &mut connection,
        include_str!("../migrations/202609030002_catalog.up.sql"),
    )
    .await;
    let exists: bool = sqlx::query_scalar("SELECT to_regclass('catalog_scope_items') IS NOT NULL")
        .fetch_one(&mut *connection)
        .await
        .expect("catalog table exists");
    assert!(exists);
    apply_migration(
        &mut connection,
        include_str!("../migrations/202609030002_catalog.down.sql"),
    )
    .await;
    let exists: bool = sqlx::query_scalar("SELECT to_regclass('products') IS NOT NULL")
        .fetch_one(&mut *connection)
        .await
        .expect("catalog table removed");
    assert!(!exists);
}

async fn setup() -> (Router, PgPool) {
    support::setup_router_with_options(false, None).await
}

async fn create_product(router: &Router, usage_model: &str) -> Value {
    let body = json!({"name":format!("Product {}", Uuid::new_v4()),"description":null,"usage_model":usage_model});
    let response = request(router, Method::POST, "/v1/products", body).await;
    assert_eq!(response.status(), StatusCode::CREATED);
    response_json(response).await
}

async fn create_item(router: &Router, product_id: Uuid) -> Value {
    let body = json!({
        "name":format!("Item {}", Uuid::new_v4()),"parent_item_id":null,
        "unit_name":"request","quantity_scale":"1"
    });
    let response = request(
        router,
        Method::POST,
        &format!("/v1/products/{product_id}/items"),
        body,
    )
    .await;
    assert_eq!(response.status(), StatusCode::CREATED);
    response_json(response).await
}

async fn create_price(router: &Router, item_id: Uuid, body: Value) -> Value {
    let response = request(router, Method::POST, &price_path(item_id), body).await;
    assert_eq!(response.status(), StatusCode::CREATED);
    response_json(response).await
}

async fn patch_status(router: &Router, kind: &str, id: Uuid, status: &str) -> Value {
    let response = request(
        router,
        Method::PATCH,
        &format!("/v1/{kind}/{id}"),
        json!({"name":null,"status":status,"expected_version":1}),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    response_json(response).await
}

async fn assert_published_price_is_immutable(router: &Router, pool: &PgPool, price_id: Uuid) {
    let response = router
        .clone()
        .oneshot(empty_request(
            Method::POST,
            &format!("/v1/price-versions/{price_id}/publish"),
        ))
        .await
        .expect("repeat publish request");
    assert_eq!(response.status(), StatusCode::CONFLICT);
    let result =
        sqlx::query("UPDATE price_versions SET credit_units=999 WHERE price_version_id=$1")
            .bind(price_id)
            .execute(pool)
            .await;
    assert!(result.is_err());
    let stored = get(router, &format!("/v1/price-versions/{price_id}")).await;
    assert_eq!(stored["credit_units"], "100");
}

async fn assert_scope_can_leave_and_reenter(router: &Router, item_id: Uuid, original: &Value) {
    patch_status_version(router, "items", item_id, "INACTIVE", 2).await;
    let empty_scope = get(router, "/v1/catalog-scope/current").await;
    assert_eq!(empty_scope["items"], json!([]));
    patch_status_version(router, "items", item_id, "ACTIVE", 3).await;
    let restored_scope = get(router, "/v1/catalog-scope/current").await;
    assert_eq!(restored_scope["scope_version"], original["scope_version"]);
    assert_eq!(restored_scope["items"], original["items"]);
}

async fn assert_overlapping_price_is_rejected(router: &Router, item_id: Uuid) {
    let overlapping = create_price(router, item_id, unit_price()).await;
    let id = uuid(&overlapping, "price_version_id");
    let response = router
        .clone()
        .oneshot(empty_request(
            Method::POST,
            &format!("/v1/price-versions/{id}/publish"),
        ))
        .await
        .expect("publish overlapping price");
    assert_eq!(response.status(), StatusCode::CONFLICT);
    assert_eq!(
        response_json(response).await["error"]["code"],
        "price_period_overlap"
    );
}

async fn patch_status_version(
    router: &Router,
    kind: &str,
    id: Uuid,
    status: &str,
    expected_version: i64,
) -> Value {
    let response = request(
        router,
        Method::PATCH,
        &format!("/v1/{kind}/{id}"),
        json!({"name":null,"status":status,"expected_version":expected_version}),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    response_json(response).await
}

async fn assert_entitlement_only_cannot_be_activated(router: &Router) {
    let product = create_product(router, "ENTITLEMENT_ONLY").await;
    let response = request(
        router,
        Method::PATCH,
        &format!("/v1/products/{}", uuid(&product, "product_id")),
        json!({"name":null,"description":null,"status":"ACTIVE","expected_version":1}),
    )
    .await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
}

async fn assert_openapi_contains_catalog(router: &Router) {
    let openapi = get(router, "/openapi.json").await;
    assert!(openapi["paths"].get("/v1/products").is_some());
    assert!(openapi["paths"].get("/v1/catalog-scope/current").is_some());
}

async fn assert_invalid_price(router: &Router, item_id: Uuid, body: Value) {
    let response = request(router, Method::POST, &price_path(item_id), body).await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        response_json(response).await["error"]["code"],
        "invalid_price"
    );
}

async fn assert_invalid_cycle(router: &Router, item_id: Uuid) {
    let mut body = invalid_gap_price();
    body["tiers"] = json!([
        {"from_accumulated_units":"0","to_accumulated_units":null,"unit_block_size":"1","credit_units":"1"}
    ]);
    body["accumulation_cycle"] = json!({
        "anchor_at":"2026-01-01T00:00:00Z","recurrence_rule":"COUNT=1"
    });
    let response = request(router, Method::POST, &price_path(item_id), body).await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        response_json(response).await["error"]["code"],
        "invalid_accumulation_cycle"
    );
}

async fn assert_decimal_contract_rejects_numbers(router: &Router, item_id: Uuid) {
    let mut body = unit_price();
    body["unit_block_size"] = json!(1000);
    let response = request(router, Method::POST, &price_path(item_id), body).await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
}

async fn create_price_service(repository: &DatabaseRepository, item_id: Uuid) -> Uuid {
    let request = serde_json::from_value(unit_price()).expect("price request");
    catalog::create_price_version(repository, item_id, request)
        .await
        .expect("create draft")
        .price_version_id
}

fn unit_price() -> Value {
    json!({
        "pricing_model":"unit","unit_block_size":"1000","credit_units":"100",
        "effective_from":"2026-09-01T00:00:00Z","effective_until":null,
        "accumulation_cycle":null,"tiers":[]
    })
}

fn invalid_gap_price() -> Value {
    json!({
        "pricing_model":"tiered","effective_from":"2026-09-01T00:00:00Z",
        "accumulation_cycle":null,"tiers":[
            {"from_accumulated_units":"0","to_accumulated_units":"10","unit_block_size":"2","credit_units":"1"},
            {"from_accumulated_units":"11","to_accumulated_units":null,"unit_block_size":"1","credit_units":"1"}
        ]
    })
}

fn overflowing_tier_price() -> Value {
    json!({
        "pricing_model":"tiered","effective_from":"2026-09-01T00:00:00Z",
        "accumulation_cycle":null,"tiers":[
            {"from_accumulated_units":"0","to_accumulated_units":"9223372036854775807","unit_block_size":"1","credit_units":"2"},
            {"from_accumulated_units":"9223372036854775807","to_accumulated_units":null,"unit_block_size":"1","credit_units":"1"}
        ]
    })
}

fn uuid(value: &Value, field: &str) -> Uuid {
    Uuid::parse_str(value[field].as_str().expect("uuid field")).expect("valid uuid")
}

fn price_path(item_id: Uuid) -> String {
    format!("/v1/items/{item_id}/price-versions")
}

async fn get(router: &Router, uri: &str) -> Value {
    let response = router
        .clone()
        .oneshot(empty_request(Method::GET, uri))
        .await
        .expect("get request");
    assert_eq!(response.status(), StatusCode::OK);
    response_json(response).await
}

async fn post_empty(router: &Router, uri: &str) -> Value {
    let response = router
        .clone()
        .oneshot(empty_request(Method::POST, uri))
        .await
        .expect("post request");
    assert_eq!(response.status(), StatusCode::OK);
    response_json(response).await
}

async fn request(
    router: &Router,
    method: Method,
    uri: &str,
    body: Value,
) -> axum::response::Response {
    router
        .clone()
        .oneshot(
            Request::builder()
                .method(method)
                .uri(uri)
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .expect("json request"),
        )
        .await
        .expect("request")
}

fn empty_request(method: Method, uri: &str) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .body(Body::empty())
        .expect("empty request")
}

async fn apply_migration(
    connection: &mut sqlx::pool::PoolConnection<sqlx::Postgres>,
    sql: &'static str,
) {
    sqlx::raw_sql(sql)
        .execute(&mut **connection)
        .await
        .expect("apply migration");
}
