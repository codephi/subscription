use super::{
    append_lifecycle_if_changed, count_materialized, emit_provisioning_events,
    ensure_customer_wallet, finish_provisioning, load_current_scope, start_provisioning,
    synchronize_wallets, ApiResult, Postgres, Transaction, Uuid, WalletProvisioningResponse,
    WalletStatus,
};

pub(super) async fn reconcile_attempt(
    transaction: &mut Transaction<'_, Postgres>,
    workspace_id: Uuid,
    operational_status: &str,
    correlation_id: Uuid,
    actor_reference: Option<&str>,
) -> ApiResult<WalletProvisioningResponse> {
    // Keep the workspace lock outside the savepoint: concurrent retries must observe
    // the committed failure or recovery, never a partially materialized hierarchy.
    sqlx::query("SAVEPOINT wallet_materialization")
        .execute(&mut **transaction)
        .await?;
    let result = synchronize_wallets(
        transaction,
        workspace_id,
        operational_status,
        correlation_id,
        None,
        actor_reference,
    )
    .await;
    if result.is_err() {
        sqlx::query("ROLLBACK TO SAVEPOINT wallet_materialization")
            .execute(&mut **transaction)
            .await?;
    }
    sqlx::query("RELEASE SAVEPOINT wallet_materialization")
        .execute(&mut **transaction)
        .await?;
    match result {
        Ok(response) => Ok(response),
        Err(error) => {
            record_failure(
                transaction,
                workspace_id,
                correlation_id,
                actor_reference,
                error.code(),
            )
            .await
        }
    }
}

async fn record_failure(
    transaction: &mut Transaction<'_, Postgres>,
    workspace_id: Uuid,
    correlation_id: Uuid,
    actor_reference: Option<&str>,
    error_code: &str,
) -> ApiResult<WalletProvisioningResponse> {
    let (scope, items) = load_current_scope(transaction).await?;
    let known_failure: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM wallet_provisioning WHERE customer_id=$1 AND scope_version=$2 AND status='ERROR')")
        .bind(workspace_id).bind(scope).fetch_one(&mut **transaction).await?;
    start_provisioning(transaction, workspace_id, scope, items.len()).await?;
    let (wallet, _) = ensure_customer_wallet(transaction, workspace_id, scope).await?;
    let changed = append_lifecycle_if_changed(
        transaction,
        wallet,
        WalletStatus::Error,
        "wallet materialization failed",
        correlation_id,
        actor_reference,
    )
    .await?;
    let materialized = count_materialized(transaction, workspace_id, &items).await?;
    let mut response = finish_provisioning(
        transaction,
        workspace_id,
        scope,
        WalletStatus::Error,
        items.len() as i64,
        materialized,
    )
    .await?;
    let detail = format!("workspace {workspace_id}: {error_code}; expected {} complete item wallets, materialized {materialized}", items.len());
    sqlx::query(
        "UPDATE wallet_provisioning SET error_detail=$3 WHERE customer_id=$1 AND scope_version=$2",
    )
    .bind(workspace_id)
    .bind(scope)
    .bind(&detail)
    .execute(&mut **transaction)
    .await?;
    response.error_detail = Some(detail);
    if changed || !known_failure {
        emit_provisioning_events(transaction, &response, correlation_id, None).await?;
    }
    Ok(response)
}
