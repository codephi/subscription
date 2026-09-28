use serde_json::json;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::{
    dto::promotions::{
        CreateCouponRequest, CreateVoucherRequest, PromotionListQuery, RedeemVoucherRequest,
        UpdatePromotionRequest,
    },
    error::{ApiError, ApiResult},
    repositories::database::DatabaseRepository,
};

pub async fn create_voucher(
    repo: &DatabaseRepository,
    mut request: CreateVoucherRequest,
) -> ApiResult<crate::dto::promotions::PromotionResponse> {
    request.code = normalize_code(&request.code)?;
    validate_common(
        &request.name,
        request.description.as_deref(),
        request.valid_from,
        request.valid_until,
        request.max_total_uses,
        request.max_uses_per_workspace,
    )?;
    if request.credit_units.value() <= 0 {
        return Err(ApiError::unprocessable(
            "invalid_credit_units",
            format!(
                "credit_units {} must be positive",
                request.credit_units.value()
            ),
        ));
    }
    let body = serde_json::to_value(request).map_err(ApiError::serialization)?;
    repo.create_promotion("VOUCHER", body, None).await
}

pub async fn create_coupon(
    repo: &DatabaseRepository,
    mut request: CreateCouponRequest,
) -> ApiResult<crate::dto::promotions::PromotionResponse> {
    request.code = normalize_code(&request.code)?;
    validate_common(
        &request.name,
        request.description.as_deref(),
        request.valid_from,
        request.valid_until,
        request.max_total_uses,
        request.max_uses_per_workspace,
    )?;
    validate_discount(
        &request.discount_kind,
        request.discount_value,
        request.currency.as_deref(),
    )?;
    if !request.applies_to_initial && !request.applies_to_on_demand {
        return Err(ApiError::unprocessable(
            "coupon_scope_required",
            "coupon must apply to initial purchase, on-demand purchase, or both",
        ));
    }
    let body = serde_json::to_value(request).map_err(ApiError::serialization)?;
    repo.create_promotion("COUPON", body, None).await
}

pub async fn list(
    repo: &DatabaseRepository,
    kind: &str,
    query: PromotionListQuery,
) -> ApiResult<crate::dto::promotions::PromotionPageResponse> {
    if let Some(status) = &query.status {
        validate_status(status)?;
    }
    if query
        .search
        .as_ref()
        .is_some_and(|search| search.len() > 100)
    {
        return Err(ApiError::unprocessable(
            "invalid_promotion_search",
            "search text must contain no more than 100 bytes",
        ));
    }
    let limit = i64::from(query.limit.unwrap_or(20));
    if !(1..=100).contains(&limit) {
        return Err(ApiError::unprocessable(
            "invalid_page_limit",
            format!("promotion page limit {limit} must be between 1 and 100"),
        ));
    }
    repo.list_promotions(kind, &query, limit).await
}

pub async fn update(
    repo: &DatabaseRepository,
    kind: &str,
    id: Uuid,
    request: UpdatePromotionRequest,
) -> ApiResult<crate::dto::promotions::PromotionResponse> {
    validate_status(request.status.as_deref().unwrap_or("ACTIVE"))?;
    validate_common(
        "valid promotion",
        None,
        request.valid_from,
        request.valid_until,
        request.max_total_uses,
        request.max_uses_per_workspace,
    )?;
    if request.expected_version < 1 {
        return Err(ApiError::unprocessable(
            "invalid_promotion_version",
            "expected_version must be positive",
        ));
    }
    if request.max_total_uses.is_some_and(|value| value <= 0)
        || request
            .max_uses_per_workspace
            .is_some_and(|value| value <= 0)
    {
        return Err(ApiError::unprocessable(
            "invalid_promotion_limit",
            "promotion usage limits must be positive or null",
        ));
    }
    if let Some(actor) = request.actor_reference.as_deref() {
        validate_text("actor_reference", actor, 255)?;
    }
    let body = json!({"status":request.status,"valid_from":request.valid_from,"valid_until":request.valid_until,
        "max_total_uses":request.max_total_uses,"max_uses_per_workspace":request.max_uses_per_workspace,"expected_version":request.expected_version});
    repo.update_promotion(kind, id, body, request.actor_reference.as_deref())
        .await
}

pub async fn redeem_voucher(
    repo: &DatabaseRepository,
    workspace_id: Uuid,
    key: &str,
    mut request: RedeemVoucherRequest,
) -> ApiResult<crate::dto::promotions::VoucherRedemptionResponse> {
    validate_text("Idempotency-Key", key, 255)?;
    validate_text("transaction_id", &request.transaction_id, 255)?;
    if request.voucher_id.is_some() == request.code.is_some() {
        return Err(ApiError::unprocessable(
            "voucher_identifier_required",
            "provide exactly one of voucher_id or code",
        ));
    }
    if let Some(code) = request.code.as_deref() {
        request.code = Some(normalize_code(code)?);
    }
    if let Some(description) = request.description.as_deref() {
        validate_text("description", description, 500)?;
    }
    let canonical = serde_json::to_vec(&request).map_err(ApiError::serialization)?;
    let hash = format!("{:x}", Sha256::digest(canonical));
    repo.redeem_voucher(workspace_id, key, &hash, &request)
        .await
}

fn normalize_code(code: &str) -> ApiResult<String> {
    let normalized = code.trim().to_ascii_uppercase();
    if normalized.len() <= 64
        && !normalized.is_empty()
        && normalized
            .bytes()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || matches!(c, b'-' | b'_'))
    {
        return Ok(normalized);
    }
    Err(ApiError::unprocessable(
        "invalid_promotion_code",
        format!(
            "promotion code {code:?} must contain 1 to 64 ASCII letters, digits, hyphens or underscores"
        ),
    ))
}

fn validate_discount(kind: &str, value: i64, currency: Option<&str>) -> ApiResult<()> {
    let valid = match kind {
        "PERCENTAGE" => (1..=10_000).contains(&value) && currency.is_none(),
        "FIXED" => {
            value > 0
                && currency.is_some_and(|code| {
                    code.len() == 3 && code.bytes().all(|b| b.is_ascii_uppercase())
                })
        }
        _ => false,
    };
    if valid {
        return Ok(());
    }
    Err(ApiError::unprocessable(
        "invalid_coupon_discount",
        format!(
            "discount {kind:?} at value {value} requires a percentage from 1 to 10000 basis points without currency, or a positive fixed amount with a three-letter uppercase currency"
        ),
    ))
}

fn validate_common(
    name: &str,
    description: Option<&str>,
    start: Option<chrono::DateTime<chrono::Utc>>,
    end: Option<chrono::DateTime<chrono::Utc>>,
    total: Option<i64>,
    per_workspace: Option<i64>,
) -> ApiResult<()> {
    validate_text("name", name, 160)?;
    if let Some(value) = description {
        validate_text("description", value, 1000)?;
    }
    if start.zip(end).is_some_and(|(from, until)| until <= from) {
        return Err(ApiError::unprocessable(
            "invalid_promotion_validity",
            "valid_until must be later than valid_from",
        ));
    }
    if total.is_some_and(|value| value <= 0) || per_workspace.is_some_and(|value| value <= 0) {
        return Err(ApiError::unprocessable(
            "invalid_promotion_limit",
            "promotion limits must be positive or null for unlimited",
        ));
    }
    Ok(())
}

fn validate_text(label: &str, value: &str, max: usize) -> ApiResult<()> {
    if !value.trim().is_empty() && value.len() <= max && value.trim() == value {
        return Ok(());
    }
    Err(ApiError::unprocessable(
        "invalid_promotion_text",
        format!("{label} {value:?} must contain 1 to {max} bytes without edge whitespace"),
    ))
}

fn validate_status(value: &str) -> ApiResult<()> {
    if matches!(value, "ACTIVE" | "DISABLED" | "ARCHIVED") {
        return Ok(());
    }
    Err(ApiError::unprocessable(
        "invalid_promotion_status",
        format!("status {value:?} must be ACTIVE, DISABLED, or ARCHIVED"),
    ))
}
