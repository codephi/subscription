use serde_json::json;
use sqlx::{Postgres, Row, Transaction};
use uuid::Uuid;

use crate::{
    dto::events::{
        DomainEventEnvelope, WorkspaceEventEnvelope, WorkspaceEventOutcome, WorkspaceEventResponse,
        WorkspaceEventType,
    },
    error::{ApiError, ApiResult},
    repositories::{database::DatabaseRepository, wallets::provision_wallets_for_event},
};

struct WorkspaceProjection {
    status: String,
    sequence: i64,
}

impl DatabaseRepository {
    pub async fn apply_workspace_event(
        &self,
        event: &WorkspaceEventEnvelope,
    ) -> ApiResult<WorkspaceEventResponse> {
        let mut transaction = self.pool().begin().await?;
        if !insert_inbox(&mut transaction, event).await? {
            return duplicate_response(transaction, event).await;
        }
        let projection = lock_projection(&mut transaction, event.workspace_id).await?;
        let response = apply_or_quarantine(&mut transaction, event, projection).await?;
        transaction.commit().await?;
        Ok(response)
    }

    pub async fn replay_workspace_event(
        &self,
        event_id: Uuid,
    ) -> ApiResult<WorkspaceEventResponse> {
        let mut transaction = self.pool().begin().await?;
        let event = load_quarantined_event(&mut transaction, event_id).await?;
        let projection = lock_projection(&mut transaction, event.workspace_id).await?;
        let response = replay_quarantined(&mut transaction, &event, projection).await?;
        transaction.commit().await?;
        Ok(response)
    }
}

async fn load_quarantined_event(
    transaction: &mut Transaction<'_, Postgres>,
    event_id: Uuid,
) -> ApiResult<WorkspaceEventEnvelope> {
    let row = sqlx::query(
        "SELECT i.payload FROM integration_inbox i JOIN integration_inbox_quarantine q \
         ON q.event_id=i.event_id WHERE i.event_id=$1 AND q.replayed_at IS NULL FOR UPDATE OF i,q",
    )
    .bind(event_id)
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or_else(|| {
        ApiError::conflict(
            "event_not_replayable",
            format!("event_id {event_id} must identify an unreplayed quarantined event"),
        )
    })?;
    serde_json::from_value(row.get("payload")).map_err(ApiError::serialization)
}

async fn replay_quarantined(
    transaction: &mut Transaction<'_, Postgres>,
    event: &WorkspaceEventEnvelope,
    projection: Option<WorkspaceProjection>,
) -> ApiResult<WorkspaceEventResponse> {
    let current =
        projection.ok_or_else(|| replay_conflict(event, "workspace projection is absent"))?;
    if event.sequence != current.sequence + 1 {
        return Err(replay_conflict(event, "sequence gap is not resolved"));
    }
    let next = next_status(&current.status, event.event_type)
        .ok_or_else(|| replay_conflict(event, "transition remains invalid"))?;
    update_projection(transaction, event, next).await?;
    mark_quarantine_replayed(transaction, event.event_id).await?;
    finalize_applied(transaction, event, next).await
}

fn replay_conflict(event: &WorkspaceEventEnvelope, reason: &str) -> ApiError {
    ApiError::conflict(
        "event_not_replayable",
        format!(
            "event_id {} with sequence {} cannot replay: {reason}",
            event.event_id, event.sequence
        ),
    )
}

async fn mark_quarantine_replayed(
    transaction: &mut Transaction<'_, Postgres>,
    event_id: Uuid,
) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE integration_inbox_quarantine SET replayed_at=now() WHERE event_id=$1")
        .bind(event_id)
        .execute(&mut **transaction)
        .await?;
    Ok(())
}

async fn insert_inbox(
    transaction: &mut Transaction<'_, Postgres>,
    event: &WorkspaceEventEnvelope,
) -> Result<bool, sqlx::Error> {
    let inserted = sqlx::query(
        "INSERT INTO integration_inbox (event_id, workspace_id, event_type, schema_version, \
         aggregate_id, external_sequence, occurred_at, correlation_id, causation_id, payload, \
         processing_status) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,'RECEIVED') \
         ON CONFLICT (event_id) DO NOTHING",
    )
    .bind(event.event_id)
    .bind(event.workspace_id)
    .bind(event.event_type.as_str())
    .bind(i32::from(event.schema_version))
    .bind(event.aggregate_id)
    .bind(event.sequence)
    .bind(event.occurred_at)
    .bind(event.correlation_id)
    .bind(event.causation_id)
    .bind(serde_json::to_value(event).expect("workspace event must serialize"))
    .execute(&mut **transaction)
    .await?;
    Ok(inserted.rows_affected() == 1)
}

async fn duplicate_response(
    mut transaction: Transaction<'_, Postgres>,
    event: &WorkspaceEventEnvelope,
) -> ApiResult<WorkspaceEventResponse> {
    validate_duplicate_identity(&mut transaction, event).await?;
    let projection = lock_projection(&mut transaction, event.workspace_id).await?;
    transaction.commit().await?;
    Ok(response_from_projection(
        event.event_id,
        WorkspaceEventOutcome::Duplicate,
        projection,
    ))
}

async fn validate_duplicate_identity(
    transaction: &mut Transaction<'_, Postgres>,
    event: &WorkspaceEventEnvelope,
) -> ApiResult<()> {
    let identical: bool =
        sqlx::query_scalar("SELECT payload=$2 FROM integration_inbox WHERE event_id=$1")
            .bind(event.event_id)
            .bind(serde_json::to_value(event).map_err(ApiError::serialization)?)
            .fetch_one(&mut **transaction)
            .await?;
    if identical {
        return Ok(());
    }
    Err(ApiError::conflict(
        "workspace_event_identity_conflict",
        format!(
            "event_id {} must retain its original envelope and workspace",
            event.event_id
        ),
    ))
}

async fn lock_projection(
    transaction: &mut Transaction<'_, Postgres>,
    workspace_id: Uuid,
) -> Result<Option<WorkspaceProjection>, sqlx::Error> {
    let row = sqlx::query(
        "SELECT operational_status, external_sequence FROM workspace_projections \
         WHERE workspace_id = $1 FOR UPDATE",
    )
    .bind(workspace_id)
    .fetch_optional(&mut **transaction)
    .await?;
    Ok(row.map(|row| WorkspaceProjection {
        status: row.get("operational_status"),
        sequence: row.get("external_sequence"),
    }))
}

async fn apply_or_quarantine(
    transaction: &mut Transaction<'_, Postgres>,
    event: &WorkspaceEventEnvelope,
    projection: Option<WorkspaceProjection>,
) -> ApiResult<WorkspaceEventResponse> {
    if let Some(current) = projection {
        return apply_to_existing(transaction, event, current).await;
    }
    if event.sequence != 1 || !matches!(event.event_type, WorkspaceEventType::Created) {
        return quarantine(transaction, event, "workspace_not_created").await;
    }
    insert_projection(transaction, event, "CREATED").await?;
    finalize_applied(transaction, event, "CREATED").await
}

async fn apply_to_existing(
    transaction: &mut Transaction<'_, Postgres>,
    event: &WorkspaceEventEnvelope,
    current: WorkspaceProjection,
) -> ApiResult<WorkspaceEventResponse> {
    if event.sequence <= current.sequence {
        mark_inbox(transaction, event.event_id, "IGNORED").await?;
        return Ok(response_from_projection(
            event.event_id,
            WorkspaceEventOutcome::Stale,
            Some(current),
        ));
    }
    if event.sequence != current.sequence + 1 {
        return quarantine(transaction, event, "sequence_gap").await;
    }
    let Some(next_status) = next_status(&current.status, event.event_type) else {
        return quarantine(transaction, event, "invalid_transition").await;
    };
    update_projection(transaction, event, next_status).await?;
    finalize_applied(transaction, event, next_status).await
}

fn next_status(current: &str, event_type: WorkspaceEventType) -> Option<&'static str> {
    match (current, event_type) {
        ("CREATED", WorkspaceEventType::Activated) | ("BLOCKED", WorkspaceEventType::Activated) => {
            Some("ACTIVE")
        }
        ("ACTIVE", WorkspaceEventType::Blocked) => Some("BLOCKED"),
        ("CREATED" | "ACTIVE" | "BLOCKED", WorkspaceEventType::Terminated) => Some("TERMINATED"),
        _ => None,
    }
}

async fn insert_projection(
    transaction: &mut Transaction<'_, Postgres>,
    event: &WorkspaceEventEnvelope,
    status: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO workspace_projections (workspace_id, operational_status, external_sequence, \
         external_occurred_at, last_event_id) VALUES ($1,$2,$3,$4,$5)",
    )
    .bind(event.workspace_id)
    .bind(status)
    .bind(event.sequence)
    .bind(event.occurred_at)
    .bind(event.event_id)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

async fn update_projection(
    transaction: &mut Transaction<'_, Postgres>,
    event: &WorkspaceEventEnvelope,
    status: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE workspace_projections SET operational_status=$2, external_sequence=$3, \
         external_occurred_at=$4, last_event_id=$5 WHERE workspace_id=$1",
    )
    .bind(event.workspace_id)
    .bind(status)
    .bind(event.sequence)
    .bind(event.occurred_at)
    .bind(event.event_id)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

async fn finalize_applied(
    transaction: &mut Transaction<'_, Postgres>,
    event: &WorkspaceEventEnvelope,
    status: &str,
) -> ApiResult<WorkspaceEventResponse> {
    provision_wallets_for_event(transaction, event, status).await?;
    mark_inbox(transaction, event.event_id, "PROCESSED").await?;
    insert_outbox(transaction, event, status).await?;
    insert_audit(transaction, event, status).await?;
    Ok(WorkspaceEventResponse {
        event_id: event.event_id,
        outcome: WorkspaceEventOutcome::Applied,
        workspace_status: Some(status.to_string()),
        external_sequence: Some(event.sequence),
    })
}

async fn mark_inbox(
    transaction: &mut Transaction<'_, Postgres>,
    event_id: Uuid,
    status: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE integration_inbox SET processing_status=$2, processed_at=now() WHERE event_id=$1",
    )
    .bind(event_id)
    .bind(status)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

async fn quarantine(
    transaction: &mut Transaction<'_, Postgres>,
    event: &WorkspaceEventEnvelope,
    reason_code: &str,
) -> ApiResult<WorkspaceEventResponse> {
    mark_inbox(transaction, event.event_id, "QUARANTINED").await?;
    let detail = format!(
        "event {} with sequence {} cannot advance workspace {}",
        event.event_type.as_str(),
        event.sequence,
        event.workspace_id
    );
    sqlx::query(
        "INSERT INTO integration_inbox_quarantine (event_id, reason_code, reason_detail) \
         VALUES ($1,$2,$3)",
    )
    .bind(event.event_id)
    .bind(reason_code)
    .bind(detail)
    .execute(&mut **transaction)
    .await?;
    let projection = lock_projection(transaction, event.workspace_id).await?;
    Ok(response_from_projection(
        event.event_id,
        WorkspaceEventOutcome::Quarantined,
        projection,
    ))
}

async fn insert_outbox(
    transaction: &mut Transaction<'_, Postgres>,
    event: &WorkspaceEventEnvelope,
    status: &str,
) -> ApiResult<()> {
    let domain_event = build_domain_event(event, status);
    sqlx::query(
        "INSERT INTO outbox_events (event_id,event_type,aggregate_type,aggregate_id, \
         aggregate_sequence,workspace_id,correlation_id,causation_id,payload,occurred_at) \
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)",
    )
    .bind(domain_event.event_id)
    .bind(&domain_event.event_type)
    .bind(&domain_event.aggregate_type)
    .bind(domain_event.aggregate_id)
    .bind(domain_event.sequence)
    .bind(domain_event.workspace_id)
    .bind(domain_event.correlation_id)
    .bind(domain_event.causation_id)
    .bind(serde_json::to_value(&domain_event).map_err(ApiError::serialization)?)
    .bind(domain_event.occurred_at)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

fn build_domain_event(event: &WorkspaceEventEnvelope, status: &str) -> DomainEventEnvelope {
    DomainEventEnvelope {
        event_id: Uuid::new_v4(),
        event_type: "workspace.projection_updated".to_string(),
        schema_version: 1,
        aggregate_type: "workspace".to_string(),
        aggregate_id: event.workspace_id,
        sequence: event.sequence,
        occurred_at: chrono::Utc::now(),
        workspace_id: event.workspace_id,
        correlation_id: event.correlation_id,
        causation_id: Some(event.event_id),
        payload: json!({ "operational_status": status }),
    }
}

async fn insert_audit(
    transaction: &mut Transaction<'_, Postgres>,
    event: &WorkspaceEventEnvelope,
    status: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO audit_events (audit_event_id,workspace_id,action,resource_kind,resource_id, \
         correlation_id,details) VALUES ($1,$2,$3,'workspace_projection',$2,$4,$5)",
    )
    .bind(Uuid::new_v4())
    .bind(event.workspace_id)
    .bind(event.event_type.as_str())
    .bind(event.correlation_id)
    .bind(json!({ "operational_status": status, "sequence": event.sequence }))
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

fn response_from_projection(
    event_id: Uuid,
    outcome: WorkspaceEventOutcome,
    projection: Option<WorkspaceProjection>,
) -> WorkspaceEventResponse {
    WorkspaceEventResponse {
        event_id,
        outcome,
        workspace_status: projection.as_ref().map(|value| value.status.clone()),
        external_sequence: projection.map(|value| value.sequence),
    }
}
