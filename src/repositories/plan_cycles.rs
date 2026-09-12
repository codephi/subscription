use chrono::{DateTime, Utc};
use sqlx::{Postgres, Row, Transaction};
use uuid::Uuid;

use crate::{
    dto::{
        plans::{CommercialModel, PlanRecurrence},
        units::CreditUnits,
    },
    error::{ApiError, ApiResult},
    repositories::{
        credit_writes::insert_credit_expiry_outbox,
        credits::{lock_active_customer_wallet, LockedWallet},
        database::DatabaseRepository,
        plan_rows::{parse_recurrence, plan_from_row, PlanRecord},
        plan_writes::{grant_cycle_credit, insert_plan_outbox, insert_plan_references},
    },
};

#[derive(Clone)]
pub(crate) struct DueCycle {
    pub cycle_id: Uuid,
    pub customer_plan_id: Uuid,
    pub anchor_at: DateTime<Utc>,
    pub cycle_ordinal: i64,
    pub recurrence: PlanRecurrence,
}

#[derive(Clone, Copy, Default)]
pub(crate) struct CycleAdvanceOutcome {
    pub created_cycle: bool,
    pub canceled_plan: bool,
}

impl DatabaseRepository {
    pub(crate) async fn find_due_cycles(&self, as_of: DateTime<Utc>) -> ApiResult<Vec<DueCycle>> {
        let rows = sqlx::query(
            "SELECT c.customer_plan_id,c.anchor_at,cy.customer_plan_cycle_id,cy.cycle_ordinal-c.anchor_cycle_ordinal+1 cycle_ordinal,p.recurrence \
             FROM customer_plans c JOIN customer_plan_cycles cy ON cy.customer_plan_id=c.customer_plan_id \
             JOIN subscription_plan_versions p ON p.plan_version_id=c.plan_version_id \
             WHERE c.commercial_status='ACTIVE' AND cy.status='ACTIVE' \
               AND cy.current_period_end IS NOT NULL AND cy.current_period_end<=$1 \
             ORDER BY cy.current_period_end,c.customer_plan_id LIMIT 100",
        )
        .bind(as_of)
        .fetch_all(&self.pool())
        .await?;
        rows.iter()
            .map(|row| {
                Ok(DueCycle {
                    cycle_id: row.get("customer_plan_cycle_id"),
                    customer_plan_id: row.get("customer_plan_id"),
                    anchor_at: row.get("anchor_at"),
                    cycle_ordinal: row.get("cycle_ordinal"),
                    recurrence: parse_recurrence(row.get("recurrence"))?,
                })
            })
            .collect()
    }

    pub(crate) async fn advance_customer_plan_cycle(
        &self,
        customer_plan_id: Uuid,
        expected_cycle_id: Uuid,
        as_of: DateTime<Utc>,
        next_end: DateTime<Utc>,
    ) -> ApiResult<CycleAdvanceOutcome> {
        let workspace_id: Uuid =
            sqlx::query_scalar("SELECT customer_id FROM customer_plans WHERE customer_plan_id=$1")
                .bind(customer_plan_id)
                .fetch_optional(&self.pool())
                .await?
                .ok_or_else(|| {
                    ApiError::not_found(
                        "commercial_resource_not_found",
                        format!("customer_plan {customer_plan_id} does not exist"),
                    )
                })?;
        let mut transaction = self.pool().begin().await?;
        let wallet = lock_active_customer_wallet(&mut transaction, workspace_id).await?;
        let Some(due) =
            lock_due_cycle(&mut transaction, customer_plan_id, expected_cycle_id, as_of).await?
        else {
            transaction.commit().await?;
            return Ok(CycleAdvanceOutcome::default());
        };
        let plan = load_locked_plan(&mut transaction, due.plan_version_id).await?;
        let plan_revoked = plan.response.revoked_at.is_some();
        let wallet = expire_cycle_lot(
            &mut transaction,
            &wallet,
            workspace_id,
            customer_plan_id,
            due.cycle_id,
            due.plan_version_id,
        )
        .await?;
        sqlx::query(
            "UPDATE customer_plan_cycles SET status='COMPLETED' WHERE customer_plan_cycle_id=$1",
        )
        .bind(due.cycle_id)
        .execute(&mut *transaction)
        .await?;
        if due.cancel_at_period_end || plan_revoked {
            terminalize_customer_plan(
                &mut transaction,
                customer_plan_id,
                due.period_end,
                plan_revoked,
            )
            .await?;
            insert_plan_outbox(
                &mut transaction,
                workspace_id,
                customer_plan_id,
                due.cycle_id,
                if plan_revoked {
                    "customer_plan.expired"
                } else {
                    "customer_plan.canceled"
                },
                due.customer_plan_version + 1,
            )
            .await?;
            transaction.commit().await?;
            return Ok(CycleAdvanceOutcome {
                created_cycle: false,
                canceled_plan: true,
            });
        }
        let next_cycle_id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO customer_plan_cycles (customer_plan_cycle_id,customer_plan_id,cycle_ordinal, \
             current_period_start,current_period_end,granted_credit_units,status) \
             VALUES ($1,$2,$3,$4,$5,$6,'ACTIVE')",
        )
        .bind(next_cycle_id)
        .bind(customer_plan_id)
        .bind(due.ordinal + 1)
        .bind(due.period_end)
        .bind(next_end)
        .bind(plan.response.granted_credit_units.value())
        .execute(&mut *transaction)
        .await?;
        if plan.response.commercial_model == CommercialModel::Free
            && plan.response.granted_credit_units.value() > 0
        {
            grant_cycle_credit(
                &mut transaction,
                &wallet,
                workspace_id,
                customer_plan_id,
                next_cycle_id,
                &plan,
                Some(next_end),
                None,
            )
            .await?;
        }
        sqlx::query("UPDATE customer_plans SET version=version+1 WHERE customer_plan_id=$1")
            .bind(customer_plan_id)
            .execute(&mut *transaction)
            .await?;
        insert_plan_outbox(
            &mut transaction,
            workspace_id,
            customer_plan_id,
            next_cycle_id,
            "customer_plan.cycle_started",
            due.customer_plan_version + 1,
        )
        .await?;
        transaction.commit().await?;
        Ok(CycleAdvanceOutcome {
            created_cycle: true,
            canceled_plan: false,
        })
    }
}

struct LockedDueCycle {
    cycle_id: Uuid,
    ordinal: i64,
    period_end: DateTime<Utc>,
    plan_version_id: Uuid,
    cancel_at_period_end: bool,
    customer_plan_version: i64,
}

async fn lock_due_cycle(
    transaction: &mut Transaction<'_, Postgres>,
    customer_plan_id: Uuid,
    expected_cycle_id: Uuid,
    as_of: DateTime<Utc>,
) -> ApiResult<Option<LockedDueCycle>> {
    let row = sqlx::query(
        "SELECT cy.customer_plan_cycle_id,cy.cycle_ordinal,cy.current_period_end,c.plan_version_id,c.version, \
         c.cancel_at_period_end FROM customer_plans c \
         JOIN customer_plan_cycles cy ON cy.customer_plan_id=c.customer_plan_id \
         WHERE c.customer_plan_id=$1 AND cy.customer_plan_cycle_id=$2 \
           AND c.commercial_status='ACTIVE' AND cy.status='ACTIVE' \
           AND cy.current_period_end IS NOT NULL AND cy.current_period_end<=$3 FOR UPDATE OF c,cy",
    )
    .bind(customer_plan_id)
    .bind(expected_cycle_id)
    .bind(as_of)
    .fetch_optional(&mut **transaction)
    .await?;
    Ok(row.map(|row| LockedDueCycle {
        cycle_id: row.get("customer_plan_cycle_id"),
        ordinal: row.get("cycle_ordinal"),
        period_end: row.get("current_period_end"),
        plan_version_id: row.get("plan_version_id"),
        cancel_at_period_end: row.get("cancel_at_period_end"),
        customer_plan_version: row.get("version"),
    }))
}

pub(super) async fn load_locked_plan(
    transaction: &mut Transaction<'_, Postgres>,
    plan_id: Uuid,
) -> ApiResult<PlanRecord> {
    let row =
        sqlx::query("SELECT * FROM subscription_plan_versions WHERE plan_version_id=$1 FOR SHARE")
            .bind(plan_id)
            .fetch_one(&mut **transaction)
            .await?;
    let product_ids = sqlx::query_scalar(
        "SELECT product_id FROM subscription_plan_products WHERE plan_version_id=$1 ORDER BY product_id",
    )
    .bind(plan_id)
    .fetch_all(&mut **transaction)
    .await?;
    Ok(PlanRecord {
        response: plan_from_row(&row, product_ids)?,
    })
}

pub(super) async fn expire_cycle_lot(
    transaction: &mut Transaction<'_, Postgres>,
    wallet: &LockedWallet,
    workspace_id: Uuid,
    customer_plan_id: Uuid,
    cycle_id: Uuid,
    plan_id: Uuid,
) -> ApiResult<LockedWallet> {
    let lot = sqlx::query(
        "SELECT l.credit_lot_id,l.remaining_credit_units FROM credit_lots l \
         JOIN wallet_transaction_references r ON r.credit_lot_id=l.credit_lot_id \
         JOIN wallet_transaction_references c ON c.customer_wallet_entry_id=r.customer_wallet_entry_id \
         WHERE c.customer_plan_cycle_id=$1 AND l.source_kind='SUBSCRIPTION' FOR UPDATE OF l",
    )
    .bind(cycle_id)
    .fetch_optional(&mut **transaction)
    .await?;
    let Some(lot) = lot else {
        return Ok(wallet.clone());
    };
    let remaining: i64 = lot.get("remaining_credit_units");
    if remaining == 0 {
        return Ok(wallet.clone());
    }
    let balance_after = wallet
        .balance
        .value()
        .checked_sub(remaining)
        .ok_or_else(|| {
            ApiError::unexpected(format!(
                "wallet {} cannot forfeit {remaining} credits",
                wallet.wallet_id
            ))
        })?;
    let entry_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO customer_wallet_entries (customer_wallet_entry_id,customer_wallet_id,customer_id, \
         entry_sequence,entry_type,source_channel,signed_credit_units,balance_before_credit_units, \
         balance_after_credit_units,metadata,request_id) \
         VALUES ($1,$2,$3,$4,'CREDIT_EXPIRY_FORFEITURE','subscription_cycle',$5,$6,$7,'{}',$8)",
    )
    .bind(entry_id)
    .bind(wallet.wallet_id)
    .bind(workspace_id)
    .bind(wallet.next_sequence)
    .bind(-remaining)
    .bind(wallet.balance.value())
    .bind(balance_after)
    .bind(Uuid::new_v4())
    .execute(&mut **transaction)
    .await?;
    sqlx::query(
        "UPDATE customer_wallets SET balance_credit_units=$2,version=version+1 WHERE wallet_id=$1",
    )
    .bind(wallet.wallet_id)
    .bind(balance_after)
    .execute(&mut **transaction)
    .await?;
    sqlx::query("UPDATE credit_lots SET remaining_credit_units=0 WHERE credit_lot_id=$1")
        .bind(lot.get::<Uuid, _>("credit_lot_id"))
        .execute(&mut **transaction)
        .await?;
    insert_plan_references(
        transaction,
        entry_id,
        lot.get("credit_lot_id"),
        customer_plan_id,
        cycle_id,
        plan_id,
    )
    .await?;
    insert_credit_expiry_outbox(
        transaction,
        workspace_id,
        wallet.wallet_id,
        wallet.next_sequence,
        entry_id,
        remaining,
    )
    .await?;
    Ok(LockedWallet {
        wallet_id: wallet.wallet_id,
        balance: CreditUnits::new(balance_after),
        next_sequence: wallet.next_sequence + 1,
    })
}

async fn terminalize_customer_plan(
    transaction: &mut Transaction<'_, Postgres>,
    customer_plan_id: Uuid,
    effective_at: DateTime<Utc>,
    plan_revoked: bool,
) -> Result<(), sqlx::Error> {
    let status = if plan_revoked { "EXPIRED" } else { "CANCELED" };
    sqlx::query(
        "UPDATE customer_plans SET commercial_status=$2,renewal_status='RENEWAL_INACTIVE', \
         ended_at=$3,end_reason=$4,version=version+1 WHERE customer_plan_id=$1",
    )
    .bind(customer_plan_id)
    .bind(status)
    .bind(effective_at)
    .bind(if plan_revoked {
        "PLAN_REVOKED"
    } else {
        "CUSTOMER_CANCELED"
    })
    .execute(&mut **transaction)
    .await?;
    sqlx::query(
        "UPDATE customer_plan_entitlements SET effective_until=$2 \
         WHERE customer_plan_id=$1 AND effective_until IS NULL",
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
