use axum::{
    extract::{Query, State},
    Json,
};
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::{
    dto::inbox_admin::{InboxPageQuery, InboxPageResponse},
    error::{ApiResult, ErrorResponse},
    services::inbox_admin,
    state::AppState,
};

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new().routes(routes!(list_inbox))
}

#[utoipa::path(get, path = "/v1/admin/integration-inbox", tag = "Operations", params(InboxPageQuery),
    responses((status = 200, body = InboxPageResponse), (status = 422, body = ErrorResponse)))]
async fn list_inbox(
    State(state): State<AppState>,
    Query(query): Query<InboxPageQuery>,
) -> ApiResult<Json<InboxPageResponse>> {
    Ok(Json(
        inbox_admin::list_inbox(&state.database(), query).await?,
    ))
}
