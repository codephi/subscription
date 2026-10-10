use serde_json::json;
use sqlx::{Postgres, Row, Transaction};
use uuid::Uuid;

use crate::{
    dto::events::{
        AccountEventEnvelope, AccountEventOutcome, AccountEventResponse, AccountEventType,
        DomainEventEnvelope,
    },
    error::{ApiError, ApiResult},
    repositories::{database::DatabaseRepository, wallets::provision_wallets_for_event},
};

struct AccountProjection {
    status: String,
    sequence: i64,
}

impl DatabaseRepository {
    /// Terminate a account administratively while preserving its event history.
    pub async fn terminate_account(&self, account_id: Uuid) -> ApiResult<AccountEventResponse> {
        let mut transaction = self.pool().begin().await?;
        let current = lock_projection(&mut transaction, account_id)
            .await?
            .ok_or_else(|| {
                ApiError::not_found(
                    "account_not_found",
                    format!("account {account_id} does not exist"),
                )
            })?;
        if current.status == "TERMINATED" {
            return Err(ApiError::conflict(
                "account_already_terminated",
                format!("account {account_id} is already TERMINATED"),
            ));
        }
        let event = termination_event(account_id, current.sequence + 1);
        insert_inbox(&mut transaction, &event).await?;
        let response = apply_to_existing(&mut transaction, &event, current).await?;
        transaction.commit().await?;
        Ok(response)
    }

    pub async fn apply_account_event(
        &self,
        event: &AccountEventEnvelope,
    ) -> ApiResult<AccountEventResponse> {
        let mut transaction = self.pool().begin().await?;
        if !insert_inbox(&mut transaction, event).await? {
            return duplicate_response(transaction, event).await;
        }
        let projection = lock_projection(&mut transaction, event.account_id).await?;
        let response = apply_or_quarantine(&mut transaction, event, projection).await?;
        transaction.commit().await?;
        Ok(response)
    }

    pub async fn replay_account_event(&self, event_id: Uuid) -> ApiResult<AccountEventResponse> {
        let mut transaction = self.pool().begin().await?;
        let event = load_quarantined_event(&mut transaction, event_id).await?;
        let projection = lock_projection(&mut transaction, event.account_id).await?;
        let response = replay_quarantined(&mut transaction, &event, projection).await?;
        transaction.commit().await?;
        Ok(response)
    }
}

fn termination_event(account_id: Uuid, sequence: i64) -> AccountEventEnvelope {
    let event_id = Uuid::new_v4();
    AccountEventEnvelope {
        event_id,
        event_type: AccountEventType::Terminated,
        schema_version: 1,
        aggregate_id: account_id,
        sequence,
        occurred_at: chrono::Utc::now(),
        account_id,
        correlation_id: Uuid::new_v4(),
        causation_id: None,
        payload: crate::dto::events::AccountEventPayload { account_id },
    }
}

async fn load_quarantined_event(
    transaction: &mut Transaction<'_, Postgres>,
    event_id: Uuid,
) -> ApiResult<AccountEventEnvelope> {
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
    event: &AccountEventEnvelope,
    projection: Option<AccountProjection>,
) -> ApiResult<AccountEventResponse> {
    let current =
        projection.ok_or_else(|| replay_conflict(event, "account projection is absent"))?;
    if event.sequence != current.sequence + 1 {
        return Err(replay_conflict(event, "sequence gap is not resolved"));
    }
    let next = next_status(&current.status, event.event_type)
        .ok_or_else(|| replay_conflict(event, "transition remains invalid"))?;
    update_projection(transaction, event, next).await?;
    mark_quarantine_replayed(transaction, event.event_id).await?;
    finalize_applied(transaction, event, next).await
}

fn replay_conflict(event: &AccountEventEnvelope, reason: &str) -> ApiError {
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
    event: &AccountEventEnvelope,
) -> Result<bool, sqlx::Error> {
    let inserted = sqlx::query(
        "INSERT INTO integration_inbox (event_id, account_id, event_type, schema_version, \
         aggregate_id, external_sequence, occurred_at, correlation_id, causation_id, payload, \
         processing_status) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,'RECEIVED') \
         ON CONFLICT (event_id) DO NOTHING",
    )
    .bind(event.event_id)
    .bind(event.account_id)
    .bind(event.event_type.as_str())
    .bind(i32::from(event.schema_version))
    .bind(event.aggregate_id)
    .bind(event.sequence)
    .bind(event.occurred_at)
    .bind(event.correlation_id)
    .bind(event.causation_id)
    .bind(serde_json::to_value(event).expect("account event must serialize"))
    .execute(&mut **transaction)
    .await?;
    Ok(inserted.rows_affected() == 1)
}

async fn duplicate_response(
    mut transaction: Transaction<'_, Postgres>,
    event: &AccountEventEnvelope,
) -> ApiResult<AccountEventResponse> {
    validate_duplicate_identity(&mut transaction, event).await?;
    let projection = lock_projection(&mut transaction, event.account_id).await?;
    transaction.commit().await?;
    Ok(response_from_projection(
        event.event_id,
        AccountEventOutcome::Duplicate,
        projection,
    ))
}

async fn validate_duplicate_identity(
    transaction: &mut Transaction<'_, Postgres>,
    event: &AccountEventEnvelope,
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
        "account_event_identity_conflict",
        format!(
            "event_id {} must retain its original envelope and account",
            event.event_id
        ),
    ))
}

async fn lock_projection(
    transaction: &mut Transaction<'_, Postgres>,
    account_id: Uuid,
) -> Result<Option<AccountProjection>, sqlx::Error> {
    let row = sqlx::query(
        "SELECT operational_status, external_sequence FROM account_projections \
         WHERE account_id = $1 FOR UPDATE",
    )
    .bind(account_id)
    .fetch_optional(&mut **transaction)
    .await?;
    Ok(row.map(|row| AccountProjection {
        status: row.get("operational_status"),
        sequence: row.get("external_sequence"),
    }))
}

async fn apply_or_quarantine(
    transaction: &mut Transaction<'_, Postgres>,
    event: &AccountEventEnvelope,
    projection: Option<AccountProjection>,
) -> ApiResult<AccountEventResponse> {
    if let Some(current) = projection {
        return apply_to_existing(transaction, event, current).await;
    }
    if event.sequence != 1 || !matches!(event.event_type, AccountEventType::Created) {
        return quarantine(transaction, event, "account_not_created").await;
    }
    insert_projection(transaction, event, "CREATED").await?;
    finalize_applied(transaction, event, "CREATED").await
}

async fn apply_to_existing(
    transaction: &mut Transaction<'_, Postgres>,
    event: &AccountEventEnvelope,
    current: AccountProjection,
) -> ApiResult<AccountEventResponse> {
    if event.sequence <= current.sequence {
        mark_inbox(transaction, event.event_id, "IGNORED").await?;
        return Ok(response_from_projection(
            event.event_id,
            AccountEventOutcome::Stale,
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

fn next_status(current: &str, event_type: AccountEventType) -> Option<&'static str> {
    match (current, event_type) {
        ("CREATED", AccountEventType::Activated) | ("BLOCKED", AccountEventType::Activated) => {
            Some("ACTIVE")
        }
        ("ACTIVE", AccountEventType::Blocked) => Some("BLOCKED"),
        ("CREATED" | "ACTIVE" | "BLOCKED", AccountEventType::Terminated) => Some("TERMINATED"),
        _ => None,
    }
}

async fn insert_projection(
    transaction: &mut Transaction<'_, Postgres>,
    event: &AccountEventEnvelope,
    status: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO account_projections (account_id, operational_status, external_sequence, \
         external_occurred_at, last_event_id) VALUES ($1,$2,$3,$4,$5)",
    )
    .bind(event.account_id)
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
    event: &AccountEventEnvelope,
    status: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE account_projections SET operational_status=$2, external_sequence=$3, \
         external_occurred_at=$4, last_event_id=$5 WHERE account_id=$1",
    )
    .bind(event.account_id)
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
    event: &AccountEventEnvelope,
    status: &str,
) -> ApiResult<AccountEventResponse> {
    provision_wallets_for_event(transaction, event, status).await?;
    mark_inbox(transaction, event.event_id, "PROCESSED").await?;
    insert_outbox(transaction, event, status).await?;
    insert_audit(transaction, event, status).await?;
    Ok(AccountEventResponse {
        event_id: event.event_id,
        outcome: AccountEventOutcome::Applied,
        account_status: Some(status.to_string()),
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
    event: &AccountEventEnvelope,
    reason_code: &str,
) -> ApiResult<AccountEventResponse> {
    mark_inbox(transaction, event.event_id, "QUARANTINED").await?;
    let detail = format!(
        "event {} with sequence {} cannot advance account {}",
        event.event_type.as_str(),
        event.sequence,
        event.account_id
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
    let projection = lock_projection(transaction, event.account_id).await?;
    Ok(response_from_projection(
        event.event_id,
        AccountEventOutcome::Quarantined,
        projection,
    ))
}

async fn insert_outbox(
    transaction: &mut Transaction<'_, Postgres>,
    event: &AccountEventEnvelope,
    status: &str,
) -> ApiResult<()> {
    let domain_event = build_domain_event(event, status);
    sqlx::query(
        "INSERT INTO outbox_events (event_id,event_type,aggregate_type,aggregate_id, \
         aggregate_sequence,account_id,correlation_id,causation_id,payload,occurred_at) \
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)",
    )
    .bind(domain_event.event_id)
    .bind(&domain_event.event_type)
    .bind(&domain_event.aggregate_type)
    .bind(domain_event.aggregate_id)
    .bind(domain_event.sequence)
    .bind(domain_event.account_id)
    .bind(domain_event.correlation_id)
    .bind(domain_event.causation_id)
    .bind(serde_json::to_value(&domain_event).map_err(ApiError::serialization)?)
    .bind(domain_event.occurred_at)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

fn build_domain_event(event: &AccountEventEnvelope, status: &str) -> DomainEventEnvelope {
    DomainEventEnvelope {
        event_id: Uuid::new_v4(),
        event_type: "account.projection_updated".to_string(),
        schema_version: 1,
        aggregate_type: "account".to_string(),
        aggregate_id: event.account_id,
        sequence: event.sequence,
        occurred_at: chrono::Utc::now(),
        account_id: event.account_id,
        correlation_id: event.correlation_id,
        causation_id: Some(event.event_id),
        payload: json!({ "operational_status": status }),
    }
}

async fn insert_audit(
    transaction: &mut Transaction<'_, Postgres>,
    event: &AccountEventEnvelope,
    status: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO audit_events (audit_event_id,account_id,action,resource_kind,resource_id, \
         correlation_id,details) VALUES ($1,$2,$3,'account_projection',$2,$4,$5)",
    )
    .bind(Uuid::new_v4())
    .bind(event.account_id)
    .bind(event.event_type.as_str())
    .bind(event.correlation_id)
    .bind(json!({ "operational_status": status, "sequence": event.sequence }))
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

fn response_from_projection(
    event_id: Uuid,
    outcome: AccountEventOutcome,
    projection: Option<AccountProjection>,
) -> AccountEventResponse {
    AccountEventResponse {
        event_id,
        outcome,
        account_status: projection.as_ref().map(|value| value.status.clone()),
        external_sequence: projection.map(|value| value.sequence),
    }
}
