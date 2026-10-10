use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::{
    dto::admission::{
        AdmissionEvidenceRequest, AdmissionEvidenceResponse, AdmissionFact,
        AdmissionPolicyResponse, CreateAdmissionPolicyRequest,
    },
    error::{ApiError, ApiResult},
    repositories::database::DatabaseRepository,
};

/// Publish immutable admission requirements; e.g. policy version 2 can require verified identity.
pub async fn create_policy(
    repository: &DatabaseRepository,
    request: CreateAdmissionPolicyRequest,
) -> ApiResult<AdmissionPolicyResponse> {
    validate_facts(&request.required_facts)?;
    if request.version < 1 || request.required_facts.is_empty() {
        return Err(ApiError::unprocessable(
            "invalid_admission_policy",
            format!(
                "version {} and facts {:?} require a positive version and nonempty facts",
                request.version, request.required_facts
            ),
        ));
    }
    repository.insert_admission_policy(&request).await
}

/// Read one published version; e.g. resolve the reference before publishing a plan.
pub async fn get_policy(
    repository: &DatabaseRepository,
    id: Uuid,
) -> ApiResult<AdmissionPolicyResponse> {
    repository.find_admission_policy(id).await
}

/// Persist an Accounts attestation after transport signature verification; e.g. an empty fact set withdraws approval.
pub async fn receive_evidence(
    repository: &DatabaseRepository,
    request: AdmissionEvidenceRequest,
) -> ApiResult<AdmissionEvidenceResponse> {
    validate_facts(&request.verified_facts)?;
    validate_evidence_shape(&request)?;
    repository
        .find_admission_policy(request.policy_version_id)
        .await?;
    repository
        .insert_admission_evidence(&request, &evidence_hash(&request)?)
        .await
}

fn validate_evidence_shape(request: &AdmissionEvidenceRequest) -> ApiResult<()> {
    if request.sequence < 1
        || request.evidence_reference.is_empty()
        || request.evidence_reference.len() > 255
        || request.evidence_reference.trim() != request.evidence_reference
    {
        return Err(ApiError::unprocessable(
            "invalid_admission_evidence",
            format!(
                "sequence {} must be positive and reference {:?} must contain 1..255 trimmed bytes",
                request.sequence, request.evidence_reference
            ),
        ));
    }
    Ok(())
}

fn evidence_hash(request: &AdmissionEvidenceRequest) -> ApiResult<String> {
    Ok(format!(
        "{:x}",
        Sha256::digest(
            serde_json::to_vec(&request)
                .map_err(|error| ApiError::unexpected(error.to_string()))?
        )
    ))
}

fn validate_facts(facts: &[AdmissionFact]) -> ApiResult<()> {
    if facts.len() <= 2 && (facts.len() != 2 || facts[0] != facts[1]) {
        return Ok(());
    }
    Err(ApiError::unprocessable(
        "invalid_admission_facts",
        format!("facts {facts:?} must contain each supported fact at most once"),
    ))
}
