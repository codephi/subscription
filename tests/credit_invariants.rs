#[path = "support/credit_fixture.rs"]
mod credit_fixture;
mod support;

use credit_fixture::{credit_request, CreditFixture};
use subscription::{dto::credits::StatementQuery, services::credits};

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_distinct_credits_reconcile_sequence_balance_lots_and_version() {
    let fixture = CreditFixture::new().await;
    let mut workers = Vec::new();
    for units in 1..=16 {
        let repository = fixture.repository.clone();
        let account = fixture.account_id;
        workers.push(tokio::spawn(async move {
            let identifier = format!("concurrent-credit-{units}");
            credits::grant_direct_credit(
                &repository,
                account,
                &identifier,
                credit_request(&identifier, units),
            )
            .await
            .unwrap()
        }));
    }
    for worker in workers {
        worker.await.unwrap();
    }
    assert_concurrent_ledger(&fixture).await;
    assert_complete_statement(&fixture).await;
}

async fn assert_concurrent_ledger(fixture: &CreditFixture) {
    let values: (i64, i64, i64, i64) = sqlx::query_as("SELECT cw.balance_credit_units,cw.version,(SELECT count(*) FROM customer_wallet_entries WHERE customer_id=$1),(SELECT count(*) FROM credit_lots WHERE customer_id=$1) FROM customer_wallets cw JOIN wallets w USING(wallet_id) WHERE w.customer_id=$1")
        .bind(fixture.account_id).fetch_one(&fixture.pool).await.unwrap();
    assert_eq!(values, (136, 17, 16, 16));
    let reconciliation = credits::reconcile(&fixture.repository, fixture.account_id)
        .await
        .unwrap();
    assert!(reconciliation.consistent);
    assert_eq!(reconciliation.ledger_balance_credit_units.value(), 136);
    assert_eq!(reconciliation.available_lot_credit_units.value(), 136);
    let events: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM outbox_events WHERE account_id=$1 AND event_type='credit.granted'",
    )
    .bind(fixture.account_id)
    .fetch_one(&fixture.pool)
    .await
    .unwrap();
    assert_eq!(events, 16);
}

async fn assert_complete_statement(fixture: &CreditFixture) {
    let mut cursor = None;
    let mut sequences = Vec::new();
    let mut sum = 0;
    loop {
        let page = credits::statement(
            &fixture.repository,
            fixture.account_id,
            StatementQuery {
                cursor,
                limit: Some(3),
            },
        )
        .await
        .unwrap();
        for entry in page.items {
            sequences.push(entry.sequence);
            sum += entry.signed_credit_units.value();
        }
        cursor = page.next_cursor;
        if cursor.is_none() {
            break;
        }
    }
    assert_eq!(sequences, (1..=16).rev().collect::<Vec<_>>());
    assert_eq!(sum, 136);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn credit_reconciliation_detects_projection_lot_and_middle_entry_corruption() {
    let fixture = CreditFixture::new().await;
    fixture.grant("first", 10).await.unwrap();
    fixture.grant("second", 20).await.unwrap();
    sqlx::query("UPDATE customer_wallets SET balance_credit_units=29")
        .execute(&fixture.pool)
        .await
        .unwrap();
    assert_readonly_inconsistent(&fixture).await;
    sqlx::query("UPDATE customer_wallets SET balance_credit_units=30")
        .execute(&fixture.pool)
        .await
        .unwrap();
    sqlx::query("UPDATE credit_lots SET remaining_credit_units=original_credit_units-1")
        .execute(&fixture.pool)
        .await
        .unwrap();
    assert_readonly_inconsistent(&fixture).await;
    sqlx::query("UPDATE credit_lots SET remaining_credit_units=original_credit_units")
        .execute(&fixture.pool)
        .await
        .unwrap();
    corrupt_middle_entry(&fixture).await;
    let report = credits::reconcile(&fixture.repository, fixture.account_id)
        .await
        .unwrap();
    assert_eq!(report.ledger_balance_credit_units.value(), 31);
    assert_readonly_inconsistent(&fixture).await;
}

async fn corrupt_middle_entry(fixture: &CreditFixture) {
    // Simulate damaged restored history in this disposable database only.
    sqlx::raw_sql("ALTER TABLE customer_wallet_entries DISABLE TRIGGER trg_customer_wallet_entries_append_only; UPDATE customer_wallet_entries SET signed_credit_units=11,balance_after_credit_units=11 WHERE entry_sequence=1; ALTER TABLE customer_wallet_entries ENABLE TRIGGER trg_customer_wallet_entries_append_only;")
        .execute(&fixture.pool).await.unwrap();
}

async fn assert_readonly_inconsistent(fixture: &CreditFixture) {
    let before = fixture.snapshot().await;
    assert!(
        !credits::reconcile(&fixture.repository, fixture.account_id)
            .await
            .unwrap()
            .consistent
    );
    assert_eq!(fixture.snapshot().await, before);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn credit_reconciliation_checks_chain_even_when_totals_match() {
    let fixture = CreditFixture::new().await;
    fixture.grant("first", 10).await.unwrap();
    fixture.grant("second", 20).await.unwrap();
    sqlx::raw_sql("ALTER TABLE customer_wallet_entries DISABLE TRIGGER trg_customer_wallet_entries_append_only; UPDATE customer_wallet_entries SET balance_before_credit_units=1,balance_after_credit_units=11 WHERE entry_sequence=1; ALTER TABLE customer_wallet_entries ENABLE TRIGGER trg_customer_wallet_entries_append_only;")
        .execute(&fixture.pool).await.unwrap();
    let report = credits::reconcile(&fixture.repository, fixture.account_id)
        .await
        .unwrap();
    assert_eq!(report.ledger_balance_credit_units.value(), 30);
    assert_eq!(report.wallet_balance_credit_units.value(), 30);
    assert_readonly_inconsistent(&fixture).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn expired_credit_lots_are_excluded_from_available_reconciliation() {
    let fixture = CreditFixture::new().await;
    fixture.grant("expired", 10).await.unwrap();
    sqlx::query("UPDATE credit_lots SET expires_at=now()-interval '1 second'")
        .execute(&fixture.pool)
        .await
        .unwrap();
    let report = credits::reconcile(&fixture.repository, fixture.account_id)
        .await
        .unwrap();
    assert_eq!(report.available_lot_credit_units.value(), 0);
    assert_eq!(report.ledger_balance_credit_units.value(), 10);
    assert_readonly_inconsistent(&fixture).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn nonoperational_account_preserves_credit_state_and_reuses_rejected_keys() {
    let fixture = CreditFixture::new().await;
    fixture.grant("initial", 10).await.unwrap();
    fixture.event("account.blocked", 3).await;
    assert_blocked_credit_is_atomic(&fixture).await;
    fixture.event("account.activated", 4).await;
    assert_eq!(
        fixture
            .grant("blocked", 5)
            .await
            .unwrap()
            .entry
            .balance_after_credit_units
            .value(),
        15
    );
    fixture.event("account.terminated", 5).await;
    assert_blocked_credit_is_atomic(&fixture).await;
}

async fn assert_blocked_credit_is_atomic(fixture: &CreditFixture) {
    let before = fixture.snapshot().await;
    assert_eq!(
        fixture.grant("blocked", 5).await.unwrap_err().code(),
        "account_not_operational"
    );
    assert_eq!(fixture.snapshot().await, before);
}
