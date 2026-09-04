use chrono::{DateTime, Utc};
use sqlx::{Postgres, Row, Transaction};
use uuid::Uuid;

use crate::{
    dto::{
        plans::{CreatePlanTransitionRequest, PlanTransitionResponse},
        units::CreditUnits,
    },
    error::{ApiError, ApiResult},
    repositories::{
        credit_writes::complete_reservations,
        credits::{lock_active_customer_wallet, reserve_idempotency, reserve_transaction},
        database::DatabaseRepository,
        plan_rows::PlanRecord,
        plan_writes::{insert_entitlements, insert_plan_outbox, lock_valid_plan},
    },
};

impl DatabaseRepository {
    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn apply_plan_downgrade(
        &self,
        workspace_id: Uuid,
        customer_plan_id: Uuid,
        idempotency_key: &str,
        request_hash: &str,
        request: &CreatePlanTransitionRequest,
        target_plan: &PlanRecord,
        effective_at: DateTime<Utc>,
        new_period_end: Option<DateTime<Utc>>,
    ) -> ApiResult<PlanTransitionResponse> {
        let mut transaction = self.pool().begin().await?;
        lock_active_customer_wallet(&mut transaction, workspace_id).await?;
        reserve_transition_keys(
            &mut transaction,
            workspace_id,
            idempotency_key,
            request_hash,
            &request.transaction_id,
        )
        .await?;
        lock_valid_plan(&mut transaction, target_plan.response.plan_version_id).await?;
        let current =
            lock_transition_source(&mut transaction, workspace_id, customer_plan_id).await?;
        validate_transition_source(customer_plan_id, &current, target_plan)?;
        let transition_id = Uuid::new_v4();
        insert_transition_row(
            &mut transaction,
            transition_id,
            customer_plan_id,
            request,
            target_plan,
            &current,
            effective_at,
            new_period_end,
        )
        .await?;
        let reclassified = reclassify_subscription_lots(
            &mut transaction,
            customer_plan_id,
            transition_id,
            &request.actor_reference,
        )
        .await?;
        apply_downgrade_state(
            &mut transaction,
            customer_plan_id,
            target_plan,
            &current,
            effective_at,
            new_period_end,
        )
        .await?;
        complete_reservations(
            &mut transaction,
            workspace_id,
            idempotency_key,
            &request.transaction_id,
            transition_id,
        )
        .await?;
        insert_plan_outbox(
            &mut transaction,
            workspace_id,
            customer_plan_id,
            transition_id,
            "customer_plan.plan_changed",
            current.version + 1,
        )
        .await?;
        transaction.commit().await?;
        Ok(PlanTransitionResponse {
            plan_transition_id: transition_id,
            transition_kind: request.transition_kind,
            previous_plan_version_id: current.plan_version_id,
            new_plan_version_id: target_plan.response.plan_version_id,
            reclassified_credit_units: CreditUnits::new(reclassified),
            customer_plan: self
                .find_customer_plan(workspace_id, customer_plan_id)
                .await?,
        })
    }
}

struct TransitionSource {
    plan_version_id: Uuid,
    subscription_id: Uuid,
    anchor_at: DateTime<Utc>,
    period_end: Option<DateTime<Utc>>,
    version: i64,
    next_cycle_ordinal: i64,
}

async fn reserve_transition_keys(
    transaction: &mut Transaction<'_, Postgres>,
    workspace_id: Uuid,
    key: &str,
    request_hash: &str,
    transaction_id: &str,
) -> ApiResult<()> {
    reserve_idempotency(
        transaction,
        workspace_id,
        key,
        request_hash,
        "PLAN_TRANSITION",
    )
    .await?;
    reserve_transaction(transaction, workspace_id, transaction_id, "PLAN_TRANSITION").await
}

async fn lock_transition_source(
    transaction: &mut Transaction<'_, Postgres>,
    workspace_id: Uuid,
    customer_plan_id: Uuid,
) -> ApiResult<TransitionSource> {
    let row = sqlx::query(
        "SELECT c.plan_version_id,c.anchor_at,c.version,c.commercial_status,c.activation_status, \
         p.subscription_id,cy.current_period_end,COALESCE(cy.cycle_ordinal,0)+1 next_cycle_ordinal \
         FROM customer_plans c JOIN subscription_plan_versions p ON p.plan_version_id=c.plan_version_id \
         LEFT JOIN customer_plan_cycles cy ON cy.customer_plan_id=c.customer_plan_id AND cy.status='ACTIVE' \
         WHERE c.customer_id=$1 AND c.customer_plan_id=$2 FOR UPDATE OF c",
    )
    .bind(workspace_id)
    .bind(customer_plan_id)
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or_else(|| missing_customer_plan(workspace_id, customer_plan_id))?;
    let status: String = row.get("commercial_status");
    let activation: String = row.get("activation_status");
    if status != "ACTIVE" || activation != "ACTIVATED" {
        return Err(ApiError::conflict(
            "customer_plan_not_active",
            format!("customer plan {customer_plan_id} must be ACTIVE/ACTIVATED, found {status}/{activation}"),
        ));
    }
    Ok(TransitionSource {
        plan_version_id: row.get("plan_version_id"),
        subscription_id: row.get("subscription_id"),
        anchor_at: row.get("anchor_at"),
        period_end: row.get("current_period_end"),
        version: row.get("version"),
        next_cycle_ordinal: row.get("next_cycle_ordinal"),
    })
}

fn validate_transition_source(
    customer_plan_id: Uuid,
    current: &TransitionSource,
    target: &PlanRecord,
) -> ApiResult<()> {
    if current.subscription_id != target.response.subscription_id {
        return Err(ApiError::conflict(
            "plan_transition_crosses_subscription",
            format!("customer plan {customer_plan_id} and target plan {} must belong to subscription {}", target.response.plan_version_id, current.subscription_id),
        ));
    }
    if current.plan_version_id == target.response.plan_version_id {
        return Err(ApiError::unprocessable(
            "plan_transition_has_same_version",
            format!(
                "target plan {} must differ from current plan",
                target.response.plan_version_id
            ),
        ));
    }
    Ok(())
}

async fn reclassify_subscription_lots(
    transaction: &mut Transaction<'_, Postgres>,
    customer_plan_id: Uuid,
    transition_id: Uuid,
    actor_reference: &str,
) -> ApiResult<i64> {
    let rows = sqlx::query(
        "SELECT l.credit_lot_id,l.remaining_credit_units,l.expires_at,cr.customer_plan_cycle_id \
         FROM credit_lots l JOIN wallet_transaction_references lr ON lr.credit_lot_id=l.credit_lot_id \
         JOIN wallet_transaction_references pr ON pr.customer_wallet_entry_id=lr.customer_wallet_entry_id \
         JOIN wallet_transaction_references cr ON cr.customer_wallet_entry_id=lr.customer_wallet_entry_id \
         WHERE pr.customer_plan_id=$1 AND cr.customer_plan_cycle_id IS NOT NULL \
           AND l.source_kind='SUBSCRIPTION' AND l.remaining_credit_units>0 FOR UPDATE OF l",
    )
    .bind(customer_plan_id)
    .fetch_all(&mut **transaction)
    .await?;
    let mut total = 0_i64;
    for row in rows {
        let units: i64 = row.get("remaining_credit_units");
        total = total
            .checked_add(units)
            .ok_or_else(|| ApiError::unexpected("reclassified credit total overflow"))?;
        reclassify_lot(transaction, transition_id, actor_reference, &row).await?;
    }
    Ok(total)
}

async fn reclassify_lot(
    transaction: &mut Transaction<'_, Postgres>,
    transition_id: Uuid,
    actor_reference: &str,
    row: &sqlx::postgres::PgRow,
) -> Result<(), sqlx::Error> {
    let lot_id: Uuid = row.get("credit_lot_id");
    sqlx::query(
        "UPDATE credit_lots SET source_kind='ON_DEMAND',expires_at=NULL WHERE credit_lot_id=$1",
    )
    .bind(lot_id)
    .execute(&mut **transaction)
    .await?;
    sqlx::query(
        "INSERT INTO credit_lot_reclassifications (credit_lot_reclassification_id,credit_lot_id, \
         customer_plan_cycle_id,plan_transition_id,preserved_credit_units,previous_source_kind, \
         previous_expires_at,new_source_kind,actor_reference) VALUES ($1,$2,$3,$4,$5,'SUBSCRIPTION',$6,'ON_DEMAND',$7)",
    )
    .bind(Uuid::new_v4()).bind(lot_id).bind(row.get::<Uuid, _>("customer_plan_cycle_id"))
    .bind(transition_id).bind(row.get::<i64, _>("remaining_credit_units"))
    .bind(row.get::<Option<DateTime<Utc>>, _>("expires_at")).bind(actor_reference)
    .execute(&mut **transaction).await?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn apply_downgrade_state(
    transaction: &mut Transaction<'_, Postgres>,
    customer_plan_id: Uuid,
    target: &PlanRecord,
    current: &TransitionSource,
    effective_at: DateTime<Utc>,
    new_period_end: Option<DateTime<Utc>>,
) -> ApiResult<()> {
    sqlx::query("UPDATE customer_plan_cycles SET status='COMPLETED' WHERE customer_plan_id=$1 AND status='ACTIVE'")
        .bind(customer_plan_id).execute(&mut **transaction).await?;
    sqlx::query("UPDATE customer_plan_entitlements SET effective_until=$2 WHERE customer_plan_id=$1 AND effective_until IS NULL")
        .bind(customer_plan_id).bind(effective_at).execute(&mut **transaction).await?;
    insert_transition_cycle(
        transaction,
        customer_plan_id,
        current.next_cycle_ordinal,
        effective_at,
        new_period_end,
    )
    .await?;
    insert_entitlements(
        transaction,
        customer_plan_id,
        &target.response.product_ids,
        effective_at,
    )
    .await?;
    sqlx::query("UPDATE customer_plans SET plan_version_id=$2,anchor_at=$3,cancel_at_period_end=false,renewal_status='CURRENT',version=version+1 WHERE customer_plan_id=$1")
        .bind(customer_plan_id).bind(target.response.plan_version_id).bind(effective_at)
        .execute(&mut **transaction).await?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn insert_transition_row(
    transaction: &mut Transaction<'_, Postgres>,
    transition_id: Uuid,
    customer_plan_id: Uuid,
    request: &CreatePlanTransitionRequest,
    target: &PlanRecord,
    current: &TransitionSource,
    effective_at: DateTime<Utc>,
    new_period_end: Option<DateTime<Utc>>,
) -> Result<(), sqlx::Error> {
    sqlx::query("INSERT INTO customer_plan_transitions (plan_transition_id,customer_plan_id,previous_plan_version_id,new_plan_version_id,transition_kind,transaction_id,actor_reference,previous_anchor_at,previous_period_end,new_anchor_at,new_period_end) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11)")
        .bind(transition_id).bind(customer_plan_id).bind(current.plan_version_id)
        .bind(target.response.plan_version_id).bind(request.transition_kind.as_str())
        .bind(&request.transaction_id).bind(&request.actor_reference).bind(current.anchor_at)
        .bind(current.period_end).bind(effective_at).bind(new_period_end)
        .execute(&mut **transaction).await?;
    Ok(())
}

async fn insert_transition_cycle(
    transaction: &mut Transaction<'_, Postgres>,
    customer_plan_id: Uuid,
    ordinal: i64,
    effective_at: DateTime<Utc>,
    period_end: Option<DateTime<Utc>>,
) -> Result<(), sqlx::Error> {
    sqlx::query("INSERT INTO customer_plan_cycles (customer_plan_cycle_id,customer_plan_id,cycle_ordinal,current_period_start,current_period_end,granted_credit_units,status) VALUES ($1,$2,$3,$4,$5,0,'ACTIVE')")
        .bind(Uuid::new_v4()).bind(customer_plan_id).bind(ordinal).bind(effective_at).bind(period_end)
        .execute(&mut **transaction).await?;
    Ok(())
}

fn missing_customer_plan(workspace_id: Uuid, customer_plan_id: Uuid) -> ApiError {
    ApiError::not_found(
        "commercial_resource_not_found",
        format!("customer_plan {customer_plan_id} does not exist in workspace {workspace_id}"),
    )
}
