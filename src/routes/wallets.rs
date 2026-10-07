use axum::{
    extract::{Path, State},
    Json,
};
use utoipa_axum::{router::OpenApiRouter, routes};
use uuid::Uuid;

use crate::{
    dto::wallets::{WalletHierarchyResponse, WalletProvisioningResponse},
    error::{ApiResult, ErrorResponse},
    services::wallets,
    state::AppState,
};

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(get_wallets))
        .routes(routes!(get_wallet_provisioning))
        .routes(routes!(reconcile_wallet_provisioning))
}

#[utoipa::path(
    get,
    path = "/v1/accounts/{account_id}/wallets",
    tag = "Wallets",
    params(("account_id" = Uuid, Path)),
    responses(
        (status = 200, body = WalletHierarchyResponse, description = "Materialized hierarchy; ready requires all expected wallets to be active, including when returning to a previous scope"),
        (status = 503, body = ErrorResponse, description = "Wallet hierarchy is incomplete")
    )
)]
async fn get_wallets(
    State(state): State<AppState>,
    Path(account_id): Path<Uuid>,
) -> ApiResult<Json<WalletHierarchyResponse>> {
    Ok(Json(
        wallets::get_wallets(&state.database(), account_id).await?,
    ))
}

#[utoipa::path(
    get,
    path = "/v1/accounts/{account_id}/wallet-provisioning",
    tag = "Wallets",
    params(("account_id" = Uuid, Path)),
    responses(
        (status = 200, body = WalletProvisioningResponse, description = "Current readiness; a historical ACTIVE record with an incomplete hierarchy is reported as PROVISIONING without completed_at"),
        (status = 503, body = ErrorResponse, description = "Provisioning has not completed")
    )
)]
async fn get_wallet_provisioning(
    State(state): State<AppState>,
    Path(account_id): Path<Uuid>,
) -> ApiResult<Json<WalletProvisioningResponse>> {
    Ok(Json(
        wallets::get_provisioning(&state.database(), account_id).await?,
    ))
}

#[utoipa::path(
    post,
    path = "/v1/admin/accounts/{account_id}/wallet-provisioning/reconcile",
    tag = "Operations",
    params(("account_id" = Uuid, Path)),
    responses(
        (status = 200, body = WalletProvisioningResponse, description = "Reconciliation result; ERROR records a rolled-back materialization failure and its retryable operational state"),
        (status = 404, body = ErrorResponse, description = "Account does not exist"),
        (status = 500, body = ErrorResponse, description = "Failure outcome could not be persisted atomically"),
        (status = 503, body = ErrorResponse, description = "Current catalog scope is unavailable")
    )
)]
async fn reconcile_wallet_provisioning(
    State(state): State<AppState>,
    Path(account_id): Path<Uuid>,
) -> ApiResult<Json<WalletProvisioningResponse>> {
    Ok(Json(
        wallets::reconcile(&state.database(), account_id).await?,
    ))
}
