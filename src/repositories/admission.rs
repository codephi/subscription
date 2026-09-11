use sqlx::{Postgres, Row, Transaction};
use uuid::Uuid;

use crate::{
    dto::admission::{
        AdmissionEvidenceRequest, AdmissionEvidenceResponse, AdmissionPolicyResponse,
        CreateAdmissionPolicyRequest,
    },
    error::{ApiError, ApiResult},
    repositories::database::DatabaseRepository,
};

impl DatabaseRepository {
    pub async fn insert_admission_policy(
        &self,
        request: &CreateAdmissionPolicyRequest,
    ) -> ApiResult<AdmissionPolicyResponse> {
        let facts: Vec<&str> = request
            .required_facts
            .iter()
            .map(|fact| fact.as_str())
            .collect();
        let id = Uuid::new_v4();
        let inserted = sqlx::query("INSERT INTO subscription_admission_policies(policy_version_id,policy_id,version,required_facts) VALUES ($1,$2,$3,$4) ON CONFLICT(policy_id,version) DO NOTHING")
            .bind(id).bind(request.policy_id).bind(request.version).bind(facts).execute(&self.pool()).await?;
        if inserted.rows_affected() == 0 {
            return Err(ApiError::conflict(
                "admission_policy_version_exists",
                format!(
                    "policy {} version {} already exists; expected a new version",
                    request.policy_id, request.version
                ),
            ));
        }
        self.find_admission_policy(id).await
    }

    pub async fn find_admission_policy(&self, id: Uuid) -> ApiResult<AdmissionPolicyResponse> {
        let row =
            sqlx::query("SELECT * FROM subscription_admission_policies WHERE policy_version_id=$1")
                .bind(id)
                .fetch_optional(&self.pool())
                .await?
                .ok_or_else(|| {
                    ApiError::not_found(
                        "admission_policy_not_found",
                        format!("policy version {id} must exist"),
                    )
                })?;
        Ok(AdmissionPolicyResponse {
            policy_version_id: id,
            policy_id: row.get("policy_id"),
            version: row.get("version"),
            required_facts: row.get("required_facts"),
        })
    }

    pub async fn insert_admission_evidence(
        &self,
        request: &AdmissionEvidenceRequest,
        hash: &str,
    ) -> ApiResult<AdmissionEvidenceResponse> {
        let mut transaction = self.pool().begin().await?;
        lock_evidence_workspace(&mut transaction, request.workspace_id).await?;
        if evidence_duplicate(&mut transaction, request.event_id, hash).await? {
            transaction.commit().await?;
            return Ok(AdmissionEvidenceResponse {
                event_id: request.event_id,
                duplicate: true,
            });
        }
        validate_evidence_sequence(&mut transaction, request).await?;
        let inserted = insert_evidence_row(&mut transaction, request, hash).await?;
        if inserted.rows_affected() == 0 {
            evidence_duplicate(&mut transaction, request.event_id, hash).await?;
        }
        transaction.commit().await?;
        Ok(AdmissionEvidenceResponse {
            event_id: request.event_id,
            duplicate: inserted.rows_affected() == 0,
        })
    }
}

async fn lock_evidence_workspace(
    transaction: &mut Transaction<'_, Postgres>,
    workspace_id: Uuid,
) -> ApiResult<()> {
    sqlx::query("SELECT workspace_id FROM workspace_projections WHERE workspace_id=$1 FOR UPDATE")
        .bind(workspace_id)
        .fetch_optional(&mut **transaction)
        .await?
        .ok_or_else(|| {
            ApiError::not_found(
                "workspace_not_found",
                format!("workspace {workspace_id} must exist"),
            )
        })?;
    Ok(())
}

async fn insert_evidence_row(
    transaction: &mut Transaction<'_, Postgres>,
    request: &AdmissionEvidenceRequest,
    hash: &str,
) -> Result<sqlx::postgres::PgQueryResult, sqlx::Error> {
    let facts: Vec<&str> = request
        .verified_facts
        .iter()
        .map(|fact| fact.as_str())
        .collect();
    sqlx::query("INSERT INTO subscription_admission_evidence(event_id,workspace_id,policy_version_id,sequence,verified_facts,evidence_reference,valid_until,request_hash) VALUES ($1,$2,$3,$4,$5,$6,$7,$8) ON CONFLICT(event_id) DO NOTHING")
        .bind(request.event_id).bind(request.workspace_id).bind(request.policy_version_id).bind(request.sequence)
        .bind(facts).bind(&request.evidence_reference).bind(request.valid_until).bind(hash)
        .execute(&mut **transaction).await
}

async fn evidence_duplicate(
    transaction: &mut Transaction<'_, Postgres>,
    event_id: Uuid,
    hash: &str,
) -> ApiResult<bool> {
    let previous: Option<String> = sqlx::query_scalar(
        "SELECT request_hash FROM subscription_admission_evidence WHERE event_id=$1",
    )
    .bind(event_id)
    .fetch_optional(&mut **transaction)
    .await?;
    match previous {
        None => Ok(false),
        Some(previous) if previous == hash => Ok(true),
        Some(_) => Err(ApiError::conflict(
            "admission_evidence_identity_conflict",
            format!("event {event_id} must retain its original content and workspace"),
        )),
    }
}

async fn validate_evidence_sequence(
    transaction: &mut Transaction<'_, Postgres>,
    request: &AdmissionEvidenceRequest,
) -> ApiResult<()> {
    let previous: i64 = sqlx::query_scalar("SELECT COALESCE(max(sequence),0) FROM subscription_admission_evidence WHERE workspace_id=$1 AND policy_version_id=$2")
        .bind(request.workspace_id).bind(request.policy_version_id).fetch_one(&mut **transaction).await?;
    if previous.checked_add(1) == Some(request.sequence) {
        return Ok(());
    }
    Err(ApiError::conflict(
        "admission_evidence_sequence_conflict",
        format!(
            "sequence {} must immediately follow {previous} for workspace {} and policy {}",
            request.sequence, request.workspace_id, request.policy_version_id
        ),
    ))
}

pub(super) async fn ensure_admission_evidence(
    transaction: &mut Transaction<'_, Postgres>,
    workspace_id: Uuid,
    plan_id: Uuid,
) -> ApiResult<Option<Uuid>> {
    let decision = sqlx::query(include_str!("admission_check.sql"))
        .bind(workspace_id)
        .bind(plan_id)
        .fetch_one(&mut **transaction)
        .await?;
    if decision.get::<bool, _>("allowed") {
        return Ok(decision.get("evidence_event_id"));
    }
    Err(ApiError::conflict(
        "customer_plan_approval_required",
        format!("workspace {workspace_id} requires current verified evidence for plan {plan_id}"),
    ))
}

pub(super) async fn record_admission_decision(
    transaction: &mut Transaction<'_, Postgres>,
    customer_plan_id: Uuid,
    transition_id: Option<Uuid>,
    plan_id: Uuid,
    evidence_id: Option<Uuid>,
) -> ApiResult<()> {
    let Some(evidence_id) = evidence_id else {
        return Ok(());
    };
    sqlx::query("INSERT INTO subscription_admission_decisions(decision_id,customer_plan_id,plan_transition_id,plan_version_id,evidence_event_id) VALUES ($1,$2,$3,$4,$5)")
        .bind(Uuid::new_v4()).bind(customer_plan_id).bind(transition_id).bind(plan_id).bind(evidence_id)
        .execute(&mut **transaction).await?;
    Ok(())
}
