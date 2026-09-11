#[path = "support/credit_fixture.rs"]
mod credit_fixture;
mod support;

use axum::body::Body;
use credit_fixture::{credit_body, CreditFixture};
use serde_json::{json, Value};
use subscription::{dto::credits::UpdateWorkspaceBillingConfigRequest, services::credits};
use tower::ServiceExt;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn credit_key_reuse_rejects_equal_reordered_and_changed_payloads() {
    let fixture = CreditFixture::new().await;
    let original = credit_body("original-credit", 10);
    assert_eq!(
        fixture.post("single-use", original.clone()).await.status(),
        201
    );
    let before = fixture.snapshot().await;
    for body in duplicate_variants(original) {
        let mut request = fixture.post_request("single-use", Value::Null);
        *request.body_mut() = Body::from(body);
        let response = fixture.router.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), 409);
        let conflict = support::response_json(response).await;
        assert_eq!(conflict["error"]["code"], "idempotency_key_already_used");
        assert_eq!(
            conflict["error"]["existing_operation"]["transaction_id"],
            "original-credit"
        );
        assert_eq!(fixture.snapshot().await, before);
    }
}

fn duplicate_variants(original: Value) -> [String; 3] {
    let mut changed = original.clone();
    changed["transaction_id"] = json!("another-credit");
    changed["credit_units"] = json!("99");
    let reordered = format!(
        r#"{{ "transaction_id":"original-credit", "metadata":{}, "description":"credit recovery test", "external_reference":"external-order", "credit_units":"10" }}"#,
        original["metadata"]
    );
    [original.to_string(), reordered, changed.to_string()]
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn credit_duplicate_references_and_lookups_are_workspace_scoped() {
    let first = CreditFixture::new().await;
    let mut second = CreditFixture {
        router: first.router.clone(),
        pool: first.pool.clone(),
        repository: first.repository.clone(),
        workspace_id: uuid::Uuid::new_v4(),
    };
    second.event("workspace.created", 1).await;
    second.event("workspace.activated", 2).await;
    let a = first.grant("shared-identifier", 10).await.unwrap();
    let b = second.grant("shared-identifier", 20).await.unwrap();
    assert_ne!(a.direct_credit_id, b.direct_credit_id);
    for (fixture, resource) in [(&first, a.direct_credit_id), (&second, b.direct_credit_id)] {
        let response = fixture
            .post("shared-identifier", credit_body("shared-identifier", 10))
            .await;
        let conflict = support::response_json(response).await;
        assert_eq!(
            conflict["error"]["existing_operation"]["resource_id"],
            resource.to_string()
        );
        assert_eq!(
            conflict["error"]["existing_operation"]["workspace_id"],
            fixture.workspace_id.to_string()
        );
    }
    second.workspace_id = uuid::Uuid::new_v4();
    let uri = format!(
        "/v1/workspaces/{}/customer-wallet/transactions/shared-identifier",
        second.workspace_id
    );
    assert_eq!(second.get(&uri).await.status(), 404);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn billing_config_optimistic_version_allows_one_concurrent_update() {
    let fixture = CreditFixture::new().await;
    let request = UpdateWorkspaceBillingConfigRequest {
        direct_credit_enabled: false,
        recurring_credit_enabled: true,
        expected_version: 1,
    };
    let (first, second) = tokio::join!(
        credits::update_billing_config(&fixture.repository, fixture.workspace_id, request.clone()),
        credits::update_billing_config(&fixture.repository, fixture.workspace_id, request)
    );
    assert_eq!(usize::from(first.is_ok()) + usize::from(second.is_ok()), 1);
    assert_eq!(
        first.err().or_else(|| second.err()).unwrap().code(),
        "billing_config_version_conflict"
    );
    let current = credits::get_billing_config(&fixture.repository, fixture.workspace_id)
        .await
        .unwrap();
    assert_eq!(current.version, 2);
    let before = fixture.snapshot().await;
    assert_eq!(
        fixture
            .grant("config-blocked", 10)
            .await
            .unwrap_err()
            .code(),
        "direct_credit_disabled"
    );
    assert_eq!(fixture.snapshot().await, before);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn credit_conflict_reference_is_documented_without_changing_other_errors() {
    let fixture = CreditFixture::new().await;
    let openapi = support::response_json(fixture.get("/openapi.json").await).await;
    assert!(openapi["components"]["schemas"]["ErrorBody"]["properties"]
        .get("existing_operation")
        .is_some());
    assert!(openapi["components"]["schemas"]
        .get("ExistingOperationReference")
        .is_some());
    let response = fixture.post("invalid", credit_body("invalid", 0)).await;
    assert_eq!(response.status(), 422);
    let error = support::response_json(response).await;
    assert!(error["error"].get("existing_operation").is_none());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn incomplete_reservation_conflict_omits_uncommitted_resource_reference() {
    let fixture = CreditFixture::new().await;
    sqlx::query("INSERT INTO idempotency_records (workspace_id,idempotency_key,operation_kind,request_hash) VALUES ($1,'reserved','DIRECT_CREDIT','injected-incomplete')")
        .bind(fixture.workspace_id).execute(&fixture.pool).await.unwrap();
    let before = fixture.snapshot().await;
    let response = fixture
        .post("reserved", credit_body("uncommitted", 10))
        .await;
    assert_eq!(response.status(), 409);
    let conflict = support::response_json(response).await;
    assert_eq!(conflict["error"]["code"], "idempotency_key_already_used");
    assert!(conflict["error"].get("existing_operation").is_none());
    assert_eq!(fixture.snapshot().await, before);
}
