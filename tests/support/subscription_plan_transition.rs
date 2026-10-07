use chrono::{DateTime, Utc};
use sqlx::PgPool;
use subscription::{
    dto::plans::{CreatePlanTransitionRequest, PlanTransitionKind},
    repositories::database::DatabaseRepository,
    services::{credits, plans},
};
use uuid::Uuid;

pub async fn assert_downgrade(
    repository: &DatabaseRepository,
    pool: &PgPool,
    account_id: Uuid,
    customer_plan_id: Uuid,
    target_plan_id: Uuid,
) {
    let request = CreatePlanTransitionRequest {
        new_plan_version_id: target_plan_id,
        transition_kind: PlanTransitionKind::Downgrade,
        payment_method_binding_id: None,
        transaction_id: "downgrade-transaction".to_string(),
        actor_reference: "customer:test".to_string(),
    };
    let changed = plans::transition_customer_plan(
        repository,
        account_id,
        customer_plan_id,
        "downgrade-key",
        request.clone(),
    )
    .await
    .expect("apply downgrade");
    assert_eq!(changed.reclassified_credit_units.value(), 60);
    assert_eq!(changed.customer_plan.plan_version_id, target_plan_id);
    assert_eq!(
        changed
            .customer_plan
            .current_cycle
            .expect("new cycle")
            .granted_credit_units
            .value(),
        0
    );
    let retry = plans::transition_customer_plan(
        repository,
        account_id,
        customer_plan_id,
        "downgrade-key",
        request,
    )
    .await;
    assert_eq!(
        retry.expect_err("duplicate transition").code(),
        "idempotency_key_already_used"
    );
    assert_reclassified_lot(pool, account_id).await;
    assert!(
        credits::reconcile(repository, account_id)
            .await
            .expect("reconcile reclassified lot")
            .consistent
    );
}

pub async fn assert_blocked_join(
    repository: &DatabaseRepository,
    account_id: Uuid,
    plan_version_id: Uuid,
) {
    let result = plans::create_customer_plan(
        repository,
        account_id,
        "blocked-plan-key",
        subscription::dto::plans::CreateCustomerPlanRequest {
            plan_version_id,
            transaction_id: "blocked-plan-transaction".to_string(),
        },
    )
    .await;
    assert_eq!(
        result.expect_err("blocked account join").code(),
        "account_not_operational"
    );
}

pub async fn revoke_for_cleanup(
    repository: &DatabaseRepository,
    account_id: Uuid,
    customer_plan_id: Uuid,
) {
    plans::revoke_customer_plan(
        repository,
        account_id,
        customer_plan_id,
        subscription::dto::plans::RevokeCustomerPlanRequest {
            reason: "test cleanup".to_string(),
            actor_reference: "test-suite".to_string(),
        },
    )
    .await
    .expect("revoke test customer plan");
}

pub async fn assert_audit(pool: &PgPool, resource_id: Uuid, action: &str, actor_reference: &str) {
    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM audit_events WHERE resource_id=$1 AND action=$2 AND actor_reference=$3",
    )
    .bind(resource_id)
    .bind(action)
    .bind(actor_reference)
    .fetch_one(pool)
    .await
    .expect("audit count");
    assert_eq!(count, 1);
}

pub async fn assert_plan_state(
    pool: &PgPool,
    account_id: Uuid,
    active_slots: i64,
    cycles: i64,
    entries: i64,
    balance: i64,
) {
    let actual: (i64, i64, i64, i64) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM active_customer_plan_slots WHERE customer_id=$1), \
         (SELECT count(*) FROM customer_plan_cycles c JOIN customer_plans p \
          ON p.customer_plan_id=c.customer_plan_id WHERE p.customer_id=$1), \
         (SELECT count(*) FROM customer_wallet_entries WHERE customer_id=$1), \
         (SELECT cw.balance_credit_units FROM customer_wallets cw JOIN wallets w \
          ON w.wallet_id=cw.wallet_id WHERE w.customer_id=$1)",
    )
    .bind(account_id)
    .fetch_one(pool)
    .await
    .expect("plan state");
    assert_eq!(actual, (active_slots, cycles, entries, balance));
}

async fn assert_reclassified_lot(pool: &PgPool, account_id: Uuid) {
    let lot: (String, Option<DateTime<Utc>>, i64) = sqlx::query_as(
        "SELECT source_kind,expires_at,(SELECT count(*) FROM credit_lot_reclassifications \
         WHERE credit_lot_id=l.credit_lot_id) FROM credit_lots l \
         WHERE customer_id=$1 AND original_credit_units=60",
    )
    .bind(account_id)
    .fetch_one(pool)
    .await
    .expect("reclassified lot");
    assert_eq!(lot, ("ON_DEMAND".to_string(), None, 1));
    assert!(sqlx::query(
        "UPDATE credit_lot_reclassifications SET actor_reference='changed' \
         WHERE credit_lot_id IN (SELECT credit_lot_id FROM credit_lots WHERE customer_id=$1)",
    )
    .bind(account_id)
    .execute(pool)
    .await
    .is_err());
}
