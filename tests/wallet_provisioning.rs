mod support;
#[path = "support/wallet_integrity.rs"]
mod wallet_integrity;

use std::sync::OnceLock;

use axum::{
    body::Body,
    http::{Method, Request, StatusCode},
    Router,
};
use serde_json::{json, Value};
use sqlx::PgPool;
use subscription::{
    dto::{
        catalog::{
            CatalogStatus, CreateItemRequest, CreatePriceVersionRequest, CreateProductRequest,
            UpdateItemRequest, UpdateProductRequest,
        },
        events::WorkspaceEventEnvelope,
    },
    repositories::database::DatabaseRepository,
    services::{catalog, workspace_events::process_workspace_event},
};
use tokio::sync::Mutex;
use tower::ServiceExt;
use uuid::Uuid;

use support::{response_bytes, response_json, setup_router_with_options};

static TEST_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn wallet_provisioning_converges_for_workspace_lifecycle() {
    let _guard = TEST_LOCK.get_or_init(|| Mutex::new(())).lock().await;
    let (router, pool) = setup_router_with_options(false, None).await;
    let repository = DatabaseRepository::new(pool.clone());
    let product_id = create_billable_catalog(&repository, 10).await;
    create_entitlement_catalog(&repository).await;
    let workspace_id = Uuid::new_v4();

    apply_workspace_event(&repository, workspace_id, "workspace.created", 1).await;
    let created = get_json(&router, &format!("/v1/workspaces/{workspace_id}/wallets")).await;
    assert_eq!(created["ready"], false);
    assert_eq!(created["customer_wallet"]["status"], "PROVISIONING");
    assert_eq!(
        created["item_wallets"]
            .as_array()
            .expect("item wallets")
            .len(),
        10
    );
    assert_parent_links(&created);

    apply_workspace_event(&repository, workspace_id, "workspace.activated", 2).await;
    let active = get_json(&router, &format!("/v1/workspaces/{workspace_id}/wallets")).await;
    assert_eq!(active["ready"], true);
    assert_all_states(&active, "ACTIVE");
    assert_eq!(active["customer_wallet"]["balance_credit_units"], "0");
    let original_wallet_ids = wallet_ids(&active);

    apply_workspace_event(&repository, workspace_id, "workspace.blocked", 3).await;
    let blocked = get_json(&router, &format!("/v1/workspaces/{workspace_id}/wallets")).await;
    assert_all_states(&blocked, "DISABLED");
    apply_workspace_event(&repository, workspace_id, "workspace.activated", 4).await;
    let reactivated = get_json(&router, &format!("/v1/workspaces/{workspace_id}/wallets")).await;
    assert_eq!(wallet_ids(&reactivated), original_wallet_ids);
    assert_all_states(&reactivated, "ACTIVE");
    apply_workspace_event(&repository, workspace_id, "workspace.terminated", 5).await;
    let terminated = get_json(&router, &format!("/v1/workspaces/{workspace_id}/wallets")).await;
    assert_all_states(&terminated, "DISABLED");

    assert_append_only_guards(&pool, workspace_id).await;
    assert_provisioning_outbox(&pool, workspace_id).await;
    assert_wallet_swagger(&router).await;
    deactivate_product(&repository, product_id).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn concurrent_provisioning_reuses_wallets() {
    let _guard = TEST_LOCK.get_or_init(|| Mutex::new(())).lock().await;
    let (_, pool) = setup_router_with_options(false, None).await;
    let repository = DatabaseRepository::new(pool.clone());
    let product_id = create_billable_catalog(&repository, 1).await;
    let workspace_id = Uuid::new_v4();
    let event = workspace_event(workspace_id, "workspace.created", 1);
    let first_repository = repository.clone();
    let second_repository = repository.clone();

    let (first, second) = tokio::join!(
        process_workspace_event(&first_repository, event.clone()),
        process_workspace_event(&second_repository, event)
    );
    assert!(first.is_ok());
    assert!(second.is_ok());
    let counts: (i64, i64, i64) = sqlx::query_as(
        "SELECT count(*),count(*) FILTER (WHERE wallet_type='CUSTOMER'), \
         count(*) FILTER (WHERE wallet_type='ITEM') FROM wallets WHERE customer_id=$1",
    )
    .bind(workspace_id)
    .fetch_one(&pool)
    .await
    .expect("wallet counts");
    assert_eq!(counts, (2, 1, 1));
    let lifecycle_duplicates: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM wallet_lifecycle_events e JOIN wallets w ON w.wallet_id=e.wallet_id \
         WHERE w.customer_id=$1",
    )
    .bind(workspace_id)
    .fetch_one(&pool)
    .await
    .expect("lifecycle count");
    assert_eq!(lifecycle_duplicates, 2);
    deactivate_product(&repository, product_id).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn customer_wallet_waits_for_complete_scope() {
    let _guard = TEST_LOCK.get_or_init(|| Mutex::new(())).lock().await;
    let (router, pool) = setup_router_with_options(false, None).await;
    let repository = DatabaseRepository::new(pool);
    let workspace_id = Uuid::new_v4();
    apply_workspace_event(&repository, workspace_id, "workspace.created", 1).await;
    apply_workspace_event(&repository, workspace_id, "workspace.activated", 2).await;
    let initial = get_json(&router, &format!("/v1/workspaces/{workspace_id}/wallets")).await;
    assert!(initial["ready"].as_bool().expect("ready"));

    let product_id = create_billable_catalog(&repository, 1).await;
    let partial_wallet_id = insert_partial_item_wallet(&repository, workspace_id).await;
    let stale = get_response(&router, &format!("/v1/workspaces/{workspace_id}/wallets")).await;
    assert_eq!(stale.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(
        response_json(stale).await["error"]["code"],
        "wallet_not_provisioned"
    );

    let reconcile = post_json(
        &router,
        &format!("/v1/admin/workspaces/{workspace_id}/wallet-provisioning/reconcile"),
    )
    .await;
    assert_eq!(reconcile["status"], "ACTIVE");
    assert_eq!(reconcile["expected_item_wallets"], 1);
    assert_eq!(reconcile["materialized_item_wallets"], 1);
    let current = get_json(&router, &format!("/v1/workspaces/{workspace_id}/wallets")).await;
    assert_eq!(
        current["item_wallets"]
            .as_array()
            .expect("item wallets")
            .len(),
        1
    );
    assert_eq!(
        current["item_wallets"][0]["wallet_id"],
        partial_wallet_id.to_string()
    );

    let repeated = post_json(
        &router,
        &format!("/v1/admin/workspaces/{workspace_id}/wallet-provisioning/reconcile"),
    )
    .await;
    assert_eq!(repeated["scope_version"], reconcile["scope_version"]);
    deactivate_product(&repository, product_id).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn wallet_provisioning_migration_round_trips() {
    let _guard = TEST_LOCK.get_or_init(|| Mutex::new(())).lock().await;
    let (_, pool) = setup_router_with_options(false, None).await;
    let mut connection = pool.acquire().await.expect("test connection");
    sqlx::query("DROP SCHEMA IF EXISTS wallet_migration_round_trip CASCADE")
        .execute(&mut *connection)
        .await
        .expect("reset isolated schema");
    sqlx::query("CREATE SCHEMA wallet_migration_round_trip")
        .execute(&mut *connection)
        .await
        .expect("create isolated schema");
    sqlx::query("SET search_path TO wallet_migration_round_trip")
        .execute(&mut *connection)
        .await
        .expect("select isolated schema");
    for migration in [
        include_str!("../migrations/202601020000_init.sql"),
        include_str!("../migrations/202609030001_foundation.up.sql"),
        include_str!("../migrations/202609030002_catalog.up.sql"),
        include_str!("../migrations/202609030003_wallet_provisioning.up.sql"),
    ] {
        apply_migration(&mut connection, migration).await;
    }
    let exists: bool = sqlx::query_scalar("SELECT to_regclass('wallets') IS NOT NULL")
        .fetch_one(&mut *connection)
        .await
        .expect("wallet table exists");
    assert!(exists);
    apply_migration(
        &mut connection,
        include_str!("../migrations/202609030003_wallet_provisioning.down.sql"),
    )
    .await;
    let exists: bool = sqlx::query_scalar("SELECT to_regclass('wallets') IS NOT NULL")
        .fetch_one(&mut *connection)
        .await
        .expect("wallet table removed");
    assert!(!exists);
}

async fn create_billable_catalog(repository: &DatabaseRepository, item_count: usize) -> Uuid {
    let product = catalog::create_product(repository, product_request("CREDIT_METERED"))
        .await
        .expect("create metered product");
    for index in 0..item_count {
        let item = catalog::create_item(repository, product.product_id, item_request(index))
            .await
            .expect("create metered item");
        let price = catalog::create_price_version(repository, item.item_id, price_request())
            .await
            .expect("create price");
        catalog::publish_price_version(repository, price.price_version_id)
            .await
            .expect("publish price");
        catalog::update_item(
            repository,
            item.item_id,
            UpdateItemRequest {
                name: None,
                status: Some(CatalogStatus::Active),
                expected_version: 1,
            },
        )
        .await
        .expect("activate item");
    }
    catalog::update_product(
        repository,
        product.product_id,
        UpdateProductRequest {
            name: None,
            description: None,
            status: Some(CatalogStatus::Active),
            expected_version: 1,
        },
    )
    .await
    .expect("activate product");
    product.product_id
}

async fn create_entitlement_catalog(repository: &DatabaseRepository) {
    let product = catalog::create_product(repository, product_request("ENTITLEMENT_ONLY"))
        .await
        .expect("create entitlement product");
    catalog::create_item(
        repository,
        product.product_id,
        serde_json::from_value(json!({
            "name":"Entitlement item","parent_item_id":null,"unit_name":null,"quantity_scale":null
        }))
        .expect("entitlement item request"),
    )
    .await
    .expect("create entitlement item");
}

async fn deactivate_product(repository: &DatabaseRepository, product_id: Uuid) {
    catalog::update_product(
        repository,
        product_id,
        UpdateProductRequest {
            name: None,
            description: None,
            status: Some(CatalogStatus::Inactive),
            expected_version: 2,
        },
    )
    .await
    .expect("deactivate product");
}

async fn insert_partial_item_wallet(repository: &DatabaseRepository, workspace_id: Uuid) -> Uuid {
    let pool = repository.pool();
    let customer_wallet_id: Uuid = sqlx::query_scalar(
        "SELECT wallet_id FROM wallets WHERE customer_id=$1 AND wallet_type='CUSTOMER'",
    )
    .bind(workspace_id)
    .fetch_one(&pool)
    .await
    .expect("customer wallet");
    let (scope_version, item_id): (Uuid, Uuid) = sqlx::query_as(
        "SELECT c.scope_version,i.item_id FROM catalog_scope_current c \
         JOIN catalog_scope_items i ON i.scope_version=c.scope_version WHERE c.singleton",
    )
    .fetch_one(&pool)
    .await
    .expect("current scope item");
    let wallet_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO wallets (wallet_id,customer_id,wallet_type,parent_customer_wallet_id,item_id, \
         provisioning_scope_version) VALUES ($1,$2,'ITEM',$3,$4,$5)",
    )
    .bind(wallet_id)
    .bind(workspace_id)
    .bind(customer_wallet_id)
    .bind(item_id)
    .bind(scope_version)
    .execute(&pool)
    .await
    .expect("insert partial item wallet");
    wallet_id
}

async fn apply_workspace_event(
    repository: &DatabaseRepository,
    workspace_id: Uuid,
    event_type: &str,
    sequence: i64,
) {
    process_workspace_event(
        repository,
        workspace_event(workspace_id, event_type, sequence),
    )
    .await
    .expect("apply workspace event");
}

fn workspace_event(workspace_id: Uuid, event_type: &str, sequence: i64) -> WorkspaceEventEnvelope {
    serde_json::from_value(json!({
        "event_id":Uuid::new_v4(),"event_type":event_type,"schema_version":1,
        "aggregate_id":workspace_id,"sequence":sequence,"occurred_at":"2026-09-03T12:00:00Z",
        "workspace_id":workspace_id,"correlation_id":Uuid::new_v4(),"causation_id":null,
        "payload":{"workspace_id":workspace_id}
    }))
    .expect("workspace event")
}

fn product_request(usage_model: &str) -> CreateProductRequest {
    serde_json::from_value(json!({
        "name":format!("Product {}",Uuid::new_v4()),"description":null,"usage_model":usage_model
    }))
    .expect("product request")
}

fn item_request(index: usize) -> CreateItemRequest {
    serde_json::from_value(json!({
        "name":format!("Metered item {index}"),"parent_item_id":null,
        "unit_name":"request","quantity_scale":"1"
    }))
    .expect("item request")
}

fn price_request() -> CreatePriceVersionRequest {
    serde_json::from_value(json!({
        "pricing_model":"unit","unit_block_size":"1","credit_units":"1",
        "effective_from":"2026-09-01T00:00:00Z","effective_until":null,
        "accumulation_cycle":null,"tiers":[]
    }))
    .expect("price request")
}

fn assert_parent_links(hierarchy: &Value) {
    let customer_id = hierarchy["customer_wallet"]["wallet_id"]
        .as_str()
        .expect("customer wallet id");
    for wallet in hierarchy["item_wallets"].as_array().expect("item wallets") {
        assert_eq!(wallet["parent_customer_wallet_id"], customer_id);
        assert!(wallet.get("balance_credit_units").is_none());
    }
}

fn assert_all_states(hierarchy: &Value, expected: &str) {
    assert_eq!(hierarchy["customer_wallet"]["status"], expected);
    for wallet in hierarchy["item_wallets"].as_array().expect("item wallets") {
        assert_eq!(wallet["status"], expected);
    }
}

fn wallet_ids(hierarchy: &Value) -> Vec<String> {
    let mut ids = vec![hierarchy["customer_wallet"]["wallet_id"]
        .as_str()
        .expect("customer wallet id")
        .to_string()];
    ids.extend(
        hierarchy["item_wallets"]
            .as_array()
            .expect("item wallets")
            .iter()
            .map(|wallet| {
                wallet["wallet_id"]
                    .as_str()
                    .expect("item wallet id")
                    .to_string()
            }),
    );
    ids
}

async fn assert_append_only_guards(pool: &PgPool, workspace_id: Uuid) {
    let wallet_id: Uuid =
        sqlx::query_scalar("SELECT wallet_id FROM wallets WHERE customer_id=$1 LIMIT 1")
            .bind(workspace_id)
            .fetch_one(pool)
            .await
            .expect("wallet id");
    assert!(
        sqlx::query("UPDATE wallets SET customer_id=$2 WHERE wallet_id=$1")
            .bind(wallet_id)
            .bind(Uuid::new_v4())
            .execute(pool)
            .await
            .is_err()
    );
    assert!(
        sqlx::query("DELETE FROM wallet_lifecycle_events WHERE wallet_id=$1")
            .bind(wallet_id)
            .execute(pool)
            .await
            .is_err()
    );
}

async fn assert_provisioning_outbox(pool: &PgPool, workspace_id: Uuid) {
    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM outbox_events WHERE workspace_id=$1 \
         AND event_type LIKE 'workspace_provisioning.%'",
    )
    .bind(workspace_id)
    .fetch_one(pool)
    .await
    .expect("provisioning outbox count");
    assert_eq!(count, 6);
}

async fn assert_wallet_swagger(router: &Router) {
    let openapi = get_json(router, "/openapi.json").await;
    assert!(openapi["paths"]
        .get("/v1/workspaces/{workspace_id}/wallets")
        .is_some());
    assert!(openapi["paths"]
        .get("/v1/admin/workspaces/{workspace_id}/wallet-provisioning/reconcile")
        .is_some());
    assert!(openapi["components"]["schemas"]
        .get("WalletHierarchyResponse")
        .is_some());
    let docs = get_response(router, "/docs/").await;
    assert_eq!(docs.status(), StatusCode::OK);
    let body = response_bytes(docs).await;
    assert!(String::from_utf8_lossy(&body).contains("Swagger UI"));
}

async fn get_json(router: &Router, uri: &str) -> Value {
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
                .expect("get request"),
        )
        .await
        .expect("get response")
}

async fn post_json(router: &Router, uri: &str) -> Value {
    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri(uri)
                .body(Body::empty())
                .expect("post request"),
        )
        .await
        .expect("post response");
    assert_eq!(response.status(), StatusCode::OK);
    response_json(response).await
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
