use std::time::Duration;

use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::{
    error::{ApiError, ApiResult},
    repositories::database::DatabaseRepository,
    services::calendar::cycle_end,
};

#[derive(Debug, PartialEq, Eq)]
pub enum CalendarDispatchOutcome {
    Idle,
    Advanced,
    Deferred,
}

/// Resume persisted free-plan calendar work; e.g. spawn once when the server starts.
pub async fn run_scheduler(repository: DatabaseRepository) {
    loop {
        match dispatch_next(&repository).await {
            Ok(CalendarDispatchOutcome::Advanced | CalendarDispatchOutcome::Deferred) => continue,
            Ok(CalendarDispatchOutcome::Idle) => {}
            Err(error) => tracing::error!(
                error_code = error.code(),
                "subscription calendar dispatch failed"
            ),
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}

async fn dispatch_next(repository: &DatabaseRepository) -> ApiResult<CalendarDispatchOutcome> {
    dispatch_once(repository, repository.current_time().await?).await
}

/// Execute one durable calendar job; e.g. call with a cutoff to recover overdue cycles.
pub async fn dispatch_once(
    repository: &DatabaseRepository,
    as_of: DateTime<Utc>,
) -> ApiResult<CalendarDispatchOutcome> {
    let token = Uuid::new_v4();
    let Some(due) = repository.claim_calendar_job(as_of, token).await? else {
        return Ok(CalendarDispatchOutcome::Idle);
    };
    let result = advance_claimed(repository, &due, as_of).await;
    if let Err(error) = result {
        repository
            .defer_calendar_job(due.cycle_id, token, error.code())
            .await?;
        tracing::warn!(cycle_id=%due.cycle_id, error_code=error.code(), error=%error, "subscription calendar job deferred");
        return Ok(CalendarDispatchOutcome::Deferred);
    }
    Ok(CalendarDispatchOutcome::Advanced)
}

async fn advance_claimed(
    repository: &DatabaseRepository,
    due: &crate::repositories::plan_cycles::DueCycle,
    as_of: DateTime<Utc>,
) -> ApiResult<()> {
    let next_end = cycle_end(due.anchor_at, due.recurrence, due.cycle_ordinal + 1)?
        .ok_or_else(|| ApiError::unexpected("calendar job requires a recurring cycle boundary"))?;
    repository
        .advance_customer_plan_cycle(due.customer_plan_id, due.cycle_id, as_of, next_end)
        .await?;
    Ok(())
}
