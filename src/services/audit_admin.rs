use crate::{
    dto::audit_admin::{AuditPageQuery, AuditPageResponse},
    error::{ApiError, ApiResult},
    repositories::database::DatabaseRepository,
};

/// Page immutable audit evidence; e.g. `list_audit_events(&repo, query).await`.
pub async fn list_audit_events(
    repository: &DatabaseRepository,
    query: AuditPageQuery,
) -> ApiResult<AuditPageResponse> {
    let limit = query.limit.unwrap_or(20);
    if !(1..=100).contains(&limit) {
        return Err(ApiError::unprocessable(
            "invalid_audit_page_limit",
            format!("limit {limit} must be between 1 and 100"),
        ));
    }
    repository.list_audit_events(query, i64::from(limit)).await
}
