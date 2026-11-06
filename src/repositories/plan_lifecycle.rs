use chrono::Utc;
use sqlx::Row;
use uuid::Uuid;

use crate::{
    dto::plans::CustomerPlanResponse,
    error::{ApiError, ApiResult},
    repositories::{database::DatabaseRepository, plan_writes::insert_plan_outbox},
};

impl DatabaseRepository {
    pub async fn cancel_customer_plan(
        &self,
        account_id: Uuid,
        customer_plan_id: Uuid,
    ) -> ApiResult<CustomerPlanResponse> {
        let mut transaction = self.pool().begin().await?;
        let row = lock_customer_plan(&mut transaction, account_id, customer_plan_id).await?;
        let status: String = row.get("commercial_status");
        if matches!(status.as_str(), "CANCELED" | "EXPIRED" | "REVOKED")
            || row.get::<bool, _>("cancel_at_period_end")
        {
            transaction.commit().await?;
            return self.find_customer_plan(account_id, customer_plan_id).await;
        }
        let immediate = row.get::<String, _>("activation_status") != "ACTIVATED"
            || row.get::<String, _>("recurrence") == "NONE";
        if immediate {
            cancel_pending_collections(&mut transaction, customer_plan_id, "CUSTOMER_CANCELED")
                .await?;
            close_customer_plan(
                &mut transaction,
                customer_plan_id,
                Utc::now(),
                "CANCELED",
                "CUSTOMER_CANCELED",
            )
            .await?;
        } else {
            sqlx::query("UPDATE customer_plans SET cancel_at_period_end=true,renewal_status='RENEWAL_INACTIVE',version=version+1 WHERE customer_plan_id=$1")
                .bind(customer_plan_id).execute(&mut *transaction).await?;
            sqlx::query("INSERT INTO subscription_calendar_jobs (customer_plan_cycle_id,due_at) SELECT customer_plan_cycle_id,current_period_end FROM customer_plan_cycles WHERE customer_plan_id=$1 AND status='ACTIVE' AND current_period_end IS NOT NULL ON CONFLICT (customer_plan_cycle_id) DO UPDATE SET due_at=EXCLUDED.due_at,retry_at='-infinity',completed_at=NULL")
                .bind(customer_plan_id).execute(&mut *transaction).await?;
        }
        insert_plan_outbox(
            &mut transaction,
            account_id,
            customer_plan_id,
            row.get("plan_version_id"),
            if immediate {
                "customer_plan.canceled"
            } else {
                "customer_plan.cancellation_scheduled"
            },
            row.get::<i64, _>("version") + 1,
        )
        .await?;
        transaction.commit().await?;
        self.find_customer_plan(account_id, customer_plan_id).await
    }

    pub async fn revoke_customer_plan(
        &self,
        account_id: Uuid,
        customer_plan_id: Uuid,
        reason: &str,
        actor_reference: &str,
    ) -> ApiResult<CustomerPlanResponse> {
        let mut transaction = self.pool().begin().await?;
        let row = lock_customer_plan(&mut transaction, account_id, customer_plan_id).await?;
        let status: String = row.get("commercial_status");
        if status == "REVOKED" {
            transaction.commit().await?;
            return self.find_customer_plan(account_id, customer_plan_id).await;
        }
        ensure_revocable(customer_plan_id, &status)?;
        let version: i64 = row.get("version");
        let plan_version_id: Uuid = row.get("plan_version_id");
        close_customer_plan(
            &mut transaction,
            customer_plan_id,
            Utc::now(),
            "REVOKED",
            "ADMIN_REVOKED",
        )
        .await?;
        cancel_pending_collections(&mut transaction, customer_plan_id, "CUSTOMER_PLAN_REVOKED")
            .await?;
        insert_revocation_audit(
            &mut transaction,
            account_id,
            customer_plan_id,
            reason,
            actor_reference,
        )
        .await?;
        insert_plan_outbox(
            &mut transaction,
            account_id,
            customer_plan_id,
            plan_version_id,
            "customer_plan.revoked",
            version + 1,
        )
        .await?;
        transaction.commit().await?;
        self.find_customer_plan(account_id, customer_plan_id).await
    }
}

async fn cancel_pending_collections(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    customer_plan_id: Uuid,
    reason: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE collection_requests SET status='CANCELED',terminal_reason=$2 \
         WHERE customer_plan_id=$1 AND status IN ('SCHEDULED','COLLECTING','PENDING_PAYMENT')",
    )
    .bind(customer_plan_id)
    .bind(reason)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

fn ensure_revocable(customer_plan_id: Uuid, status: &str) -> ApiResult<()> {
    if !matches!(status, "CANCELED" | "EXPIRED") {
        return Ok(());
    }
    Err(ApiError::conflict(
        "customer_plan_not_revocable",
        format!("customer plan {customer_plan_id} has terminal status {status}"),
    ))
}

async fn lock_customer_plan(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    account_id: Uuid,
    customer_plan_id: Uuid,
) -> ApiResult<sqlx::postgres::PgRow> {
    sqlx::query(
        "SELECT c.commercial_status,c.plan_version_id,c.version,c.activation_status,c.cancel_at_period_end,p.recurrence \
         FROM customer_plans c JOIN subscription_plan_versions p ON p.plan_version_id=c.plan_version_id \
         WHERE c.customer_id=$1 AND c.customer_plan_id=$2 FOR UPDATE OF c",
    )
    .bind(account_id)
    .bind(customer_plan_id)
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or_else(|| {
        ApiError::not_found(
            "commercial_resource_not_found",
            format!("customer_plan {customer_plan_id} does not exist in account {account_id}"),
        )
    })
}

async fn close_customer_plan(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    customer_plan_id: Uuid,
    effective_at: chrono::DateTime<Utc>,
    status: &str,
    reason: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE customer_plans SET commercial_status=$3,renewal_status='RENEWAL_INACTIVE', \
         ended_at=$2,end_reason=$4,version=version+1 WHERE customer_plan_id=$1",
    )
    .bind(customer_plan_id)
    .bind(effective_at)
    .bind(status)
    .bind(reason)
    .execute(&mut **transaction)
    .await?;
    close_customer_plan_dependents(transaction, customer_plan_id, effective_at).await
}

async fn close_customer_plan_dependents(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    customer_plan_id: Uuid,
    effective_at: chrono::DateTime<Utc>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE customer_plan_cycles SET status='CANCELED' WHERE customer_plan_id=$1 AND status='ACTIVE'",
    )
    .bind(customer_plan_id)
    .execute(&mut **transaction)
    .await?;
    sqlx::query(
        "UPDATE customer_plan_entitlements SET effective_until=$2 WHERE customer_plan_id=$1 AND effective_until IS NULL",
    )
    .bind(customer_plan_id)
    .bind(effective_at)
    .execute(&mut **transaction)
    .await?;
    sqlx::query("DELETE FROM active_customer_plan_slots WHERE customer_plan_id=$1")
        .bind(customer_plan_id)
        .execute(&mut **transaction)
        .await?;
    Ok(())
}

async fn insert_revocation_audit(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    account_id: Uuid,
    customer_plan_id: Uuid,
    reason: &str,
    actor_reference: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO audit_events (audit_event_id,account_id,actor_reference,action,resource_kind, \
         resource_id,correlation_id,details) VALUES ($1,$2,$3,'customer_plan.revoked', \
         'customer_plan',$4,$5,jsonb_build_object('reason',$6))",
    )
    .bind(Uuid::new_v4())
    .bind(account_id)
    .bind(actor_reference)
    .bind(customer_plan_id)
    .bind(Uuid::new_v4())
    .bind(reason)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}
