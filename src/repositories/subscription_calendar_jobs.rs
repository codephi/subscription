use chrono::{DateTime, Utc};
use sqlx::Row;
use uuid::Uuid;

use crate::{
    error::ApiResult,
    repositories::{
        database::DatabaseRepository, plan_cycles::DueCycle, plan_rows::parse_recurrence,
    },
};

impl DatabaseRepository {
    pub(crate) async fn claim_calendar_job(
        &self,
        as_of: DateTime<Utc>,
        token: Uuid,
    ) -> ApiResult<Option<DueCycle>> {
        let row = sqlx::query(include_str!("subscription_calendar_jobs.sql"))
            .bind(as_of)
            .bind(token)
            .fetch_optional(&self.pool())
            .await?;
        row.map(|row| {
            Ok(DueCycle {
                cycle_id: row.get("customer_plan_cycle_id"),
                customer_plan_id: row.get("customer_plan_id"),
                anchor_at: row.get("anchor_at"),
                cycle_ordinal: row.get("cycle_ordinal"),
                recurrence: parse_recurrence(row.get("recurrence"))?,
            })
        })
        .transpose()
    }

    pub(crate) async fn defer_calendar_job(
        &self,
        cycle_id: Uuid,
        token: Uuid,
        error_code: &str,
    ) -> ApiResult<()> {
        sqlx::query("UPDATE subscription_calendar_jobs SET retry_at=clock_timestamp()+interval '30 seconds',last_error_code=$3,lease_token=NULL,lease_expires_at=NULL WHERE customer_plan_cycle_id=$1 AND lease_token=$2 AND completed_at IS NULL")
            .bind(cycle_id).bind(token).bind(error_code).execute(&self.pool()).await?;
        Ok(())
    }
}
