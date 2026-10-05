use chrono::{DateTime, Utc};
use sqlx::{Postgres, Row, Transaction};
use uuid::Uuid;

use crate::{
    dto::plans::{CustomerPlanCycleResponse, PlanRecurrence},
    error::{ApiError, ApiResult},
    repositories::{
        credits::LockedWallet,
        database::DatabaseRepository,
        plan_cycles::expire_cycle_lot,
        plan_rows::{cycle_from_row, parse_recurrence, PlanRecord},
        plan_writes::{grant_cycle_credit, insert_plan_outbox},
    },
};

impl DatabaseRepository {
    pub async fn schedule_due_paid_renewals(&self, as_of: DateTime<Utc>) -> ApiResult<u64> {
        let result = sqlx::query(
            "INSERT INTO collection_requests (collection_request_id,workspace_id,customer_id,customer_plan_id, \
             plan_version_id,payment_method_binding_id,request_kind,amount_minor,currency,granted_credit_units, \
             status,transaction_id,idempotency_key,correlation_id,scheduled_at,payment_expires_at) \
             SELECT gen_random_uuid(),cp.customer_id,cp.customer_id,cp.customer_plan_id,cp.plan_version_id, \
               pmb.payment_method_binding_id,'RENEWAL',p.price_amount_minor,p.currency,p.granted_credit_units, \
               'SCHEDULED', 'renewal:'||cp.customer_plan_id::text||':'||cy.customer_plan_cycle_id::text, \
               'renewal:'||cp.customer_plan_id::text||':'||cy.customer_plan_cycle_id::text,gen_random_uuid(),$1, \
               $1+s.payment_completion_window \
             FROM customer_plans cp JOIN subscription_plan_versions p USING(plan_version_id) \
             JOIN subscriptions s USING(subscription_id) JOIN customer_plan_cycles cy \
               ON cy.customer_plan_id=cp.customer_plan_id AND cy.status='ACTIVE' \
             JOIN payment_method_bindings pmb ON pmb.customer_id=cp.customer_id AND pmb.workspace_id=cp.customer_id \
               AND pmb.customer_plan_id=cp.customer_plan_id AND pmb.status='ACTIVE' \
             WHERE cp.commercial_status='ACTIVE_PAID' AND cp.activation_status='ACTIVATED' \
               AND cp.renewal_status='CURRENT' AND p.commercial_model='PAID' AND p.revoked_at IS NULL \
               AND p.recurrence='MONTHLY' AND cy.current_period_end<=$1 \
               AND NOT EXISTS (SELECT 1 FROM collection_requests cr WHERE cr.customer_plan_id=cp.customer_plan_id \
                 AND cr.request_kind='RENEWAL' AND cr.transaction_id='renewal:'||cp.customer_plan_id::text||':'||cy.customer_plan_cycle_id::text) \
             ON CONFLICT DO NOTHING",
        )
        .bind(as_of)
        .execute(&self.pool())
        .await?;
        Ok(result.rows_affected())
    }

    pub async fn confirmation_schedule(
        &self,
        collection_request_id: Uuid,
        confirmed_at: DateTime<Utc>,
    ) -> ApiResult<(PlanRecurrence, DateTime<Utc>, i64)> {
        let row = sqlx::query(
            "SELECT p.recurrence,cr.request_kind,cp.anchor_at,cp.anchor_cycle_ordinal,cy.cycle_ordinal \
             FROM collection_requests cr JOIN subscription_plan_versions p \
               ON p.plan_version_id=cr.plan_version_id JOIN customer_plans cp \
               ON cp.customer_plan_id=cr.customer_plan_id LEFT JOIN customer_plan_cycles cy \
               ON cy.customer_plan_id=cp.customer_plan_id AND cy.status='ACTIVE' \
             WHERE cr.collection_request_id=$1",
        )
        .bind(collection_request_id)
        .fetch_optional(&self.pool())
        .await?
        .ok_or_else(|| missing_schedule(collection_request_id))?;
        let recurrence = parse_recurrence(row.get("recurrence"))?;
        if row.get::<String, _>("request_kind") != "RENEWAL" {
            return Ok((recurrence, confirmed_at, 1));
        }
        let cycle_ordinal: Option<i64> = row.get("cycle_ordinal");
        let ordinal = cycle_ordinal
            .ok_or_else(|| invalid_renewal(collection_request_id, "active cycle required"))?
            - row.get::<i64, _>("anchor_cycle_ordinal")
            + 2;
        Ok((recurrence, row.get("anchor_at"), ordinal))
    }
}

struct CurrentPaidCycle {
    cycle_id: Uuid,
    ordinal: i64,
    period_end: DateTime<Utc>,
    customer_plan_version: i64,
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn renew_paid_customer_plan(
    transaction: &mut Transaction<'_, Postgres>,
    wallet: &LockedWallet,
    workspace_id: Uuid,
    customer_plan_id: Uuid,
    plan: &PlanRecord,
    confirmed_at: DateTime<Utc>,
    next_period_end: Option<DateTime<Utc>>,
    transaction_id: &str,
) -> ApiResult<CustomerPlanCycleResponse> {
    let current = lock_current_cycle(transaction, customer_plan_id).await?;
    validate_renewal_boundary(&current, confirmed_at, next_period_end)?;
    let wallet = expire_cycle_lot(
        transaction,
        wallet,
        workspace_id,
        customer_plan_id,
        current.cycle_id,
        plan.response.plan_version_id,
    )
    .await?;
    complete_cycle(transaction, current.cycle_id).await?;
    let cycle = insert_renewal_cycle(
        transaction,
        customer_plan_id,
        plan,
        &current,
        current.period_end,
        next_period_end.expect("validated recurring period end"),
    )
    .await?;
    if plan.response.granted_credit_units.value() > 0 {
        grant_cycle_credit(
            transaction,
            &wallet,
            workspace_id,
            customer_plan_id,
            cycle.customer_plan_cycle_id,
            plan,
            cycle.current_period_end,
            Some(transaction_id),
        )
        .await?;
    }
    insert_plan_outbox(
        transaction,
        workspace_id,
        customer_plan_id,
        cycle.customer_plan_cycle_id,
        "customer_plan.cycle_started",
        current.customer_plan_version + 1,
    )
    .await?;
    Ok(cycle)
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn regularize_paid_customer_plan(
    transaction: &mut Transaction<'_, Postgres>,
    wallet: &LockedWallet,
    workspace_id: Uuid,
    customer_plan_id: Uuid,
    plan: &PlanRecord,
    confirmed_at: DateTime<Utc>,
    next_period_end: Option<DateTime<Utc>>,
    transaction_id: &str,
) -> ApiResult<CustomerPlanCycleResponse> {
    let current = lock_current_cycle(transaction, customer_plan_id).await?;
    let period_end = next_period_end
        .filter(|end| *end > confirmed_at)
        .ok_or_else(|| {
            invalid_renewal(
                customer_plan_id,
                "regularization requires a future period end",
            )
        })?;
    let wallet = expire_cycle_lot(
        transaction,
        wallet,
        workspace_id,
        customer_plan_id,
        current.cycle_id,
        plan.response.plan_version_id,
    )
    .await?;
    complete_cycle(transaction, current.cycle_id).await?;
    let cycle = insert_renewal_cycle(
        transaction,
        customer_plan_id,
        plan,
        &current,
        confirmed_at,
        period_end,
    )
    .await?;
    sqlx::query(
        "UPDATE customer_plans SET anchor_at=$2,anchor_cycle_ordinal=$3 WHERE customer_plan_id=$1",
    )
    .bind(customer_plan_id)
    .bind(confirmed_at)
    .bind(cycle.cycle_ordinal)
    .execute(&mut **transaction)
    .await?;
    if plan.response.granted_credit_units.value() > 0 {
        grant_cycle_credit(
            transaction,
            &wallet,
            workspace_id,
            customer_plan_id,
            cycle.customer_plan_cycle_id,
            plan,
            cycle.current_period_end,
            Some(transaction_id),
        )
        .await?;
    }
    insert_plan_outbox(
        transaction,
        workspace_id,
        customer_plan_id,
        cycle.customer_plan_cycle_id,
        "customer_plan.regularized",
        current.customer_plan_version + 1,
    )
    .await?;
    Ok(cycle)
}

async fn lock_current_cycle(
    transaction: &mut Transaction<'_, Postgres>,
    customer_plan_id: Uuid,
) -> ApiResult<CurrentPaidCycle> {
    let row = sqlx::query(
        "SELECT cy.customer_plan_cycle_id,cy.cycle_ordinal,cy.current_period_end,cp.version \
         FROM customer_plans cp JOIN customer_plan_cycles cy USING(customer_plan_id) \
         WHERE cp.customer_plan_id=$1 AND cy.status='ACTIVE' FOR UPDATE OF cy",
    )
    .bind(customer_plan_id)
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or_else(|| invalid_renewal(customer_plan_id, "an active cycle is required"))?;
    Ok(CurrentPaidCycle {
        cycle_id: row.get("customer_plan_cycle_id"),
        ordinal: row.get("cycle_ordinal"),
        period_end: row.try_get("current_period_end").map_err(|_| {
            invalid_renewal(customer_plan_id, "the active cycle must have a period end")
        })?,
        customer_plan_version: row.get("version"),
    })
}

fn validate_renewal_boundary(
    current: &CurrentPaidCycle,
    confirmed_at: DateTime<Utc>,
    next_period_end: Option<DateTime<Utc>>,
) -> ApiResult<()> {
    if current.period_end <= confirmed_at
        && next_period_end.is_some_and(|end| end > current.period_end)
    {
        return Ok(());
    }
    Err(invalid_renewal(
        current.cycle_id,
        "confirmation and next boundary must follow the active cycle",
    ))
}

async fn complete_cycle(
    transaction: &mut Transaction<'_, Postgres>,
    cycle_id: Uuid,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE customer_plan_cycles SET status='COMPLETED' WHERE customer_plan_cycle_id=$1",
    )
    .bind(cycle_id)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

async fn insert_renewal_cycle(
    transaction: &mut Transaction<'_, Postgres>,
    customer_plan_id: Uuid,
    plan: &PlanRecord,
    current: &CurrentPaidCycle,
    period_start: DateTime<Utc>,
    period_end: DateTime<Utc>,
) -> ApiResult<CustomerPlanCycleResponse> {
    let row = sqlx::query(
        "INSERT INTO customer_plan_cycles (customer_plan_cycle_id,customer_plan_id,cycle_ordinal, \
         current_period_start,current_period_end,granted_credit_units,status) \
         VALUES ($1,$2,$3,$4,$5,$6,'ACTIVE') RETURNING *",
    )
    .bind(Uuid::new_v4())
    .bind(customer_plan_id)
    .bind(current.ordinal + 1)
    .bind(period_start)
    .bind(period_end)
    .bind(plan.response.granted_credit_units.value())
    .fetch_one(&mut **transaction)
    .await?;
    Ok(cycle_from_row(&row))
}

fn invalid_renewal(resource_id: Uuid, expected: &str) -> ApiError {
    ApiError::conflict(
        "billing_confirmation_mismatch",
        format!("paid renewal {resource_id} is invalid: {expected}"),
    )
}

fn missing_schedule(collection_request_id: Uuid) -> ApiError {
    ApiError::not_found(
        "collection_request_not_found",
        format!("collection request {collection_request_id} does not exist"),
    )
}
