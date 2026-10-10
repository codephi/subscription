use crate::{
    dto::inbox_admin::{InboxPageQuery, InboxPageResponse},
    error::{ApiError, ApiResult},
    repositories::database::DatabaseRepository,
};

/// Page integration inbox metadata; e.g. `list_inbox(&repo, query).await`.
pub async fn list_inbox(
    repository: &DatabaseRepository,
    query: InboxPageQuery,
) -> ApiResult<InboxPageResponse> {
    let limit = query.limit.unwrap_or(20);
    if !(1..=100).contains(&limit) {
        return Err(ApiError::unprocessable(
            "invalid_inbox_page_limit",
            format!("limit {limit} must be between 1 and 100"),
        ));
    }
    repository.list_inbox_events(query, i64::from(limit)).await
}
