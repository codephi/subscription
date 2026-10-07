use sqlx::{Executor, Postgres};
use uuid::Uuid;

use crate::{error::ApiResult, repositories::wallet_rows::wallet_not_provisioned};

// PH-03: a fingerprint can revisit an old scope whose provisioning counters
// predate a wallet deactivation. Readiness must prove the current hierarchy.
const HIERARCHY_READY: &str = include_str!("wallet_readiness.sql");

pub(in crate::repositories) async fn hierarchy_is_ready<'a, E>(
    executor: E,
    account_id: Uuid,
) -> ApiResult<bool>
where
    E: Executor<'a, Database = Postgres>,
{
    Ok(sqlx::query_scalar(HIERARCHY_READY)
        .bind(account_id)
        .fetch_one(executor)
        .await?)
}

pub(in crate::repositories) async fn ensure_hierarchy_ready<'a, E>(
    executor: E,
    account_id: Uuid,
) -> ApiResult<()>
where
    E: Executor<'a, Database = Postgres>,
{
    if hierarchy_is_ready(executor, account_id).await? {
        return Ok(());
    }
    Err(wallet_not_provisioned(account_id))
}
