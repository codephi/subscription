use crate::{
    dto::catalog_admin::{CatalogPageQuery, CatalogPageResponse},
    error::{ApiError, ApiResult},
    repositories::database::DatabaseRepository,
};

/// Page catalog entries for administration; e.g. `list_entries(&repo, "products", query).await`.
pub async fn list_entries(
    repository: &DatabaseRepository,
    kind: &str,
    query: CatalogPageQuery,
) -> ApiResult<CatalogPageResponse> {
    if ![
        "products",
        "items",
        "prices",
        "subscriptions",
        "plans",
        "on-demand",
        "policies",
    ]
    .contains(&kind)
    {
        return Err(ApiError::unprocessable(
            "invalid_catalog_kind",
            format!(
                "kind {kind} must be products, items, prices, subscriptions, plans, on-demand, or policies"
            ),
        ));
    }
    let limit = query.limit.unwrap_or(20);
    if !(1..=100).contains(&limit) {
        return Err(ApiError::unprocessable(
            "invalid_catalog_page_limit",
            format!("limit {limit} must be between 1 and 100"),
        ));
    }
    repository
        .list_catalog_entries(kind, query, i64::from(limit))
        .await
}
