use chrono::{DateTime, Utc};
use serde_json::json;
use sqlx::{Postgres, Transaction};
use uuid::Uuid;

use crate::{
    dto::plans::CustomerPlanCycleResponse,
    error::{ApiError, ApiResult},
    repositories::{
        credit_writes::insert_credit_outbox,
        credits::LockedWallet,
        plan_rows::{cycle_from_row, PlanRecord},
    },
};

pub(super) async fn ensure_recurring_credit_enabled(
    transaction: &mut Transaction<'_, Postgres>,
    account_id: Uuid,
    _plan: &PlanRecord,
) -> ApiResult<()> {
    let enabled: bool = sqlx::query_scalar(
        "SELECT recurring_credit_enabled FROM account_billing_configs WHERE account_id=$1",
    )
    .bind(account_id)
    .fetch_one(&mut **transaction)
    .await?;
    if enabled {
        return Ok(());
    }
    Err(ApiError::conflict(
        "recurring_credit_disabled",
        format!("account {account_id} has subscription credits disabled"),
    ))
}

pub(super) async fn lock_valid_plan(
    transaction: &mut Transaction<'_, Postgres>,
    plan_id: Uuid,
) -> ApiResult<()> {
    let revoked_at: Option<DateTime<Utc>> = sqlx::query_scalar(
        "SELECT revoked_at FROM subscription_plan_versions WHERE plan_version_id=$1 FOR UPDATE",
    )
    .bind(plan_id)
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or_else(|| {
        ApiError::not_found(
            "commercial_resource_not_found",
            format!("subscription_plan {plan_id} does not exist"),
        )
    })?;
    if revoked_at.is_none() {
        return Ok(());
    }
    Err(ApiError::conflict(
        "subscription_plan_revoked",
        format!("subscription plan {plan_id} is revoked"),
    ))
}

pub(super) async fn insert_customer_plan_row(
    transaction: &mut Transaction<'_, Postgres>,
    customer_plan_id: Uuid,
    account_id: Uuid,
    plan_id: Uuid,
    anchor_at: DateTime<Utc>,
    commercial_model: crate::dto::plans::CommercialModel,
    activates_now: bool,
) -> Result<sqlx::postgres::PgRow, sqlx::Error> {
    let activation_status = if activates_now {
        "ACTIVATED"
    } else if commercial_model == crate::dto::plans::CommercialModel::Free {
        "PENDING_CARD_VALIDATION"
    } else {
        "PENDING_INITIAL_PAYMENT"
    };
    sqlx::query(
        "INSERT INTO customer_plans (customer_plan_id,customer_id,plan_version_id, \
         commercial_status,activation_status,renewal_status,anchor_at) \
         VALUES ($1,$2,$3,$4,$5,'CURRENT',$6) RETURNING *",
    )
    .bind(customer_plan_id)
    .bind(account_id)
    .bind(plan_id)
    .bind("ACTIVE")
    .bind(activation_status)
    .bind(anchor_at)
    .fetch_one(&mut **transaction)
    .await
}

pub(super) async fn reserve_active_slot(
    transaction: &mut Transaction<'_, Postgres>,
    account_id: Uuid,
    subscription_id: Uuid,
    customer_plan_id: Uuid,
) -> ApiResult<()> {
    let inserted = sqlx::query(
        "INSERT INTO active_customer_plan_slots (customer_id,subscription_id,customer_plan_id) \
         VALUES ($1,$2,$3) ON CONFLICT (customer_id,subscription_id) DO NOTHING",
    )
    .bind(account_id)
    .bind(subscription_id)
    .bind(customer_plan_id)
    .execute(&mut **transaction)
    .await?;
    if inserted.rows_affected() == 1 {
        return Ok(());
    }
    Err(ApiError::conflict(
        "active_customer_plan_already_exists",
        format!("account {account_id} already has a plan for subscription {subscription_id}"),
    ))
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn activate_customer_plan(
    transaction: &mut Transaction<'_, Postgres>,
    wallet: &LockedWallet,
    account_id: Uuid,
    customer_plan_id: Uuid,
    plan: &PlanRecord,
    anchor_at: DateTime<Utc>,
    period_end: Option<DateTime<Utc>>,
    transaction_id: &str,
) -> ApiResult<CustomerPlanCycleResponse> {
    let cycle_id = Uuid::new_v4();
    let cycle_row = sqlx::query(
        "INSERT INTO customer_plan_cycles (customer_plan_cycle_id,customer_plan_id,cycle_ordinal, \
         current_period_start,current_period_end,granted_credit_units,status) \
         VALUES ($1,$2,1,$3,$4,$5,'ACTIVE') RETURNING *",
    )
    .bind(cycle_id)
    .bind(customer_plan_id)
    .bind(anchor_at)
    .bind(period_end)
    .bind(plan.response.granted_credit_units.value())
    .fetch_one(&mut **transaction)
    .await?;
    insert_entitlements(
        transaction,
        customer_plan_id,
        &plan.response.product_ids,
        anchor_at,
    )
    .await?;
    if plan.response.granted_credit_units.value() > 0 {
        grant_cycle_credit(
            transaction,
            wallet,
            account_id,
            customer_plan_id,
            cycle_id,
            plan,
            period_end,
            Some(transaction_id),
        )
        .await?;
    }
    insert_plan_outbox(
        transaction,
        account_id,
        customer_plan_id,
        cycle_id,
        "customer_plan.activated",
        1,
    )
    .await?;
    Ok(cycle_from_row(&cycle_row))
}

pub(super) async fn insert_entitlements(
    transaction: &mut Transaction<'_, Postgres>,
    customer_plan_id: Uuid,
    product_ids: &[Uuid],
    effective_from: DateTime<Utc>,
) -> Result<(), sqlx::Error> {
    for product_id in product_ids {
        sqlx::query(
            "INSERT INTO customer_plan_entitlements (customer_plan_entitlement_id,customer_plan_id, \
             product_id,effective_from) VALUES ($1,$2,$3,$4)",
        )
        .bind(Uuid::new_v4())
        .bind(customer_plan_id)
        .bind(product_id)
        .bind(effective_from)
        .execute(&mut **transaction)
        .await?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn grant_cycle_credit(
    transaction: &mut Transaction<'_, Postgres>,
    wallet: &LockedWallet,
    account_id: Uuid,
    customer_plan_id: Uuid,
    cycle_id: Uuid,
    plan: &PlanRecord,
    expires_at: Option<DateTime<Utc>>,
    transaction_id: Option<&str>,
) -> ApiResult<()> {
    let balance_after = wallet
        .balance
        .checked_add(plan.response.granted_credit_units)?;
    let entry_id = Uuid::new_v4();
    let lot_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO customer_wallet_entries (customer_wallet_entry_id,customer_wallet_id,customer_id, \
         entry_sequence,entry_type,source_channel,signed_credit_units,balance_before_credit_units, \
         balance_after_credit_units,transaction_id,metadata,request_id) \
         VALUES ($1,$2,$3,$4,'SUBSCRIPTION_CREDIT','subscription_cycle',$5,$6,$7,$8,'{}',$9)",
    )
    .bind(entry_id)
    .bind(wallet.wallet_id)
    .bind(account_id)
    .bind(wallet.next_sequence)
    .bind(plan.response.granted_credit_units.value())
    .bind(wallet.balance.value())
    .bind(balance_after.value())
    .bind(transaction_id)
    .bind(Uuid::new_v4())
    .execute(&mut **transaction)
    .await?;
    sqlx::query(
        "UPDATE customer_wallets SET balance_credit_units=$2,version=version+1 WHERE wallet_id=$1",
    )
    .bind(wallet.wallet_id)
    .bind(balance_after.value())
    .execute(&mut **transaction)
    .await?;
    sqlx::query(
        "INSERT INTO credit_lots (credit_lot_id,customer_id,granting_entry_id,source_kind, \
         original_credit_units,remaining_credit_units,expires_at) VALUES ($1,$2,$3,'SUBSCRIPTION',$4,$4,$5)",
    )
    .bind(lot_id)
    .bind(account_id)
    .bind(entry_id)
    .bind(plan.response.granted_credit_units.value())
    .bind(expires_at)
    .execute(&mut **transaction)
    .await?;
    insert_plan_references(
        transaction,
        entry_id,
        lot_id,
        customer_plan_id,
        cycle_id,
        plan.response.plan_version_id,
    )
    .await?;
    insert_credit_outbox(
        transaction,
        account_id,
        wallet.wallet_id,
        wallet.next_sequence,
        entry_id,
        plan.response.granted_credit_units,
    )
    .await
}

pub(super) async fn insert_plan_references(
    transaction: &mut Transaction<'_, Postgres>,
    entry_id: Uuid,
    lot_id: Uuid,
    customer_plan_id: Uuid,
    cycle_id: Uuid,
    plan_id: Uuid,
) -> Result<(), sqlx::Error> {
    insert_reference(transaction, entry_id, "CREDIT_LOT", lot_id).await?;
    insert_reference(transaction, entry_id, "CUSTOMER_PLAN", customer_plan_id).await?;
    insert_reference(transaction, entry_id, "CUSTOMER_PLAN_CYCLE", cycle_id).await?;
    insert_reference(transaction, entry_id, "PLAN_VERSION", plan_id).await?;
    Ok(())
}

async fn insert_reference(
    transaction: &mut Transaction<'_, Postgres>,
    entry_id: Uuid,
    kind: &str,
    target: Uuid,
) -> Result<(), sqlx::Error> {
    let query = match kind {
        "CREDIT_LOT" => sqlx::query(
            "INSERT INTO wallet_transaction_references (wallet_transaction_reference_id, \
             customer_wallet_entry_id,reference_kind,credit_lot_id) VALUES ($1,$2,$3,$4)",
        ),
        "CUSTOMER_PLAN" => sqlx::query(
            "INSERT INTO wallet_transaction_references (wallet_transaction_reference_id, \
             customer_wallet_entry_id,reference_kind,customer_plan_id) VALUES ($1,$2,$3,$4)",
        ),
        "CUSTOMER_PLAN_CYCLE" => sqlx::query(
            "INSERT INTO wallet_transaction_references (wallet_transaction_reference_id, \
             customer_wallet_entry_id,reference_kind,customer_plan_cycle_id) VALUES ($1,$2,$3,$4)",
        ),
        "PLAN_VERSION" => sqlx::query(
            "INSERT INTO wallet_transaction_references (wallet_transaction_reference_id, \
             customer_wallet_entry_id,reference_kind,plan_version_id) VALUES ($1,$2,$3,$4)",
        ),
        _ => unreachable!("reference kinds are fixed by the caller"),
    };
    query
        .bind(Uuid::new_v4())
        .bind(entry_id)
        .bind(kind)
        .bind(target)
        .execute(&mut **transaction)
        .await?;
    Ok(())
}

pub(super) async fn insert_plan_outbox(
    transaction: &mut Transaction<'_, Postgres>,
    account_id: Uuid,
    aggregate_id: Uuid,
    related_id: Uuid,
    event_type: &str,
    sequence: i64,
) -> ApiResult<()> {
    let event_id = Uuid::new_v4();
    let correlation_id = Uuid::new_v4();
    let payload = json!({
        "event_id":event_id,"event_type":event_type,"schema_version":1,
        "aggregate_type":"customer_plan","aggregate_id":aggregate_id,"sequence":sequence,
        "occurred_at":Utc::now(),"account_id":account_id,"correlation_id":correlation_id,
        "causation_id":null,"payload":{"related_id":related_id}
    });
    sqlx::query(
        "INSERT INTO outbox_events (event_id,event_type,aggregate_type,aggregate_id,aggregate_sequence, \
         account_id,correlation_id,payload) VALUES ($1,$2,'customer_plan',$3,$4,$5,$6,$7)",
    )
    .bind(event_id)
    .bind(event_type)
    .bind(aggregate_id)
    .bind(sequence)
    .bind(account_id)
    .bind(correlation_id)
    .bind(payload)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}
