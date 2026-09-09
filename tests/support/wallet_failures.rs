use super::{
    apply_workspace_event, create_billable_catalog, setup_router_with_options, DatabaseRepository,
};
use sqlx::PgPool;
use uuid::Uuid;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn provisioning_failure_and_concurrent_retry_preserve_one_error_and_recover() {
    let (router, pool) = setup_router_with_options(false, None).await;
    let repository = DatabaseRepository::new(pool.clone());
    let workspace = Uuid::new_v4();
    apply_workspace_event(&repository, workspace, "workspace.created", 1).await;
    apply_workspace_event(&repository, workspace, "workspace.activated", 2).await;
    create_billable_catalog(&repository, 1).await;
    FakeWalletMaterializationFailure::install(&pool).await;
    let (first, second) = tokio::join!(
        repository.reconcile_wallets(workspace, Some("test:failure")),
        repository.reconcile_wallets(workspace, Some("test:failure"))
    );
    for result in [first, second] {
        let response = result.expect("persisted failure response");
        assert_eq!(response.status.as_str(), "ERROR");
        assert_eq!(
            (
                response.expected_item_wallets,
                response.materialized_item_wallets
            ),
            (1, 0)
        );
        assert!(response.error_detail.unwrap().contains("database_error"));
    }
    let hierarchy = repository.find_wallet_hierarchy(workspace).await.unwrap();
    assert!(!hierarchy.ready);
    assert_eq!(hierarchy.customer_wallet.status.as_str(), "ERROR");
    assert!(hierarchy.item_wallets.is_empty());
    assert_failure_history(&pool, workspace).await;
    let response = super::post_json(
        &router,
        &format!("/v1/admin/workspaces/{workspace}/wallet-provisioning/reconcile"),
    )
    .await;
    assert_eq!(response["status"], "ERROR");
    let openapi = super::get_json(&router, "/openapi.json").await;
    let responses = &openapi["paths"]
        ["/v1/admin/workspaces/{workspace_id}/wallet-provisioning/reconcile"]["post"]["responses"];
    assert!(responses["200"]["description"]
        .as_str()
        .unwrap()
        .contains("ERROR"));
    assert!(responses.get("500").is_some());
    create_billable_catalog(&repository, 1).await;
    let next_scope = repository
        .reconcile_wallets(workspace, Some("test:new-scope"))
        .await
        .unwrap();
    assert_eq!(next_scope.expected_item_wallets, 2);
    let failures: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM outbox_events WHERE event_type='workspace_provisioning.failed'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(failures, 2);
    FakeWalletMaterializationFailure::remove(&pool).await;
    let (first, second) = tokio::join!(
        repository.reconcile_wallets(workspace, Some("test:recovery")),
        repository.reconcile_wallets(workspace, Some("test:recovery"))
    );
    assert_eq!(first.unwrap().status.as_str(), "ACTIVE");
    assert_eq!(second.unwrap().status.as_str(), "ACTIVE");
    assert_eq!(
        repository
            .find_wallet_hierarchy(workspace)
            .await
            .unwrap()
            .item_wallets
            .len(),
        2
    );
    let chain: Vec<String> = sqlx::query_scalar("SELECT e.new_status FROM wallet_lifecycle_events e JOIN wallets w USING(wallet_id) WHERE w.wallet_type='CUSTOMER' ORDER BY e.sequence")
        .fetch_all(&pool).await.unwrap();
    assert_eq!(chain, ["PROVISIONING", "ACTIVE", "ERROR", "ACTIVE"]);
    let events: Vec<String> = sqlx::query_scalar("SELECT event_type FROM outbox_events WHERE aggregate_type='wallet_provisioning' ORDER BY aggregate_sequence")
        .fetch_all(&pool).await.unwrap();
    assert_eq!(
        &events[4..],
        [
            "workspace_provisioning.started",
            "workspace_provisioning.failed",
            "workspace_provisioning.started",
            "workspace_provisioning.failed",
            "workspace_provisioning.started",
            "workspace_provisioning.completed"
        ]
    );
}

async fn assert_failure_history(pool: &PgPool, workspace: Uuid) {
    let failures: i64 =
        sqlx::query_scalar("SELECT count(*) FROM wallet_lifecycle_events WHERE new_status='ERROR'")
            .fetch_one(pool)
            .await
            .unwrap();
    assert_eq!(failures, 1);
    let envelope: serde_json::Value = sqlx::query_scalar("SELECT payload FROM outbox_events WHERE workspace_id=$1 AND event_type='workspace_provisioning.failed'")
        .bind(workspace).fetch_one(pool).await.unwrap();
    assert_eq!(envelope["schema_version"], 1);
    assert_eq!(envelope["payload"]["status"], "ERROR");
    assert_eq!(envelope["payload"]["materialized_item_wallets"], 0);
    let schema: serde_json::Value = serde_json::from_str(include_str!(
        "../../docs/contracts/subscription-domain-event-v1.schema.json"
    ))
    .unwrap();
    for field in schema["allOf"][0]["then"]["properties"]["payload"]["required"]
        .as_array()
        .unwrap()
    {
        assert!(envelope["payload"].get(field.as_str().unwrap()).is_some());
    }
    assert!(envelope["payload"]["error_detail"]
        .as_str()
        .unwrap()
        .contains("database_error"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn incomplete_specialization_never_marks_customer_ready() {
    let (_, pool) = setup_router_with_options(false, None).await;
    let repository = DatabaseRepository::new(pool.clone());
    let workspace = Uuid::new_v4();
    apply_workspace_event(&repository, workspace, "workspace.created", 1).await;
    apply_workspace_event(&repository, workspace, "workspace.activated", 2).await;
    create_billable_catalog(&repository, 1).await;
    FakeWalletMaterializationFailure::install(&pool).await;
    sqlx::raw_sql("CREATE OR REPLACE FUNCTION fake_wallet_failure() RETURNS trigger AS $$ BEGIN RETURN NULL; END; $$ LANGUAGE plpgsql;")
        .execute(&pool).await.unwrap();
    let response = repository
        .reconcile_wallets(workspace, Some("test:partial"))
        .await
        .unwrap();
    assert_eq!(response.status.as_str(), "ERROR");
    assert_eq!(response.materialized_item_wallets, 0);
    assert!(
        !repository
            .find_wallet_hierarchy(workspace)
            .await
            .unwrap()
            .ready
    );
    FakeWalletMaterializationFailure::remove(&pool).await;
    assert_eq!(
        repository
            .reconcile_wallets(workspace, Some("test:repair"))
            .await
            .unwrap()
            .status
            .as_str(),
        "ACTIVE"
    );
}

struct FakeWalletMaterializationFailure;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn returning_to_previously_active_scope_records_its_new_failure() {
    let (_, pool) = setup_router_with_options(false, None).await;
    let repository = DatabaseRepository::new(pool.clone());
    let workspace = Uuid::new_v4();
    create_billable_catalog(&repository, 1).await;
    apply_workspace_event(&repository, workspace, "workspace.created", 1).await;
    apply_workspace_event(&repository, workspace, "workspace.activated", 2).await;
    let additional = create_billable_catalog(&repository, 1).await;
    FakeWalletMaterializationFailure::install(&pool).await;
    assert_eq!(
        repository
            .reconcile_wallets(workspace, Some("test:expanded"))
            .await
            .unwrap()
            .status
            .as_str(),
        "ERROR"
    );
    super::deactivate_product(&repository, additional).await;
    assert_eq!(
        repository
            .reconcile_wallets(workspace, Some("test:original"))
            .await
            .unwrap()
            .status
            .as_str(),
        "ERROR"
    );
    let failures: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM outbox_events WHERE event_type='workspace_provisioning.failed'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(failures, 2);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn failed_outbox_write_rolls_back_the_entire_failure_record() {
    let (_, pool) = setup_router_with_options(false, None).await;
    let repository = DatabaseRepository::new(pool.clone());
    let workspace = Uuid::new_v4();
    apply_workspace_event(&repository, workspace, "workspace.created", 1).await;
    apply_workspace_event(&repository, workspace, "workspace.activated", 2).await;
    create_billable_catalog(&repository, 1).await;
    FakeWalletMaterializationFailure::install(&pool).await;
    sqlx::raw_sql("CREATE FUNCTION fake_failed_outbox() RETURNS trigger AS $$ BEGIN IF NEW.event_type='workspace_provisioning.failed' THEN RAISE EXCEPTION 'fake failed outbox'; END IF; RETURN NEW; END; $$ LANGUAGE plpgsql; CREATE TRIGGER fake_failed_outbox BEFORE INSERT ON outbox_events FOR EACH ROW EXECUTE FUNCTION fake_failed_outbox();")
        .execute(&pool).await.unwrap();
    let before = provisioning_snapshot(&pool).await;
    assert!(repository
        .reconcile_wallets(workspace, Some("test:atomic"))
        .await
        .is_err());
    assert_eq!(provisioning_snapshot(&pool).await, before);
}

async fn provisioning_snapshot(pool: &PgPool) -> serde_json::Value {
    sqlx::query_scalar("SELECT jsonb_build_object('wallets',(SELECT jsonb_agg(to_jsonb(w) ORDER BY wallet_id) FROM wallets w),'lifecycle',(SELECT jsonb_agg(to_jsonb(e) ORDER BY wallet_id,sequence) FROM wallet_lifecycle_events e),'provisioning',(SELECT jsonb_agg(to_jsonb(p) ORDER BY customer_id,scope_version) FROM wallet_provisioning p),'states',(SELECT jsonb_agg(to_jsonb(s) ORDER BY wallet_id) FROM wallet_effective_states s),'outbox',(SELECT jsonb_agg(to_jsonb(o) ORDER BY event_id) FROM outbox_events o))")
        .fetch_one(pool).await.unwrap()
}

impl FakeWalletMaterializationFailure {
    async fn install(pool: &PgPool) {
        sqlx::raw_sql("CREATE FUNCTION fake_wallet_failure() RETURNS trigger AS $$ BEGIN RAISE EXCEPTION 'fake item materialization failure'; END; $$ LANGUAGE plpgsql; CREATE TRIGGER fake_wallet_failure BEFORE INSERT ON item_wallets FOR EACH ROW EXECUTE FUNCTION fake_wallet_failure();")
            .execute(pool).await.unwrap();
    }

    async fn remove(pool: &PgPool) {
        sqlx::raw_sql("DROP TRIGGER fake_wallet_failure ON item_wallets; DROP FUNCTION fake_wallet_failure();")
            .execute(pool).await.unwrap();
    }
}
