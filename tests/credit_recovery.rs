#[path = "support/credit_fixture.rs"]
mod credit_fixture;
mod support;

use axum::{body::Body, http::Request, Router};
use credit_fixture::{credit_body, credit_request, CreditFixture};
use serde_json::Value;
use sqlx::PgPool;
use tower::ServiceExt;

struct CreditCommitFault;

impl CreditCommitFault {
    async fn install(pool: &PgPool, terminate: bool) {
        let function = if terminate {
            "CREATE FUNCTION fail_credit_commit() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN PERFORM pg_terminate_backend(pg_backend_pid()); RETURN NEW; END $$"
        } else {
            "CREATE FUNCTION fail_credit_commit() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'injected credit commit failure'; END $$"
        };
        sqlx::raw_sql(function).execute(pool).await.unwrap();
        sqlx::raw_sql("CREATE CONSTRAINT TRIGGER reject_credit_commit AFTER INSERT ON audit_events DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION fail_credit_commit()")
            .execute(pool).await.unwrap();
    }

    async fn remove(pool: &PgPool) {
        sqlx::query("DROP TRIGGER reject_credit_commit ON audit_events")
            .execute(pool)
            .await
            .unwrap();
        sqlx::query("DROP FUNCTION fail_credit_commit()")
            .execute(pool)
            .await
            .unwrap();
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn credit_commit_failure_and_backend_loss_roll_back_every_effect() {
    let fixture = CreditFixture::new().await;
    for terminate in [false, true] {
        let before = fixture.snapshot().await;
        CreditCommitFault::install(&fixture.pool, terminate).await;
        assert!(fixture.grant("failed-at-commit", 25).await.is_err());
        assert_eq!(fixture.snapshot().await, before);
        CreditCommitFault::remove(&fixture.pool).await;
    }
    let granted = fixture.grant("failed-at-commit", 25).await.unwrap();
    assert_eq!(granted.entry.balance_after_credit_units.value(), 25);
    assert_eq!(granted.entry.sequence, 1);
}

struct LostCreditResponseTransport {
    router: Router,
}

impl LostCreditResponseTransport {
    async fn send(&self, request: Request<Body>) -> Result<(), std::io::Error> {
        // The server completes its commit, but the caller never receives status or body.
        drop(self.router.clone().oneshot(request).await.unwrap());
        Err(std::io::Error::new(
            std::io::ErrorKind::TimedOut,
            "injected response loss after server completion",
        ))
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn credit_lost_response_recovers_original_resource_from_either_conflict() {
    let fixture = CreditFixture::new().await;
    let transport = LostCreditResponseTransport {
        router: fixture.router.clone(),
    };
    let failure = transport
        .send(fixture.post_request("lost-key", credit_body("lost-transaction", 25)))
        .await
        .unwrap_err();
    assert_eq!(failure.kind(), std::io::ErrorKind::TimedOut);
    let committed = fixture.snapshot().await;
    for (key, transaction, expected) in [
        (
            "lost-key",
            "new-transaction",
            "idempotency_key_already_used",
        ),
        (
            "fresh-key",
            "lost-transaction",
            "transaction_already_exists",
        ),
    ] {
        let response = fixture.post(key, credit_body(transaction, 25)).await;
        assert_eq!(response.status(), 409);
        let conflict = support::response_json(response).await;
        assert_eq!(conflict["error"]["code"], expected);
        assert_recovery_reference(&fixture, &conflict).await;
        assert_eq!(fixture.snapshot().await, committed);
    }
}

async fn assert_recovery_reference(fixture: &CreditFixture, conflict: &Value) {
    let reference = &conflict["error"]["existing_operation"];
    assert_eq!(reference["workspace_id"], fixture.workspace_id.to_string());
    assert_eq!(reference["operation_kind"], "DIRECT_CREDIT");
    assert_eq!(reference["transaction_id"], "lost-transaction");
    let uri = format!(
        "/v1/workspaces/{}/customer-wallet/transactions/{}",
        fixture.workspace_id,
        reference["transaction_id"].as_str().unwrap()
    );
    let response = fixture.get(&uri).await;
    assert_eq!(response.status(), 200);
    let entry = support::response_json(response).await;
    assert_eq!(entry["signed_credit_units"], "25");
    assert!(entry["references"]
        .as_array()
        .unwrap()
        .iter()
        .any(|r| r["reference_kind"] == "DIRECT_CREDIT"
            && r["reference_id"] == reference["resource_id"]));
    assert!(reference.get("metadata").is_none());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn credit_outbox_is_invisible_before_commit_and_excludes_private_context() {
    let fixture = CreditFixture::new().await;
    let mut blocker = fixture.pool.begin().await.unwrap();
    sqlx::query("SELECT pg_advisory_xact_lock(417004)")
        .execute(&mut *blocker)
        .await
        .unwrap();
    install_credit_commit_barrier(&fixture.pool).await;
    let before = fixture.snapshot().await;
    let repository = fixture.repository.clone();
    let workspace_id = fixture.workspace_id;
    let worker = tokio::spawn(async move {
        subscription::services::credits::grant_direct_credit(
            &repository,
            workspace_id,
            "visible-after-commit",
            credit_request("visible-after-commit", 9),
        )
        .await
    });
    wait_for_credit_commit(&fixture.pool).await;
    assert_eq!(fixture.snapshot().await, before);
    blocker.commit().await.unwrap();
    assert_eq!(
        worker
            .await
            .unwrap()
            .unwrap()
            .entry
            .signed_credit_units
            .value(),
        9
    );
    assert_private_credit_event(&fixture).await;
}

async fn install_credit_commit_barrier(pool: &PgPool) {
    sqlx::raw_sql("CREATE FUNCTION pause_credit_commit() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN PERFORM pg_advisory_xact_lock(417004); RETURN NEW; END $$; CREATE CONSTRAINT TRIGGER pause_credit_commit AFTER INSERT ON audit_events DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION pause_credit_commit();")
        .execute(pool).await.unwrap();
}

async fn wait_for_credit_commit(pool: &PgPool) {
    tokio::time::timeout(std::time::Duration::from_secs(15), async {
        loop {
            let blocked: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE wait_event='advisory' AND query='COMMIT')").fetch_one(pool).await.unwrap();
            if blocked { return; }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    }).await.expect("credit transaction must reach deferred commit barrier");
}

async fn assert_private_credit_event(fixture: &CreditFixture) {
    let rows: Vec<(Value, uuid::Uuid, uuid::Uuid)> = sqlx::query_as("SELECT payload,event_id,correlation_id FROM outbox_events WHERE workspace_id=$1 AND event_type='credit.granted'")
        .bind(fixture.workspace_id).fetch_all(&fixture.pool).await.unwrap();
    assert_eq!(rows.len(), 1);
    let (event, event_id, correlation_id) = &rows[0];
    assert_eq!(event["event_id"], event_id.to_string());
    assert_eq!(event["correlation_id"], correlation_id.to_string());
    assert_eq!(event["workspace_id"], fixture.workspace_id.to_string());
    assert_eq!(event["schema_version"], 1);
    assert_eq!(event["payload"]["credit_units"], "9");
    assert_eq!(event["payload"].as_object().unwrap().len(), 2);
    let entry: uuid::Uuid = sqlx::query_scalar(
        "SELECT customer_wallet_entry_id FROM customer_wallet_entries WHERE customer_id=$1",
    )
    .bind(fixture.workspace_id)
    .fetch_one(&fixture.pool)
    .await
    .unwrap();
    assert_eq!(
        event["payload"]["customer_wallet_entry_id"],
        entry.to_string()
    );
    assert!(!event.to_string().contains("private_note"));
}
