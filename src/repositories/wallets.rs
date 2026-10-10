use chrono::Utc;
use serde_json::json;
use sqlx::{Postgres, Row, Transaction};
use uuid::Uuid;

#[path = "wallet_failure.rs"]
mod wallet_failure;
#[path = "wallet_readiness.rs"]
pub(super) mod wallet_readiness;

use crate::{
    dto::{
        events::{AccountEventEnvelope, DomainEventEnvelope},
        wallets::{WalletHierarchyResponse, WalletProvisioningResponse, WalletStatus},
    },
    error::{ApiError, ApiResult},
    repositories::{
        database::DatabaseRepository,
        wallet_rows::{
            customer_wallet_from_row, finish_provisioning, item_wallet_from_row,
            project_wallet_state, provisioning_from_row, target_status, wallet_not_provisioned,
        },
    },
};

impl DatabaseRepository {
    pub async fn reconcile_wallets(
        &self,
        account_id: Uuid,
        actor_reference: Option<&str>,
    ) -> ApiResult<WalletProvisioningResponse> {
        let mut transaction = self.pool().begin().await?;
        let account = lock_account(&mut transaction, account_id).await?;
        let correlation_id = Uuid::new_v4();
        let response = wallet_failure::reconcile_attempt(
            &mut transaction,
            account_id,
            &account.status,
            correlation_id,
            actor_reference,
        )
        .await?;
        transaction.commit().await?;
        Ok(response)
    }

    pub async fn find_wallet_hierarchy(
        &self,
        account_id: Uuid,
    ) -> ApiResult<WalletHierarchyResponse> {
        let provisioning = self.find_wallet_provisioning(account_id).await?;
        let customer_row = sqlx::query(
            "SELECT w.*,cw.balance_credit_units,cw.version,s.status FROM wallets w \
             JOIN customer_wallets cw ON cw.wallet_id=w.wallet_id \
             JOIN wallet_effective_states s ON s.wallet_id=w.wallet_id \
             WHERE w.customer_id=$1 AND w.wallet_type='CUSTOMER'",
        )
        .bind(account_id)
        .fetch_optional(&self.pool())
        .await?
        .ok_or_else(|| wallet_not_provisioned(account_id))?;
        let item_rows = sqlx::query(
            "SELECT w.*,iw.total_received_item_units,iw.total_converted_item_units, \
             iw.pending_item_units,iw.version,s.status FROM wallets w \
             JOIN item_wallets iw ON iw.wallet_id=w.wallet_id \
             JOIN wallet_effective_states s ON s.wallet_id=w.wallet_id \
             WHERE w.customer_id=$1 AND w.wallet_type='ITEM' ORDER BY w.item_id",
        )
        .bind(account_id)
        .fetch_all(&self.pool())
        .await?;
        Ok(WalletHierarchyResponse {
            account_id,
            scope_version: provisioning.scope_version,
            ready: provisioning.status == WalletStatus::Active,
            customer_wallet: customer_wallet_from_row(&customer_row)?,
            item_wallets: item_rows
                .iter()
                .map(item_wallet_from_row)
                .collect::<ApiResult<_>>()?,
        })
    }

    pub async fn find_wallet_provisioning(
        &self,
        account_id: Uuid,
    ) -> ApiResult<WalletProvisioningResponse> {
        let row = sqlx::query(
            "SELECT p.* FROM wallet_provisioning p JOIN catalog_scope_current c \
             ON c.scope_version=p.scope_version WHERE p.customer_id=$1 AND c.singleton",
        )
        .bind(account_id)
        .fetch_optional(&self.pool())
        .await?
        .ok_or_else(|| wallet_not_provisioned(account_id))?;
        let mut response = provisioning_from_row(&row)?;
        if response.status == WalletStatus::Active
            && !wallet_readiness::hierarchy_is_ready(&self.pool(), account_id).await?
        {
            response.status = WalletStatus::Provisioning;
            response.completed_at = None;
        }
        Ok(response)
    }
}

pub(super) async fn provision_wallets_for_event(
    transaction: &mut Transaction<'_, Postgres>,
    event: &AccountEventEnvelope,
    operational_status: &str,
) -> ApiResult<()> {
    synchronize_wallets(
        transaction,
        event.account_id,
        operational_status,
        event.correlation_id,
        Some(event.event_id),
        Some("accounts"),
    )
    .await?;
    Ok(())
}

struct LockedAccount {
    status: String,
}

async fn lock_account(
    transaction: &mut Transaction<'_, Postgres>,
    account_id: Uuid,
) -> ApiResult<LockedAccount> {
    let row = sqlx::query(
        "SELECT operational_status FROM account_projections WHERE account_id=$1 FOR UPDATE",
    )
    .bind(account_id)
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or_else(|| {
        ApiError::not_found(
            "account_not_found",
            format!("account {account_id} does not exist"),
        )
    })?;
    Ok(LockedAccount {
        status: row.get("operational_status"),
    })
}

async fn synchronize_wallets(
    transaction: &mut Transaction<'_, Postgres>,
    account_id: Uuid,
    operational_status: &str,
    correlation_id: Uuid,
    causation_id: Option<Uuid>,
    actor_reference: Option<&str>,
) -> ApiResult<WalletProvisioningResponse> {
    let (scope_version, item_ids) = load_current_scope(transaction).await?;
    start_provisioning(transaction, account_id, scope_version, item_ids.len()).await?;
    let (customer_wallet_id, mut changed) =
        ensure_customer_wallet(transaction, account_id, scope_version).await?;
    ensure_billing_config(transaction, account_id).await?;
    for item_id in &item_ids {
        changed |= ensure_item_wallet(
            transaction,
            account_id,
            customer_wallet_id,
            *item_id,
            scope_version,
        )
        .await?;
    }
    let target = target_status(operational_status)?;
    changed |= synchronize_item_states(
        transaction,
        account_id,
        &item_ids,
        target,
        correlation_id,
        actor_reference,
    )
    .await?;
    let materialized = count_materialized(transaction, account_id, &item_ids).await?;
    if materialized != item_ids.len() as i64 {
        return Err(wallet_not_provisioned(account_id));
    }
    changed |= append_lifecycle_if_changed(
        transaction,
        customer_wallet_id,
        target,
        "account lifecycle and scope reconciliation",
        correlation_id,
        actor_reference,
    )
    .await?;
    let response = finish_provisioning(
        transaction,
        account_id,
        scope_version,
        target,
        item_ids.len() as i64,
        materialized,
    )
    .await?;
    if changed && matches!(target, WalletStatus::Provisioning | WalletStatus::Active) {
        emit_provisioning_events(transaction, &response, correlation_id, causation_id).await?;
    }
    Ok(response)
}

async fn load_current_scope(
    transaction: &mut Transaction<'_, Postgres>,
) -> ApiResult<(Uuid, Vec<Uuid>)> {
    let scope_version = sqlx::query_scalar(
        "SELECT scope_version FROM catalog_scope_current WHERE singleton FOR UPDATE",
    )
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or_else(|| {
        ApiError::service_unavailable("catalog_scope_missing", "current catalog scope is absent")
    })?;
    let item_ids = sqlx::query_scalar(
        "SELECT item_id FROM catalog_scope_items WHERE scope_version=$1 ORDER BY item_id",
    )
    .bind(scope_version)
    .fetch_all(&mut **transaction)
    .await?;
    Ok((scope_version, item_ids))
}

async fn start_provisioning(
    transaction: &mut Transaction<'_, Postgres>,
    account_id: Uuid,
    scope_version: Uuid,
    expected: usize,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO wallet_provisioning (customer_id,scope_version,status,expected_item_wallets, \
         materialized_item_wallets) VALUES ($1,$2,'PROVISIONING',$3,0) \
         ON CONFLICT (customer_id,scope_version) DO UPDATE SET status='PROVISIONING', \
         expected_item_wallets=EXCLUDED.expected_item_wallets,error_detail=NULL",
    )
    .bind(account_id)
    .bind(scope_version)
    .bind(expected as i64)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

async fn ensure_customer_wallet(
    transaction: &mut Transaction<'_, Postgres>,
    account_id: Uuid,
    scope_version: Uuid,
) -> ApiResult<(Uuid, bool)> {
    let wallet_id = Uuid::new_v4();
    let inserted = sqlx::query(
        "INSERT INTO wallets (wallet_id,customer_id,wallet_type,provisioning_scope_version) \
         VALUES ($1,$2,'CUSTOMER',$3) ON CONFLICT DO NOTHING",
    )
    .bind(wallet_id)
    .bind(account_id)
    .bind(scope_version)
    .execute(&mut **transaction)
    .await?
    .rows_affected()
        == 1;
    let actual_id: Uuid = sqlx::query_scalar(
        "SELECT wallet_id FROM wallets WHERE customer_id=$1 AND wallet_type='CUSTOMER'",
    )
    .bind(account_id)
    .fetch_one(&mut **transaction)
    .await?;
    sqlx::query(
        "INSERT INTO customer_wallets (wallet_id,balance_credit_units) VALUES ($1,0) \
         ON CONFLICT (wallet_id) DO NOTHING",
    )
    .bind(actual_id)
    .execute(&mut **transaction)
    .await?;
    Ok((actual_id, inserted))
}

async fn ensure_billing_config(
    transaction: &mut Transaction<'_, Postgres>,
    account_id: Uuid,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO account_billing_configs (account_id) VALUES ($1) ON CONFLICT DO NOTHING",
    )
    .bind(account_id)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

async fn ensure_item_wallet(
    transaction: &mut Transaction<'_, Postgres>,
    account_id: Uuid,
    customer_wallet_id: Uuid,
    item_id: Uuid,
    scope_version: Uuid,
) -> ApiResult<bool> {
    let wallet_id = Uuid::new_v4();
    let inserted = sqlx::query(
        "INSERT INTO wallets (wallet_id,customer_id,wallet_type,parent_customer_wallet_id,item_id, \
         provisioning_scope_version) VALUES ($1,$2,'ITEM',$3,$4,$5) ON CONFLICT DO NOTHING",
    )
    .bind(wallet_id)
    .bind(account_id)
    .bind(customer_wallet_id)
    .bind(item_id)
    .bind(scope_version)
    .execute(&mut **transaction)
    .await?
    .rows_affected()
        == 1;
    let actual_id: Uuid = sqlx::query_scalar(
        "SELECT wallet_id FROM wallets WHERE customer_id=$1 AND wallet_type='ITEM' AND item_id=$2",
    )
    .bind(account_id)
    .bind(item_id)
    .fetch_one(&mut **transaction)
    .await?;
    sqlx::query("INSERT INTO item_wallets (wallet_id) VALUES ($1) ON CONFLICT DO NOTHING")
        .bind(actual_id)
        .execute(&mut **transaction)
        .await?;
    Ok(inserted)
}

async fn synchronize_item_states(
    transaction: &mut Transaction<'_, Postgres>,
    account_id: Uuid,
    expected_items: &[Uuid],
    target: WalletStatus,
    correlation_id: Uuid,
    actor_reference: Option<&str>,
) -> ApiResult<bool> {
    let rows = sqlx::query(
        "SELECT wallet_id,item_id FROM wallets WHERE customer_id=$1 AND wallet_type='ITEM' ORDER BY item_id",
    )
    .bind(account_id)
    .fetch_all(&mut **transaction)
    .await?;
    let mut changed = false;
    for row in rows {
        let item_id: Uuid = row.get("item_id");
        let item_target = if expected_items.contains(&item_id) {
            target
        } else {
            WalletStatus::Disabled
        };
        changed |= append_lifecycle_if_changed(
            transaction,
            row.get("wallet_id"),
            item_target,
            "account lifecycle and catalog scope reconciliation",
            correlation_id,
            actor_reference,
        )
        .await?;
    }
    Ok(changed)
}

async fn append_lifecycle_if_changed(
    transaction: &mut Transaction<'_, Postgres>,
    wallet_id: Uuid,
    target: WalletStatus,
    reason: &str,
    correlation_id: Uuid,
    actor_reference: Option<&str>,
) -> ApiResult<bool> {
    // S9-020: the caller holds the account lock; disposable projections cannot
    // determine event identity or overwrite the durable lifecycle during restore.
    let current = sqlx::query(
        "SELECT new_status AS status,sequence AS lifecycle_sequence FROM wallet_lifecycle_events \
         WHERE wallet_id=$1 ORDER BY sequence DESC LIMIT 1",
    )
    .bind(wallet_id)
    .fetch_optional(&mut **transaction)
    .await?;
    let previous: Option<String> = current.as_ref().map(|row| row.get("status"));
    if previous.as_deref() == Some(target.as_str()) {
        project_wallet_state(
            transaction,
            wallet_id,
            target,
            current.as_ref().unwrap().get("lifecycle_sequence"),
        )
        .await?;
        return Ok(false);
    }
    let sequence = current.map_or(1, |row| row.get::<i64, _>("lifecycle_sequence") + 1);
    sqlx::query(
        "INSERT INTO wallet_lifecycle_events (wallet_lifecycle_event_id,wallet_id,sequence, \
         previous_status,new_status,reason,actor_reference,correlation_id) \
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8)",
    )
    .bind(Uuid::new_v4())
    .bind(wallet_id)
    .bind(sequence)
    .bind(&previous)
    .bind(target.as_str())
    .bind(reason)
    .bind(actor_reference)
    .bind(correlation_id)
    .execute(&mut **transaction)
    .await?;
    project_wallet_state(transaction, wallet_id, target, sequence).await?;
    Ok(true)
}

async fn count_materialized(
    transaction: &mut Transaction<'_, Postgres>,
    account_id: Uuid,
    item_ids: &[Uuid],
) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT count(*) FROM wallets w JOIN item_wallets i USING(wallet_id) \
         WHERE w.customer_id=$1 AND w.wallet_type='ITEM' AND w.item_id=ANY($2)",
    )
    .bind(account_id)
    .bind(item_ids)
    .fetch_one(&mut **transaction)
    .await
}

async fn emit_provisioning_events(
    transaction: &mut Transaction<'_, Postgres>,
    response: &WalletProvisioningResponse,
    correlation_id: Uuid,
    causation_id: Option<Uuid>,
) -> ApiResult<()> {
    for event_type in [
        "account_provisioning.started",
        if response.status == WalletStatus::Error {
            "account_provisioning.failed"
        } else {
            "account_provisioning.completed"
        },
    ] {
        let sequence: i64 = sqlx::query_scalar(
            "SELECT COALESCE(max(aggregate_sequence),0)+1 FROM outbox_events \
             WHERE aggregate_type='wallet_provisioning' AND aggregate_id=$1",
        )
        .bind(response.account_id)
        .fetch_one(&mut **transaction)
        .await?;
        let event = DomainEventEnvelope {
            event_id: Uuid::new_v4(),
            event_type: event_type.to_string(),
            schema_version: 1,
            aggregate_type: "wallet_provisioning".to_string(),
            aggregate_id: response.account_id,
            sequence,
            occurred_at: Utc::now(),
            account_id: response.account_id,
            correlation_id,
            causation_id,
            payload: json!({
                "scope_version": response.scope_version,
                "expected_item_wallets": response.expected_item_wallets,
                "materialized_item_wallets": response.materialized_item_wallets,
                "status": response.status.as_str(),
                "error_detail": response.error_detail
            }),
        };
        insert_outbox_event(transaction, &event).await?;
    }
    Ok(())
}

async fn insert_outbox_event(
    transaction: &mut Transaction<'_, Postgres>,
    event: &DomainEventEnvelope,
) -> ApiResult<()> {
    sqlx::query(
        "INSERT INTO outbox_events (event_id,event_type,aggregate_type,aggregate_id, \
         aggregate_sequence,account_id,correlation_id,causation_id,payload,occurred_at) \
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)",
    )
    .bind(event.event_id)
    .bind(&event.event_type)
    .bind(&event.aggregate_type)
    .bind(event.aggregate_id)
    .bind(event.sequence)
    .bind(event.account_id)
    .bind(event.correlation_id)
    .bind(event.causation_id)
    .bind(serde_json::to_value(event).map_err(ApiError::serialization)?)
    .bind(event.occurred_at)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}
