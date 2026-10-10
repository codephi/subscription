use uuid::Uuid;

use crate::{
    dto::billing_investigation::{
        BillingRecordPageResponse, BillingRecordQuery, BillingRecordResponse,
    },
    error::{ApiError, ApiResult},
    repositories::{billing_investigation::BillingRecordKind, database::DatabaseRepository},
};

/// Page billing evidence; e.g. `list_records(&repo, "collections", query).await`.
pub async fn list_records(
    repository: &DatabaseRepository,
    kind: &str,
    query: BillingRecordQuery,
) -> ApiResult<BillingRecordPageResponse> {
    let kind = BillingRecordKind::parse(kind)?;
    let limit = query.limit.unwrap_or(20);
    if !(1..=100).contains(&limit) {
        return Err(ApiError::unprocessable(
            "invalid_billing_page_limit",
            format!("limit {limit} must be between 1 and 100"),
        ));
    }
    repository
        .list_billing_records(kind, query, i64::from(limit))
        .await
}

/// Read one billing evidence record; e.g. `get_record(&repo, "payments", id).await`.
pub async fn get_record(
    repository: &DatabaseRepository,
    kind: &str,
    id: Uuid,
) -> ApiResult<BillingRecordResponse> {
    repository
        .get_billing_record(BillingRecordKind::parse(kind)?, id)
        .await
}

#[cfg(test)]
mod tests {
    use crate::repositories::billing_investigation::BillingRecordKind;

    #[test]
    fn only_supported_billing_kinds_are_accepted() {
        assert!(BillingRecordKind::parse("collections").is_ok());
        assert!(BillingRecordKind::parse("unexpected").is_err());
    }
}
