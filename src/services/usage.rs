use serde::Serialize;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::{
    dto::usage::{
        CreateUsageEventRequest, ItemStatementQuery, ItemWalletEntryResponse,
        ItemWalletMeterResponse, ItemWalletStatementResponse, PricingAccumulatorResponse,
        ProductEligibilityResponse, UsageEventResponse, UsageReconciliationResponse,
    },
    error::{ApiError, ApiResult},
    repositories::database::DatabaseRepository,
};

pub async fn record_usage(
    repository: &DatabaseRepository,
    account_id: Uuid,
    idempotency_key: &str,
    request: CreateUsageEventRequest,
) -> ApiResult<UsageEventResponse> {
    validate_identifier("Idempotency-Key", idempotency_key)?;
    validate_identifier("transaction_id", &request.transaction_id)?;
    if request.item_units.value() <= 0 {
        return Err(ApiError::unprocessable(
            "invalid_item_units",
            format!("item_units {} must be positive", request.item_units.value()),
        ));
    }
    if !request
        .metadata
        .as_ref()
        .is_none_or(|value| value.is_object())
    {
        return Err(ApiError::unprocessable(
            "invalid_usage_metadata",
            "usage metadata must be a JSON object",
        ));
    }
    repository
        .insert_usage_event(
            account_id,
            idempotency_key,
            &request_hash(&request)?,
            &request,
        )
        .await
}

pub async fn eligibility(
    repository: &DatabaseRepository,
    account_id: Uuid,
    product_id: Uuid,
) -> ApiResult<ProductEligibilityResponse> {
    repository
        .find_product_eligibility(account_id, product_id)
        .await
}

pub async fn item_meter(
    repository: &DatabaseRepository,
    account_id: Uuid,
    item_id: Uuid,
) -> ApiResult<ItemWalletMeterResponse> {
    repository.find_item_wallet_meter(account_id, item_id).await
}

pub async fn item_statement(
    repository: &DatabaseRepository,
    account_id: Uuid,
    item_id: Uuid,
    query: ItemStatementQuery,
) -> ApiResult<ItemWalletStatementResponse> {
    let cursor = query.cursor.as_deref().map(parse_cursor).transpose()?;
    let limit = validate_limit(query.limit)?;
    repository
        .list_item_wallet_statement(account_id, item_id, cursor, limit)
        .await
}

pub async fn item_statement_entry(
    repository: &DatabaseRepository,
    account_id: Uuid,
    item_id: Uuid,
    entry_id: Uuid,
) -> ApiResult<ItemWalletEntryResponse> {
    repository
        .find_item_wallet_entry(account_id, item_id, entry_id)
        .await
}

pub async fn pricing_accumulators(
    repository: &DatabaseRepository,
    account_id: Uuid,
    item_id: Uuid,
    price_id: Option<Uuid>,
) -> ApiResult<Vec<PricingAccumulatorResponse>> {
    repository
        .list_pricing_accumulators(account_id, item_id, price_id)
        .await
}

pub async fn reconcile_item(
    repository: &DatabaseRepository,
    account_id: Uuid,
    item_id: Uuid,
) -> ApiResult<UsageReconciliationResponse> {
    repository.reconcile_item_usage(account_id, item_id).await
}

fn validate_identifier(label: &str, value: &str) -> ApiResult<()> {
    if (1..=255).contains(&value.len()) && value.trim() == value {
        return Ok(());
    }
    Err(ApiError::unprocessable(
        "invalid_identifier",
        format!("{label} {value:?} must contain 1 to 255 characters without edge whitespace"),
    ))
}

fn parse_cursor(value: &str) -> ApiResult<i64> {
    value
        .parse::<i64>()
        .ok()
        .filter(|cursor| *cursor > 0)
        .ok_or_else(|| {
            ApiError::unprocessable(
                "invalid_cursor",
                format!("item statement cursor {value:?} must be a positive decimal integer"),
            )
        })
}

fn validate_limit(limit: Option<u16>) -> ApiResult<i64> {
    let limit = i64::from(limit.unwrap_or(50));
    if (1..=100).contains(&limit) {
        return Ok(limit);
    }
    Err(ApiError::unprocessable(
        "invalid_page_limit",
        format!("item statement limit {limit} must be between 1 and 100"),
    ))
}

fn request_hash<T: Serialize>(request: &T) -> ApiResult<String> {
    let mut digest = Sha256::new();
    digest.update(serde_json::to_vec(request).map_err(ApiError::serialization)?);
    Ok(format!("{:#x}", digest.finalize()))
}
