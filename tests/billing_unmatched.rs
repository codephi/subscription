mod support;
#[path = "support/usage_fixture.rs"]
mod usage_fixture;

use std::sync::Arc;

use chrono::Utc;
use subscription::{
    repositories::billing_confirmation::ConfirmedBillingWebhook, services::billing,
};
use uuid::Uuid;

use usage_fixture::setup_usage;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn unmatched_payment_is_opened_once_without_plan_or_credit_effects() {
    let fixture = setup_usage(1, 1, 10).await;
    let connection_id = insert_connection(&fixture.pool, fixture.account_id).await;
    let before = financial_state(&fixture.pool, fixture.account_id).await;
    let webhook = Arc::new(unmatched_webhook());
    let first = spawn_unmatched(&fixture.repository, connection_id, Arc::clone(&webhook));
    let second = spawn_unmatched(&fixture.repository, connection_id, Arc::clone(&webhook));
    let first = first.await.unwrap().unwrap();
    let second = second.await.unwrap().unwrap();

    assert_eq!(first, second);
    assert_eq!(first.reason, "COLLECTION_REQUEST_NOT_FOUND");
    assert_eq!(first.status, "OPEN");
    assert_eq!(
        financial_state(&fixture.pool, fixture.account_id).await,
        before
    );
    assert_unmatched_records(&fixture.pool, first.unmatched_payment_case_id).await;
    assert_unmatched_history_is_immutable(&fixture.pool, first.unmatched_payment_case_id).await;
}

fn spawn_unmatched(
    repository: &subscription::repositories::database::DatabaseRepository,
    connection_id: Uuid,
    webhook: Arc<ConfirmedBillingWebhook>,
) -> tokio::task::JoinHandle<
    subscription::error::ApiResult<subscription::dto::billing::UnmatchedPaymentCaseResponse>,
> {
    let repository = repository.clone();
    tokio::spawn(async move {
        billing::record_unmatched_payment(&repository, connection_id, &webhook).await
    })
}

async fn insert_connection(pool: &sqlx::PgPool, account_id: Uuid) -> Uuid {
    let connection_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO billing_connections (billing_connection_id,account_id,provider, \
         external_account_reference,secret_reference,capabilities,status) \
         VALUES ($1,$2,'FAKE',$3,$4,ARRAY['CARD','WEBHOOK'],'ACTIVE')",
    )
    .bind(connection_id)
    .bind(account_id)
    .bind(format!("account-{connection_id}"))
    .bind(format!("secret-{connection_id}"))
    .execute(pool)
    .await
    .unwrap();
    connection_id
}

fn unmatched_webhook() -> ConfirmedBillingWebhook {
    ConfirmedBillingWebhook {
        provider: "FAKE".into(),
        provider_event_id: format!("unmatched-event-{}", Uuid::new_v4()),
        event_type: "payment.confirmed".into(),
        payload_sha256: "b".repeat(64),
        collection_request_id: Uuid::new_v4(),
        provider_payment_id: format!("unmatched-payment-{}", Uuid::new_v4()),
        amount_minor: 2_500,
        currency: "BRL".into(),
        occurred_at: Utc::now(),
    }
}

async fn financial_state(pool: &sqlx::PgPool, account_id: Uuid) -> (i64, i64, i64, i64) {
    sqlx::query_as(
        "SELECT (SELECT count(*) FROM customer_plans WHERE customer_id=$1), \
         (SELECT count(*) FROM customer_plan_cycles c JOIN customer_plans p USING(customer_plan_id) \
          WHERE p.customer_id=$1), \
         (SELECT count(*) FROM customer_wallet_entries WHERE customer_id=$1), \
         (SELECT balance_credit_units FROM customer_wallets cw JOIN wallets w USING(wallet_id) \
          WHERE w.customer_id=$1 AND w.wallet_type='CUSTOMER')",
    )
    .bind(account_id)
    .fetch_one(pool)
    .await
    .unwrap()
}

async fn assert_unmatched_records(pool: &sqlx::PgPool, case_id: Uuid) {
    let counts: (i64, i64, i64, String, Option<String>) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM unmatched_payment_cases WHERE unmatched_payment_case_id=$1), \
         (SELECT count(*) FROM unmatched_payment_case_events WHERE unmatched_payment_case_id=$1), \
         (SELECT count(*) FROM outbox_events WHERE aggregate_id=$1 AND event_type='payment.unmatched'), \
         wi.result,wi.failure_code FROM billing_webhook_inbox wi JOIN unmatched_payment_cases c \
           ON c.provider=wi.provider AND c.provider_event_id=wi.provider_event_id \
         WHERE c.unmatched_payment_case_id=$1",
    )
    .bind(case_id)
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(
        counts,
        (
            1,
            1,
            1,
            "UNMATCHED".into(),
            Some("COLLECTION_REQUEST_NOT_FOUND".into())
        )
    );
}

async fn assert_unmatched_history_is_immutable(pool: &sqlx::PgPool, case_id: Uuid) {
    let case_update = sqlx::query(
        "UPDATE unmatched_payment_cases SET evidence='{}'::jsonb WHERE unmatched_payment_case_id=$1",
    )
    .bind(case_id)
    .execute(pool)
    .await;
    assert!(case_update.is_err());
    let event_delete =
        sqlx::query("DELETE FROM unmatched_payment_case_events WHERE unmatched_payment_case_id=$1")
            .bind(case_id)
            .execute(pool)
            .await;
    assert!(event_delete.is_err());
}
