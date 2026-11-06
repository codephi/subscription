use sqlx::Row;
use uuid::Uuid;

use crate::{
    dto::{
        credits::{
            AccountBillingConfigResponse, CustomerWalletEntryResponse,
            WalletTransactionReferenceResponse,
        },
        units::CreditUnits,
    },
    error::ApiResult,
};

pub(super) fn entry_from_row(
    row: &sqlx::postgres::PgRow,
    references: Vec<WalletTransactionReferenceResponse>,
) -> CustomerWalletEntryResponse {
    CustomerWalletEntryResponse {
        customer_wallet_entry_id: row.get("customer_wallet_entry_id"),
        customer_wallet_id: row.get("customer_wallet_id"),
        customer_id: row.get("customer_id"),
        sequence: row.get("entry_sequence"),
        entry_type: row.get("entry_type"),
        source_channel: row.get("source_channel"),
        signed_credit_units: CreditUnits::new(row.get("signed_credit_units")),
        balance_before_credit_units: CreditUnits::new(row.get("balance_before_credit_units")),
        balance_after_credit_units: CreditUnits::new(row.get("balance_after_credit_units")),
        transaction_id: row.get("transaction_id"),
        description: row.get("description"),
        metadata: row.get("metadata"),
        request_id: row.get("request_id"),
        references,
        created_at: row.get("created_at"),
    }
}

pub(super) fn reference_from_row(
    row: &sqlx::postgres::PgRow,
) -> WalletTransactionReferenceResponse {
    let direct_credit_id: Option<Uuid> = row.get("direct_credit_id");
    let credit_lot_id: Option<Uuid> = row.get("credit_lot_id");
    let customer_plan_id: Option<Uuid> = row.get("customer_plan_id");
    let customer_plan_cycle_id: Option<Uuid> = row.get("customer_plan_cycle_id");
    let plan_version_id: Option<Uuid> = row.get("plan_version_id");
    let usage_event_id: Option<Uuid> = row.get("usage_event_id");
    let debit_id: Option<Uuid> = row.get("debit_id");
    let product_id: Option<Uuid> = row.get("product_id");
    let item_id: Option<Uuid> = row.get("item_id");
    let item_wallet_id: Option<Uuid> = row.get("item_wallet_id");
    let voucher_id: Option<Uuid> = row.try_get("voucher_id").unwrap_or(None);
    let coupon_id: Option<Uuid> = row.try_get("coupon_id").unwrap_or(None);
    WalletTransactionReferenceResponse {
        reference_kind: row.get("reference_kind"),
        reference_id: direct_credit_id
            .or(credit_lot_id)
            .or(customer_plan_id)
            .or(customer_plan_cycle_id)
            .or(plan_version_id)
            .or(usage_event_id)
            .or(debit_id)
            .or(product_id)
            .or(item_id)
            .or(item_wallet_id)
            .or(voucher_id)
            .or(coupon_id),
        external_reference: row.get("external_reference"),
    }
}

pub(super) fn billing_config_from_row(row: &sqlx::postgres::PgRow) -> AccountBillingConfigResponse {
    AccountBillingConfigResponse {
        account_id: row.get("account_id"),
        direct_credit_enabled: row.get("direct_credit_enabled"),
        recurring_credit_enabled: row.get("recurring_credit_enabled"),
        version: row.get("version"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
    }
}

pub(super) fn entry_not_found(account_id: Uuid, transaction_id: &str) -> crate::error::ApiError {
    crate::error::ApiError::not_found(
        "wallet_transaction_not_found",
        format!("transaction_id {transaction_id:?} does not exist in account {account_id}"),
    )
}

pub(super) async fn load_references(
    pool: &sqlx::PgPool,
    entry_id: Uuid,
) -> ApiResult<Vec<WalletTransactionReferenceResponse>> {
    let rows = sqlx::query(
        "SELECT reference_kind,direct_credit_id,credit_lot_id,external_reference,voucher_id,coupon_id, \
         customer_plan_id,customer_plan_cycle_id,plan_version_id,usage_event_id,debit_id, \
         product_id,item_id,item_wallet_id \
         FROM wallet_transaction_references WHERE customer_wallet_entry_id=$1 \
         ORDER BY reference_kind,wallet_transaction_reference_id",
    )
    .bind(entry_id)
    .fetch_all(pool)
    .await?;
    Ok(rows.iter().map(reference_from_row).collect())
}
