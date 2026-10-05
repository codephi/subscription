use sqlx::{Postgres, Transaction};
use uuid::Uuid;

use crate::{
    dto::units::CreditUnits,
    error::{ApiError, ApiResult},
    repositories::credits::LockedWallet,
};

pub(super) struct OnDemandCreditGrant<'a> {
    pub workspace_id: Uuid,
    pub on_demand_plan_id: Option<Uuid>,
    pub plan_version_id: Uuid,
    pub granted_credit_units: i64,
    pub quantity: i32,
    pub customer_plan_id: Uuid,
    pub transaction_id: &'a str,
}

pub(super) async fn grant_on_demand_credit(
    transaction: &mut Transaction<'_, Postgres>,
    wallet: &LockedWallet,
    grant: &OnDemandCreditGrant<'_>,
) -> ApiResult<Uuid> {
    let offer_id = grant.on_demand_plan_id.ok_or_else(|| {
        ApiError::unexpected("ON_DEMAND collection must reference an on-demand plan")
    })?;
    let unit_credits = load_credit_units(transaction, offer_id, grant.plan_version_id).await?;
    let credit_units = unit_credits
        .checked_mul(i64::from(grant.quantity))
        .ok_or_else(|| invalid_offer(offer_id, "credit quantity exceeds the supported maximum"))?;
    validate_credit_snapshot(offer_id, credit_units, grant.granted_credit_units)?;
    let balance_after = wallet.balance.checked_add(CreditUnits::new(credit_units))?;
    let entry_id = insert_credit_entry(
        transaction,
        wallet,
        grant.workspace_id,
        grant.transaction_id,
        credit_units,
        balance_after.value(),
    )
    .await?;
    update_balance(transaction, wallet.wallet_id, balance_after.value()).await?;
    let lot_id = insert_credit_lot(transaction, grant.workspace_id, entry_id, credit_units).await?;
    insert_references(
        transaction,
        entry_id,
        lot_id,
        grant.customer_plan_id,
        grant.plan_version_id,
    )
    .await?;
    crate::repositories::credit_writes::insert_credit_outbox(
        transaction,
        grant.workspace_id,
        wallet.wallet_id,
        wallet.next_sequence,
        entry_id,
        CreditUnits::new(credit_units),
    )
    .await?;
    Ok(entry_id)
}

async fn load_credit_units(
    transaction: &mut Transaction<'_, Postgres>,
    offer_id: Uuid,
    plan_version_id: Uuid,
) -> ApiResult<i64> {
    sqlx::query_scalar(
        "SELECT od.credit_units FROM on_demand_plans od JOIN subscription_plan_versions sp \
         ON sp.plan_version_id=$2 AND sp.subscription_id=od.subscription_id \
         WHERE od.on_demand_plan_id=$1 AND od.revoked_at IS NULL AND sp.revoked_at IS NULL",
    )
    .bind(offer_id)
    .bind(plan_version_id)
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or_else(|| invalid_offer(offer_id, "offer must be published and not revoked"))
}

fn validate_credit_snapshot(offer_id: Uuid, actual: i64, expected: i64) -> ApiResult<()> {
    if actual == expected {
        return Ok(());
    }
    Err(invalid_offer(
        offer_id,
        "credit snapshot must match the published offer",
    ))
}

async fn insert_credit_entry(
    transaction: &mut Transaction<'_, Postgres>,
    wallet: &LockedWallet,
    workspace_id: Uuid,
    transaction_id: &str,
    credit_units: i64,
    balance_after: i64,
) -> ApiResult<Uuid> {
    let entry_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO customer_wallet_entries (customer_wallet_entry_id,customer_wallet_id,customer_id, \
         entry_sequence,entry_type,source_channel,signed_credit_units,balance_before_credit_units, \
         balance_after_credit_units,transaction_id,metadata,request_id) \
         VALUES ($1,$2,$3,$4,'ON_DEMAND_CREDIT','on_demand',$5,$6,$7,$8,'{}',$9)",
    )
    .bind(entry_id)
    .bind(wallet.wallet_id)
    .bind(workspace_id)
    .bind(wallet.next_sequence)
    .bind(credit_units)
    .bind(wallet.balance.value())
    .bind(balance_after)
    .bind(transaction_id)
    .bind(Uuid::new_v4())
    .execute(&mut **transaction)
    .await?;
    Ok(entry_id)
}

async fn update_balance(
    transaction: &mut Transaction<'_, Postgres>,
    wallet_id: Uuid,
    balance_after: i64,
) -> ApiResult<()> {
    sqlx::query(
        "UPDATE customer_wallets SET balance_credit_units=$2,version=version+1 WHERE wallet_id=$1",
    )
    .bind(wallet_id)
    .bind(balance_after)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

async fn insert_credit_lot(
    transaction: &mut Transaction<'_, Postgres>,
    workspace_id: Uuid,
    entry_id: Uuid,
    credit_units: i64,
) -> ApiResult<Uuid> {
    let lot_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO credit_lots (credit_lot_id,customer_id,granting_entry_id,source_kind, \
         original_credit_units,remaining_credit_units) VALUES ($1,$2,$3,'ON_DEMAND',$4,$4)",
    )
    .bind(lot_id)
    .bind(workspace_id)
    .bind(entry_id)
    .bind(credit_units)
    .execute(&mut **transaction)
    .await?;
    Ok(lot_id)
}

async fn insert_references(
    transaction: &mut Transaction<'_, Postgres>,
    entry_id: Uuid,
    lot_id: Uuid,
    customer_plan_id: Uuid,
    plan_version_id: Uuid,
) -> ApiResult<()> {
    sqlx::query(
        "INSERT INTO wallet_transaction_references \
         (wallet_transaction_reference_id,customer_wallet_entry_id,reference_kind,credit_lot_id) \
         VALUES ($1,$2,'CREDIT_LOT',$3)",
    )
    .bind(Uuid::new_v4())
    .bind(entry_id)
    .bind(lot_id)
    .execute(&mut **transaction)
    .await?;
    sqlx::query(
        "INSERT INTO wallet_transaction_references \
         (wallet_transaction_reference_id,customer_wallet_entry_id,reference_kind,customer_plan_id) \
         VALUES ($1,$2,'CUSTOMER_PLAN',$3)",
    )
    .bind(Uuid::new_v4())
    .bind(entry_id)
    .bind(customer_plan_id)
    .execute(&mut **transaction)
    .await?;
    sqlx::query(
        "INSERT INTO wallet_transaction_references \
         (wallet_transaction_reference_id,customer_wallet_entry_id,reference_kind,plan_version_id) \
         VALUES ($1,$2,'PLAN_VERSION',$3)",
    )
    .bind(Uuid::new_v4())
    .bind(entry_id)
    .bind(plan_version_id)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

fn invalid_offer(offer_id: Uuid, expected: &str) -> ApiError {
    ApiError::conflict(
        "billing_confirmation_mismatch",
        format!("on-demand offer {offer_id} is invalid: {expected}"),
    )
}
