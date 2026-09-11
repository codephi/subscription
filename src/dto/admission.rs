use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AdmissionFact {
    EmailVerified,
    IdentityVerified,
}

impl AdmissionFact {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::EmailVerified => "EMAIL_VERIFIED",
            Self::IdentityVerified => "IDENTITY_VERIFIED",
        }
    }
}

#[derive(Clone, Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateAdmissionPolicyRequest {
    pub policy_id: Uuid,
    pub version: i64,
    pub required_facts: Vec<AdmissionFact>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct AdmissionPolicyResponse {
    pub policy_version_id: Uuid,
    pub policy_id: Uuid,
    pub version: i64,
    pub required_facts: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct AdmissionEvidenceRequest {
    pub event_id: Uuid,
    pub workspace_id: Uuid,
    pub policy_version_id: Uuid,
    pub sequence: i64,
    pub verified_facts: Vec<AdmissionFact>,
    pub evidence_reference: String,
    pub valid_until: DateTime<Utc>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct AdmissionEvidenceResponse {
    pub event_id: Uuid,
    pub duplicate: bool,
}
