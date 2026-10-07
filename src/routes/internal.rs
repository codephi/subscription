use axum::{
    body::Bytes,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    Json,
};
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::{
    dto::events::{AccountEventEnvelope, AccountEventResponse},
    error::{ApiError, ApiResult},
    services::{account_events, integrations, signatures},
    state::AppState,
};

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(receive_account_event))
        .routes(routes!(replay_account_event))
        .routes(routes!(replay_outbox_event))
}

#[utoipa::path(
    post,
    path = "/v1/internal/accounts/account-events",
    tag = "Integrations",
    request_body = AccountEventEnvelope,
    responses(
        (status = 202, body = AccountEventResponse),
        (status = 401, body = crate::error::ErrorResponse, description = "Missing or invalid webhook signature"),
        (status = 409, body = crate::error::ErrorResponse, description = "Event identity reused with different content"),
        (status = 422, body = crate::error::ErrorResponse, description = "Invalid schema, sequence, or account context"),
        (status = 503, body = crate::error::ErrorResponse, description = "Accounts webhook secret is not configured")
    )
)]
async fn receive_account_event(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> ApiResult<(StatusCode, Json<AccountEventResponse>)> {
    verify_request(&state, &headers, &body)?;
    let event = serde_json::from_slice(&body).map_err(ApiError::invalid_json)?;
    let response = account_events::process_account_event(&state.database(), event).await?;
    Ok((StatusCode::ACCEPTED, Json(response)))
}

#[utoipa::path(
    post,
    path = "/v1/admin/integration-inbox/{event_id}/replay",
    tag = "Operations",
    params(("event_id" = Uuid, Path)),
    responses((status = 200, body = AccountEventResponse))
)]
async fn replay_account_event(
    State(state): State<AppState>,
    Path(event_id): Path<uuid::Uuid>,
) -> ApiResult<Json<AccountEventResponse>> {
    let response = integrations::replay_account_event(&state.database(), event_id).await?;
    Ok(Json(response))
}

#[utoipa::path(
    post,
    path = "/v1/admin/outbox-events/{event_id}/replay",
    tag = "Operations",
    params(("event_id" = Uuid, Path)),
    responses((status = 204))
)]
async fn replay_outbox_event(
    State(state): State<AppState>,
    Path(event_id): Path<uuid::Uuid>,
) -> ApiResult<StatusCode> {
    integrations::replay_outbox_event(&state.database(), event_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub(super) fn verify_request(state: &AppState, headers: &HeaderMap, body: &[u8]) -> ApiResult<()> {
    let secret = state.accounts_webhook_secret().ok_or_else(|| {
        ApiError::service_unavailable(
            "accounts_webhook_not_configured",
            "ACCOUNTS_WEBHOOK_SECRET must be configured".to_string(),
        )
    })?;
    let timestamp = required_header(headers, "x-runvibe-timestamp")?;
    let signature = required_header(headers, "x-runvibe-signature")?;
    signatures::verify_signed_body(&secret, timestamp, signature, body)
}

fn required_header<'a>(headers: &'a HeaderMap, name: &str) -> ApiResult<&'a str> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| {
            ApiError::unauthorized(
                "missing_webhook_signature",
                format!("header {name} must contain visible ASCII"),
            )
        })
}
