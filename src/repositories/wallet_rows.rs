use sqlx::{Postgres, Row, Transaction};
use uuid::Uuid;

use crate::{
    dto::{
        units::CreditUnits,
        wallets::{
            CustomerWalletResponse, ItemWalletResponse, WalletProvisioningResponse, WalletStatus,
        },
    },
    error::{ApiError, ApiResult},
};

pub(super) async fn finish_provisioning(
    transaction: &mut Transaction<'_, Postgres>,
    workspace_id: Uuid,
    scope_version: Uuid,
    target: WalletStatus,
    expected: i64,
    materialized: i64,
) -> ApiResult<WalletProvisioningResponse> {
    let status = if expected == materialized {
        target
    } else {
        WalletStatus::Error
    };
    let row = sqlx::query(
        "UPDATE wallet_provisioning SET status=$3,materialized_item_wallets=$4, \
         completed_at=CASE WHEN $3<>'PROVISIONING' THEN now() ELSE completed_at END, \
         error_detail=CASE WHEN $3='ERROR' THEN 'materialized wallet count differs from scope' ELSE NULL END \
         WHERE customer_id=$1 AND scope_version=$2 RETURNING *",
    )
    .bind(workspace_id)
    .bind(scope_version)
    .bind(status.as_str())
    .bind(materialized)
    .fetch_one(&mut **transaction)
    .await?;
    provisioning_from_row(&row)
}

pub(super) fn target_status(operational_status: &str) -> ApiResult<WalletStatus> {
    match operational_status {
        "CREATED" => Ok(WalletStatus::Provisioning),
        "ACTIVE" => Ok(WalletStatus::Active),
        "BLOCKED" | "TERMINATED" => Ok(WalletStatus::Disabled),
        _ => Err(ApiError::unexpected(format!(
            "workspace operational status {operational_status:?} is unknown"
        ))),
    }
}

pub(super) fn provisioning_from_row(
    row: &sqlx::postgres::PgRow,
) -> ApiResult<WalletProvisioningResponse> {
    Ok(WalletProvisioningResponse {
        workspace_id: row.get("customer_id"),
        scope_version: row.get("scope_version"),
        status: parse_status(row.get("status"))?,
        expected_item_wallets: row.get("expected_item_wallets"),
        materialized_item_wallets: row.get("materialized_item_wallets"),
        error_detail: row.get("error_detail"),
        started_at: row.get("started_at"),
        completed_at: row.get("completed_at"),
        updated_at: row.get("updated_at"),
    })
}

pub(super) fn customer_wallet_from_row(
    row: &sqlx::postgres::PgRow,
) -> ApiResult<CustomerWalletResponse> {
    Ok(CustomerWalletResponse {
        wallet_id: row.get("wallet_id"),
        customer_id: row.get("customer_id"),
        balance_credit_units: CreditUnits::new(row.get("balance_credit_units")),
        status: parse_status(row.get("status"))?,
        version: row.get("version"),
        provisioning_scope_version: row.get("provisioning_scope_version"),
        created_at: row.get("created_at"),
    })
}

pub(super) fn item_wallet_from_row(row: &sqlx::postgres::PgRow) -> ApiResult<ItemWalletResponse> {
    Ok(ItemWalletResponse {
        wallet_id: row.get("wallet_id"),
        customer_id: row.get("customer_id"),
        parent_customer_wallet_id: row.get("parent_customer_wallet_id"),
        item_id: row.get("item_id"),
        status: parse_status(row.get("status"))?,
        provisioning_scope_version: row.get("provisioning_scope_version"),
        total_received_item_units: row.get::<i64, _>("total_received_item_units").to_string(),
        total_converted_item_units: row.get::<i64, _>("total_converted_item_units").to_string(),
        pending_item_units: row.get::<i64, _>("pending_item_units").to_string(),
        version: row.get("version"),
        created_at: row.get("created_at"),
    })
}

fn parse_status(value: &str) -> ApiResult<WalletStatus> {
    match value {
        "PROVISIONING" => Ok(WalletStatus::Provisioning),
        "ACTIVE" => Ok(WalletStatus::Active),
        "DISABLED" => Ok(WalletStatus::Disabled),
        "ERROR" => Ok(WalletStatus::Error),
        _ => Err(ApiError::unexpected(format!(
            "wallet status {value:?} is unknown"
        ))),
    }
}

pub(super) fn wallet_not_provisioned(workspace_id: Uuid) -> ApiError {
    ApiError::service_unavailable(
        "wallet_not_provisioned",
        format!("workspace {workspace_id} does not have a complete wallet hierarchy"),
    )
}
