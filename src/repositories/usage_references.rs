use sqlx::{Postgres, Transaction};
use uuid::Uuid;

use crate::{
    dto::usage::CreateUsageEventRequest,
    repositories::usage_models::{DebitResult, LockedMeter},
};

pub(super) async fn insert_usage_references(
    transaction: &mut Transaction<'_, Postgres>,
    entry_id: Uuid,
    usage_id: Uuid,
    request: &CreateUsageEventRequest,
    meter: &LockedMeter,
    debit: &DebitResult,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO wallet_transaction_references \
         (wallet_transaction_reference_id,customer_wallet_entry_id,reference_kind, \
          usage_event_id,debit_id,product_id,item_id,item_wallet_id) VALUES \
         ($1,$6,'USAGE_EVENT',$7,NULL,NULL,NULL,NULL), \
         ($2,$6,'DEBIT',NULL,$8,NULL,NULL,NULL), \
         ($3,$6,'PRODUCT',NULL,NULL,$9,NULL,NULL), \
         ($4,$6,'ITEM',NULL,NULL,NULL,$10,NULL), \
         ($5,$6,'ITEM_WALLET',NULL,NULL,NULL,NULL,$11)",
    )
    .bind(Uuid::new_v4())
    .bind(Uuid::new_v4())
    .bind(Uuid::new_v4())
    .bind(Uuid::new_v4())
    .bind(Uuid::new_v4())
    .bind(entry_id)
    .bind(usage_id)
    .bind(debit.debit_id)
    .bind(request.product_id)
    .bind(request.item_id)
    .bind(meter.wallet_id)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}
