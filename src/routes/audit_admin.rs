use axum::{
    extract::{Query, State},
    Json,
};
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::{
    dto::audit_admin::{AuditPageQuery, AuditPageResponse},
    error::{ApiResult, ErrorResponse},
    services::audit_admin,
    state::AppState,
};

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new().routes(routes!(list_audit_events))
}

#[utoipa::path(get, path = "/v1/admin/audit-events", tag = "Operations", params(AuditPageQuery),
    responses((status = 200, body = AuditPageResponse), (status = 422, body = ErrorResponse)))]
async fn list_audit_events(
    State(state): State<AppState>,
    Query(query): Query<AuditPageQuery>,
) -> ApiResult<Json<AuditPageResponse>> {
    Ok(Json(
        audit_admin::list_audit_events(&state.database(), query).await?,
    ))
}
