use sqlx::{Postgres, Row, Transaction};
use uuid::Uuid;

use crate::{
    dto::{
        credits::{
            CreditLedgerReconciliationResponse, CustomerWalletStatementResponse,
            DirectCreditRequest, DirectCreditResponse, PendingUsageTransactionResponse,
            UpdateWorkspaceBillingConfigRequest, WorkspaceBillingConfigResponse,
            WorkspaceTransactionResponse,
        },
        units::{CreditUnits, ItemUnitBoundary, ItemUnits},
    },
    error::{ApiError, ApiResult},
    repositories::{
        credit_rows::{billing_config_from_row, entry_from_row, entry_not_found, load_references},
        credit_writes::{
            complete_reservations, insert_credit_audit, insert_credit_lot, insert_credit_outbox,
            insert_direct_credit_row, insert_entry, insert_references, update_wallet_balance,
        },
        database::DatabaseRepository,
    },
};

impl DatabaseRepository {
    pub async fn insert_direct_credit(
        &self,
        workspace_id: Uuid,
        idempotency_key: &str,
        request_hash: &str,
        request: &DirectCreditRequest,
    ) -> ApiResult<DirectCreditResponse> {
        let mut transaction = self.pool().begin().await?;
        let wallet = lock_active_customer_wallet(&mut transaction, workspace_id).await?;
        ensure_direct_credit_enabled(&mut transaction, workspace_id).await?;
        reserve_idempotency(
            &mut transaction,
            workspace_id,
            idempotency_key,
            request_hash,
            "DIRECT_CREDIT",
        )
        .await?;
        reserve_transaction(
            &mut transaction,
            workspace_id,
            &request.transaction_id,
            "DIRECT_CREDIT",
        )
        .await?;
        let balance_after = wallet.balance.checked_add(request.credit_units)?;
        let direct_credit_id = Uuid::new_v4();
        let entry_id = Uuid::new_v4();
        let lot_id = Uuid::new_v4();
        insert_direct_credit_row(&mut transaction, workspace_id, direct_credit_id, request).await?;
        let entry_row = insert_entry(
            &mut transaction,
            &wallet,
            workspace_id,
            entry_id,
            balance_after,
            request,
        )
        .await?;
        update_wallet_balance(&mut transaction, wallet.wallet_id, balance_after).await?;
        insert_credit_lot(
            &mut transaction,
            workspace_id,
            lot_id,
            entry_id,
            request.credit_units,
        )
        .await?;
        insert_references(
            &mut transaction,
            entry_id,
            direct_credit_id,
            lot_id,
            request.external_reference.as_deref(),
        )
        .await?;
        complete_reservations(
            &mut transaction,
            workspace_id,
            idempotency_key,
            &request.transaction_id,
            direct_credit_id,
        )
        .await?;
        insert_credit_outbox(
            &mut transaction,
            workspace_id,
            wallet.wallet_id,
            wallet.next_sequence,
            entry_id,
            request.credit_units,
        )
        .await?;
        insert_credit_audit(&mut transaction, workspace_id, direct_credit_id, entry_id).await?;
        transaction.commit().await?;
        let references = load_references(&self.pool(), entry_id).await?;
        Ok(DirectCreditResponse {
            direct_credit_id,
            credit_lot_id: lot_id,
            entry: entry_from_row(&entry_row, references),
        })
    }

    pub async fn find_customer_wallet_transaction(
        &self,
        workspace_id: Uuid,
        transaction_id: &str,
    ) -> ApiResult<WorkspaceTransactionResponse> {
        let row = sqlx::query(
            "SELECT * FROM customer_wallet_entries WHERE customer_id=$1 AND transaction_id=$2",
        )
        .bind(workspace_id)
        .bind(transaction_id)
        .fetch_optional(&self.pool())
        .await?;
        if let Some(row) = row {
            let entry_id = row.get("customer_wallet_entry_id");
            let references = load_references(&self.pool(), entry_id).await?;
            return Ok(WorkspaceTransactionResponse::CustomerWalletEntry(
                entry_from_row(&row, references),
            ));
        }
        self.find_pending_usage_transaction(workspace_id, transaction_id)
            .await
    }

    async fn find_pending_usage_transaction(
        &self,
        workspace_id: Uuid,
        transaction_id: &str,
    ) -> ApiResult<WorkspaceTransactionResponse> {
        let row = sqlx::query(
            "SELECT u.transaction_id,u.usage_event_id,u.item_wallet_id,u.product_id,u.item_id, \
             u.item_units,u.pending_item_units_after,u.metadata,u.accepted_at,e.item_wallet_entry_id \
             FROM usage_events u JOIN item_wallet_entries e ON e.usage_event_id=u.usage_event_id \
             WHERE u.customer_id=$1 AND u.transaction_id=$2 AND NOT EXISTS \
             (SELECT 1 FROM debits d WHERE d.usage_event_id=u.usage_event_id)",
        )
        .bind(workspace_id)
        .bind(transaction_id)
        .fetch_optional(&self.pool())
        .await?
        .ok_or_else(|| entry_not_found(workspace_id, transaction_id))?;
        Ok(WorkspaceTransactionResponse::PendingUsage(
            PendingUsageTransactionResponse {
                transaction_id: row.get("transaction_id"),
                usage_event_id: row.get("usage_event_id"),
                item_wallet_entry_id: row.get("item_wallet_entry_id"),
                item_wallet_id: row.get("item_wallet_id"),
                product_id: row.get("product_id"),
                item_id: row.get("item_id"),
                received_item_units: ItemUnits::positive(row.get("item_units"))?,
                pending_item_units_after: ItemUnitBoundary::non_negative(
                    row.get("pending_item_units_after"),
                )?,
                metadata: row.get("metadata"),
                accepted_at: row.get("accepted_at"),
            },
        ))
    }

    pub async fn list_customer_wallet_entries(
        &self,
        workspace_id: Uuid,
        cursor: Option<i64>,
        limit: i64,
    ) -> ApiResult<CustomerWalletStatementResponse> {
        let rows = sqlx::query(
            "SELECT e.* FROM customer_wallet_entries e JOIN wallets w \
             ON w.wallet_id=e.customer_wallet_id WHERE w.customer_id=$1 \
             AND ($2::bigint IS NULL OR e.entry_sequence<$2) \
             ORDER BY e.entry_sequence DESC LIMIT $3",
        )
        .bind(workspace_id)
        .bind(cursor)
        .bind(limit + 1)
        .fetch_all(&self.pool())
        .await?;
        let has_more = rows.len() as i64 > limit;
        let mut items = Vec::with_capacity(rows.len().min(limit as usize));
        for row in rows.iter().take(limit as usize) {
            let entry_id = row.get("customer_wallet_entry_id");
            items.push(entry_from_row(
                row,
                load_references(&self.pool(), entry_id).await?,
            ));
        }
        let next_cursor = has_more
            .then(|| items.last().map(|entry| entry.sequence.to_string()))
            .flatten();
        Ok(CustomerWalletStatementResponse { items, next_cursor })
    }

    pub async fn reconcile_credit_ledger(
        &self,
        workspace_id: Uuid,
    ) -> ApiResult<CreditLedgerReconciliationResponse> {
        let row = sqlx::query(
            "SELECT cw.balance_credit_units, \
             COALESCE((SELECT e.balance_after_credit_units FROM customer_wallet_entries e \
               WHERE e.customer_wallet_id=cw.wallet_id ORDER BY e.entry_sequence DESC LIMIT 1),0) ledger_balance, \
             COALESCE((SELECT sum(l.remaining_credit_units)::bigint FROM credit_lots l \
               WHERE l.customer_id=w.customer_id),0) lot_balance \
             FROM wallets w JOIN customer_wallets cw ON cw.wallet_id=w.wallet_id \
             WHERE w.customer_id=$1 AND w.wallet_type='CUSTOMER'",
        )
        .bind(workspace_id)
        .fetch_optional(&self.pool())
        .await?
        .ok_or_else(|| ApiError::service_unavailable(
            "wallet_not_provisioned",
            format!("workspace {workspace_id} has no customer wallet"),
        ))?;
        let wallet_balance: i64 = row.get("balance_credit_units");
        let ledger_balance: i64 = row.get("ledger_balance");
        let lot_balance: i64 = row.get("lot_balance");
        Ok(CreditLedgerReconciliationResponse {
            workspace_id,
            wallet_balance_credit_units: CreditUnits::new(wallet_balance),
            ledger_balance_credit_units: CreditUnits::new(ledger_balance),
            available_lot_credit_units: CreditUnits::new(lot_balance),
            consistent: wallet_balance == ledger_balance && wallet_balance == lot_balance,
        })
    }

    pub async fn find_billing_config(
        &self,
        workspace_id: Uuid,
    ) -> ApiResult<WorkspaceBillingConfigResponse> {
        let row = sqlx::query("SELECT * FROM workspace_billing_configs WHERE workspace_id=$1")
            .bind(workspace_id)
            .fetch_optional(&self.pool())
            .await?
            .ok_or_else(|| {
                ApiError::not_found(
                    "billing_config_not_found",
                    format!("workspace {workspace_id} has no billing config"),
                )
            })?;
        Ok(billing_config_from_row(&row))
    }

    pub async fn update_billing_config(
        &self,
        workspace_id: Uuid,
        request: &UpdateWorkspaceBillingConfigRequest,
    ) -> ApiResult<WorkspaceBillingConfigResponse> {
        let row = sqlx::query(
            "UPDATE workspace_billing_configs SET direct_credit_enabled=$2, \
             recurring_credit_enabled=$3,version=version+1 WHERE workspace_id=$1 AND version=$4 \
             RETURNING *",
        )
        .bind(workspace_id)
        .bind(request.direct_credit_enabled)
        .bind(request.recurring_credit_enabled)
        .bind(request.expected_version)
        .fetch_optional(&self.pool())
        .await?
        .ok_or_else(|| {
            ApiError::conflict(
                "billing_config_version_conflict",
                format!(
                    "workspace {workspace_id} billing config must be version {}",
                    request.expected_version
                ),
            )
        })?;
        Ok(billing_config_from_row(&row))
    }
}

#[derive(Clone)]
pub(super) struct LockedWallet {
    pub(super) wallet_id: Uuid,
    pub(super) balance: CreditUnits,
    pub(super) next_sequence: i64,
}

pub(super) async fn lock_active_customer_wallet(
    transaction: &mut Transaction<'_, Postgres>,
    workspace_id: Uuid,
) -> ApiResult<LockedWallet> {
    let workspace = sqlx::query(
        "SELECT operational_status,EXISTS(SELECT 1 FROM integration_inbox_quarantine q \
         JOIN integration_inbox i ON i.event_id=q.event_id WHERE i.workspace_id=$1 \
         AND q.replayed_at IS NULL) has_gap FROM workspace_projections WHERE workspace_id=$1 FOR UPDATE",
    )
    .bind(workspace_id)
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or_else(|| ApiError::not_found("workspace_not_found", format!("workspace {workspace_id} does not exist")))?;
    let status: String = workspace.get("operational_status");
    if status != "ACTIVE" || workspace.get::<bool, _>("has_gap") {
        return Err(ApiError::conflict(
            "workspace_not_operational",
            format!("workspace {workspace_id} must be ACTIVE without event gaps, found {status}"),
        ));
    }
    let row = sqlx::query(
        "SELECT cw.wallet_id,cw.balance_credit_units FROM customer_wallets cw \
         JOIN wallets w ON w.wallet_id=cw.wallet_id JOIN wallet_effective_states s ON s.wallet_id=w.wallet_id \
         JOIN catalog_scope_current c ON c.singleton JOIN wallet_provisioning p \
           ON p.customer_id=w.customer_id AND p.scope_version=c.scope_version \
         WHERE w.customer_id=$1 AND s.status='ACTIVE' AND p.status='ACTIVE' \
           AND p.expected_item_wallets=p.materialized_item_wallets \
         FOR UPDATE OF cw",
    )
    .bind(workspace_id)
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or_else(|| ApiError::service_unavailable(
        "wallet_not_provisioned",
        format!("workspace {workspace_id} wallet hierarchy is not active for the current scope"),
    ))?;
    super::wallets::wallet_readiness::ensure_hierarchy_ready(&mut **transaction, workspace_id)
        .await?;
    let wallet_id = row.get("wallet_id");
    let next_sequence = sqlx::query_scalar(
        "SELECT COALESCE(max(entry_sequence),0)+1 FROM customer_wallet_entries \
         WHERE customer_wallet_id=$1",
    )
    .bind(wallet_id)
    .fetch_one(&mut **transaction)
    .await?;
    Ok(LockedWallet {
        wallet_id,
        balance: CreditUnits::new(row.get("balance_credit_units")),
        next_sequence,
    })
}

async fn ensure_direct_credit_enabled(
    transaction: &mut Transaction<'_, Postgres>,
    workspace_id: Uuid,
) -> ApiResult<()> {
    let direct_enabled: bool = sqlx::query_scalar(
        "SELECT direct_credit_enabled FROM workspace_billing_configs WHERE workspace_id=$1",
    )
    .bind(workspace_id)
    .fetch_one(&mut **transaction)
    .await?;
    if direct_enabled {
        return Ok(());
    }
    Err(ApiError::conflict(
        "direct_credit_disabled",
        format!("workspace {workspace_id} has direct credits disabled"),
    ))
}

pub(super) async fn reserve_idempotency(
    transaction: &mut Transaction<'_, Postgres>,
    workspace_id: Uuid,
    key: &str,
    request_hash: &str,
    operation_kind: &str,
) -> ApiResult<()> {
    let inserted = sqlx::query(
        "INSERT INTO idempotency_records (workspace_id,idempotency_key,operation_kind,request_hash) \
         VALUES ($1,$2,$3,$4) ON CONFLICT DO NOTHING",
    )
    .bind(workspace_id)
    .bind(key)
    .bind(operation_kind)
    .bind(request_hash)
    .execute(&mut **transaction)
    .await?;
    if inserted.rows_affected() == 1 {
        return Ok(());
    }
    Err(ApiError::conflict(
        "idempotency_key_already_used",
        format!("Idempotency-Key {key:?} was already used in workspace {workspace_id}"),
    ))
}

pub(super) async fn reserve_transaction(
    transaction: &mut Transaction<'_, Postgres>,
    workspace_id: Uuid,
    transaction_id: &str,
    operation_kind: &str,
) -> ApiResult<()> {
    let inserted = sqlx::query(
        "INSERT INTO transaction_reservations (workspace_id,transaction_id,operation_kind) \
         VALUES ($1,$2,$3) ON CONFLICT DO NOTHING",
    )
    .bind(workspace_id)
    .bind(transaction_id)
    .bind(operation_kind)
    .execute(&mut **transaction)
    .await?;
    if inserted.rows_affected() == 1 {
        return Ok(());
    }
    Err(ApiError::conflict(
        "transaction_already_exists",
        format!("transaction_id {transaction_id:?} already exists in workspace {workspace_id}"),
    ))
}
