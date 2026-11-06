use axum::{
    body::Bytes,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    Json,
};
use utoipa_axum::{router::OpenApiRouter, routes};
use uuid::Uuid;

use crate::{
    dto::admission::{
        AdmissionEvidenceRequest, AdmissionEvidenceResponse, AdmissionPolicyResponse,
        CreateAdmissionPolicyRequest,
    },
    error::{ApiError, ApiResult, ErrorResponse},
    services::admission,
    state::AppState,
};

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(publish_policy))
        .routes(routes!(get_policy))
        .routes(routes!(receive_evidence))
}

#[utoipa::path(post, path="/v1/admission-policies", tag="Subscriptions", request_body=CreateAdmissionPolicyRequest,
    responses((status=201,body=AdmissionPolicyResponse),(status=409,body=ErrorResponse),(status=422,body=ErrorResponse)))]
async fn publish_policy(
    State(state): State<AppState>,
    Json(request): Json<CreateAdmissionPolicyRequest>,
) -> ApiResult<(StatusCode, Json<AdmissionPolicyResponse>)> {
    Ok((
        StatusCode::CREATED,
        Json(admission::create_policy(&state.database(), request).await?),
    ))
}

#[utoipa::path(get, path="/v1/admission-policies/{policy_version_id}", tag="Subscriptions", params(("policy_version_id"=Uuid,Path)),
    responses((status=200,body=AdmissionPolicyResponse),(status=404,body=ErrorResponse)))]
async fn get_policy(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<AdmissionPolicyResponse>> {
    Ok(Json(admission::get_policy(&state.database(), id).await?))
}

#[utoipa::path(post, path="/v1/internal/accounts/admission-evidence", tag="Integrations", request_body=AdmissionEvidenceRequest,
    params(("x-runvibe-timestamp"=String,Header),("x-runvibe-signature"=String,Header)),
    responses((status=202,body=AdmissionEvidenceResponse),(status=401,body=ErrorResponse),(status=404,body=ErrorResponse),
        (status=409,body=ErrorResponse),(status=422,body=ErrorResponse),(status=503,body=ErrorResponse)))]
async fn receive_evidence(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> ApiResult<(StatusCode, Json<AdmissionEvidenceResponse>)> {
    super::internal::verify_request(&state, &headers, &body)?;
    let request = serde_json::from_slice(&body).map_err(ApiError::invalid_json)?;
    Ok((
        StatusCode::ACCEPTED,
        Json(admission::receive_evidence(&state.database(), request).await?),
    ))
}
