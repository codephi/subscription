use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::Row;
use uuid::Uuid;

use crate::{error::ApiResult, repositories::database::DatabaseRepository};

#[derive(Debug)]
pub struct ClaimedOutboxEvent {
    pub event_id: Uuid,
    pub payload: Value,
    pub attempts: i32,
}

impl DatabaseRepository {
    pub async fn claim_outbox_event(
        &self,
        worker_id: Uuid,
    ) -> ApiResult<Option<ClaimedOutboxEvent>> {
        let row = sqlx::query(
            "WITH candidate AS (SELECT event_id FROM outbox_events WHERE delivered_at IS NULL \
             AND dead_lettered_at IS NULL AND available_at <= now() AND \
             (lease_until IS NULL OR lease_until < now()) ORDER BY occurred_at LIMIT 1 \
             FOR UPDATE SKIP LOCKED) UPDATE outbox_events o SET lease_owner=$1, \
             lease_until=now()+interval '30 seconds' FROM candidate c WHERE o.event_id=c.event_id \
             RETURNING o.event_id,o.payload,o.delivery_attempts",
        )
        .bind(worker_id)
        .fetch_optional(&self.pool())
        .await?;
        Ok(row.map(|row| ClaimedOutboxEvent {
            event_id: row.get("event_id"),
            payload: row.get("payload"),
            attempts: row.get("delivery_attempts"),
        }))
    }

    pub async fn mark_outbox_delivered(&self, event_id: Uuid) -> ApiResult<()> {
        sqlx::query(
            "UPDATE outbox_events SET delivered_at=now(),lease_owner=NULL,lease_until=NULL \
             WHERE event_id=$1",
        )
        .bind(event_id)
        .execute(&self.pool())
        .await?;
        Ok(())
    }

    pub async fn mark_outbox_failed(
        &self,
        event_id: Uuid,
        attempts: i32,
        available_at: DateTime<Utc>,
        error: &str,
    ) -> ApiResult<()> {
        let dead_lettered = (attempts >= 12).then(Utc::now);
        sqlx::query(
            "UPDATE outbox_events SET delivery_attempts=$2,available_at=$3,last_error=$4, \
             dead_lettered_at=$5,lease_owner=NULL,lease_until=NULL WHERE event_id=$1",
        )
        .bind(event_id)
        .bind(attempts)
        .bind(available_at)
        .bind(error)
        .bind(dead_lettered)
        .execute(&self.pool())
        .await?;
        Ok(())
    }

    pub async fn replay_dead_letter(&self, event_id: Uuid) -> ApiResult<()> {
        let result = sqlx::query(
            "UPDATE outbox_events SET delivery_attempts=0,available_at=now(),dead_lettered_at=NULL, \
             last_error=NULL,lease_owner=NULL,lease_until=NULL WHERE event_id=$1 \
             AND dead_lettered_at IS NOT NULL",
        )
        .bind(event_id)
        .execute(&self.pool())
        .await?;
        if result.rows_affected() == 1 {
            return Ok(());
        }
        Err(crate::error::ApiError::conflict(
            "outbox_event_not_replayable",
            format!("event_id {event_id} must identify a dead-lettered event"),
        ))
    }
}
