use serde_json::{json, Value};
use sqlx::{Postgres, Transaction};
use uuid::Uuid;

use crate::{
    dto::{credits::DirectCreditRequest, events::DomainEventEnvelope, units::CreditUnits},
    error::{ApiError, ApiResult},
    repositories::credits::LockedWallet,
};

pub(super) async fn insert_direct_credit_row(
    transaction: &mut Transaction<'_, Postgres>,
    account_id: Uuid,
    direct_credit_id: Uuid,
    request: &DirectCreditRequest,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO direct_credits (direct_credit_id,customer_id,transaction_id,credit_units, \
         external_reference) VALUES ($1,$2,$3,$4,$5)",
    )
    .bind(direct_credit_id)
    .bind(account_id)
    .bind(&request.transaction_id)
    .bind(request.credit_units.value())
    .bind(&request.external_reference)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

pub(super) async fn insert_entry(
    transaction: &mut Transaction<'_, Postgres>,
    wallet: &LockedWallet,
    account_id: Uuid,
    entry_id: Uuid,
    balance_after: CreditUnits,
    request: &DirectCreditRequest,
) -> Result<sqlx::postgres::PgRow, sqlx::Error> {
    sqlx::query(
        "INSERT INTO customer_wallet_entries (customer_wallet_entry_id,customer_wallet_id, \
         customer_id,entry_sequence,entry_type,source_channel,signed_credit_units, \
         balance_before_credit_units,balance_after_credit_units,transaction_id,description, \
         metadata,request_id) VALUES ($1,$2,$3,$4,'DIRECT_CREDIT','direct_credit',$5,$6,$7, \
         $8,$9,$10,$11) RETURNING *",
    )
    .bind(entry_id)
    .bind(wallet.wallet_id)
    .bind(account_id)
    .bind(wallet.next_sequence)
    .bind(request.credit_units.value())
    .bind(wallet.balance.value())
    .bind(balance_after.value())
    .bind(&request.transaction_id)
    .bind(&request.description)
    .bind(request.metadata.clone().unwrap_or_else(|| json!({})))
    .bind(Uuid::new_v4())
    .fetch_one(&mut **transaction)
    .await
}

pub(super) async fn update_wallet_balance(
    transaction: &mut Transaction<'_, Postgres>,
    wallet_id: Uuid,
    balance_after: CreditUnits,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE customer_wallets SET balance_credit_units=$2,version=version+1 WHERE wallet_id=$1",
    )
    .bind(wallet_id)
    .bind(balance_after.value())
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

pub(super) async fn insert_credit_lot(
    transaction: &mut Transaction<'_, Postgres>,
    account_id: Uuid,
    lot_id: Uuid,
    entry_id: Uuid,
    units: CreditUnits,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO credit_lots (credit_lot_id,customer_id,granting_entry_id,source_kind, \
         original_credit_units,remaining_credit_units) VALUES ($1,$2,$3,'DIRECT',$4,$4)",
    )
    .bind(lot_id)
    .bind(account_id)
    .bind(entry_id)
    .bind(units.value())
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

pub(super) async fn insert_references(
    transaction: &mut Transaction<'_, Postgres>,
    entry_id: Uuid,
    direct_credit_id: Uuid,
    lot_id: Uuid,
    external_reference: Option<&str>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO wallet_transaction_references \
         (wallet_transaction_reference_id,customer_wallet_entry_id,reference_kind, \
          direct_credit_id,credit_lot_id) \
         VALUES ($1,$2,'DIRECT_CREDIT',$3,NULL),($4,$2,'CREDIT_LOT',NULL,$5)",
    )
    .bind(Uuid::new_v4())
    .bind(entry_id)
    .bind(direct_credit_id)
    .bind(Uuid::new_v4())
    .bind(lot_id)
    .execute(&mut **transaction)
    .await?;
    if let Some(reference) = external_reference {
        sqlx::query(
            "INSERT INTO wallet_transaction_references (wallet_transaction_reference_id, \
             customer_wallet_entry_id,reference_kind,external_reference) VALUES ($1,$2,'EXTERNAL',$3)",
        )
        .bind(Uuid::new_v4())
        .bind(entry_id)
        .bind(reference)
        .execute(&mut **transaction)
        .await?;
    }
    Ok(())
}

pub(super) async fn complete_reservations(
    transaction: &mut Transaction<'_, Postgres>,
    account_id: Uuid,
    idempotency_key: &str,
    transaction_id: &str,
    resource_id: Uuid,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE idempotency_records SET resource_id=$3 WHERE account_id=$1 AND idempotency_key=$2",
    )
    .bind(account_id)
    .bind(idempotency_key)
    .bind(resource_id)
    .execute(&mut **transaction)
    .await?;
    sqlx::query(
        "UPDATE transaction_reservations SET resource_id=$3 WHERE account_id=$1 AND transaction_id=$2",
    )
    .bind(account_id)
    .bind(transaction_id)
    .bind(resource_id)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

pub(super) async fn insert_credit_outbox(
    transaction: &mut Transaction<'_, Postgres>,
    account_id: Uuid,
    wallet_id: Uuid,
    sequence: i64,
    entry_id: Uuid,
    units: CreditUnits,
) -> ApiResult<()> {
    let event = credit_event(
        account_id,
        wallet_id,
        sequence,
        "credit.granted",
        json!({
            "customer_wallet_entry_id": entry_id,
            "credit_units": units.value().to_string()
        }),
    );
    persist_credit_event(transaction, event).await
}

pub(super) async fn insert_credit_expiry_outbox(
    transaction: &mut Transaction<'_, Postgres>,
    account_id: Uuid,
    wallet_id: Uuid,
    sequence: i64,
    entry_id: Uuid,
    units: i64,
) -> ApiResult<()> {
    let event = credit_event(
        account_id,
        wallet_id,
        sequence,
        "credit.expired",
        json!({"customer_wallet_entry_id":entry_id,"credit_units":units.to_string()}),
    );
    persist_credit_event(transaction, event).await
}

fn credit_event(
    account_id: Uuid,
    wallet_id: Uuid,
    sequence: i64,
    event_type: &str,
    payload: Value,
) -> DomainEventEnvelope {
    DomainEventEnvelope {
        event_id: Uuid::new_v4(),
        event_type: event_type.to_string(),
        schema_version: 1,
        aggregate_type: "customer_wallet".to_string(),
        aggregate_id: wallet_id,
        sequence,
        occurred_at: chrono::Utc::now(),
        account_id,
        correlation_id: Uuid::new_v4(),
        causation_id: None,
        payload,
    }
}

async fn persist_credit_event(
    transaction: &mut Transaction<'_, Postgres>,
    event: DomainEventEnvelope,
) -> ApiResult<()> {
    sqlx::query(
        "INSERT INTO outbox_events (event_id,event_type,aggregate_type,aggregate_id, \
         aggregate_sequence,account_id,correlation_id,causation_id,payload,occurred_at) \
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)",
    )
    .bind(event.event_id)
    .bind(&event.event_type)
    .bind(&event.aggregate_type)
    .bind(event.aggregate_id)
    .bind(event.sequence)
    .bind(event.account_id)
    .bind(event.correlation_id)
    .bind(event.causation_id)
    .bind(serde_json::to_value(&event).map_err(ApiError::serialization)?)
    .bind(event.occurred_at)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

pub(super) async fn insert_credit_audit(
    transaction: &mut Transaction<'_, Postgres>,
    account_id: Uuid,
    direct_credit_id: Uuid,
    entry_id: Uuid,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO audit_events (audit_event_id,account_id,action,resource_kind,resource_id, \
         correlation_id,details) VALUES ($1,$2,'direct_credit.created','direct_credit',$3,$4,$5)",
    )
    .bind(Uuid::new_v4())
    .bind(account_id)
    .bind(direct_credit_id)
    .bind(Uuid::new_v4())
    .bind(json!({"customer_wallet_entry_id":entry_id}))
    .execute(&mut **transaction)
    .await?;
    Ok(())
}
