use axum::{
    extract::{Path, Query, State},
    Json,
};
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::{
    dto::catalog_admin::{CatalogPageQuery, CatalogPageResponse},
    error::{ApiResult, ErrorResponse},
    services::catalog_admin,
    state::AppState,
};

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new().routes(routes!(list_entries))
}

#[utoipa::path(get, path = "/v1/admin/catalog/{kind}", tag = "Operations",
    params(("kind" = String, Path, description = "products, items, prices, subscriptions, plans, on-demand, policies"), CatalogPageQuery),
    responses((status = 200, body = CatalogPageResponse), (status = 422, body = ErrorResponse)))]
async fn list_entries(
    State(state): State<AppState>,
    Path(kind): Path<String>,
    Query(query): Query<CatalogPageQuery>,
) -> ApiResult<Json<CatalogPageResponse>> {
    Ok(Json(
        catalog_admin::list_entries(&state.database(), &kind, query).await?,
    ))
}
