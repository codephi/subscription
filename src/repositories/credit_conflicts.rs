use sqlx::{Postgres, Row, Transaction};
use uuid::Uuid;

use crate::{
    dto::idempotency::ExistingOperationReference,
    error::{ApiError, ApiResult},
};

pub(super) async fn attach_committed_operation(
    transaction: &mut Transaction<'_, Postgres>,
    workspace_id: Uuid,
    identifier: &str,
    by_key: bool,
    conflict: ApiError,
) -> ApiResult<ApiError> {
    let row = sqlx::query(include_str!("credit_conflicts.sql"))
        .bind(workspace_id)
        .bind(identifier)
        .bind(by_key)
        .fetch_optional(&mut **transaction)
        .await?;
    let Some(row) = row else {
        return Ok(conflict);
    };
    Ok(conflict.with_existing_operation(conflict_reference(row, workspace_id)))
}

fn conflict_reference(
    row: sqlx::postgres::PgRow,
    workspace_id: Uuid,
) -> ExistingOperationReference {
    ExistingOperationReference {
        workspace_id,
        operation_kind: row.get("operation_kind"),
        resource_id: row.get("resource_id"),
        transaction_id: row.get("transaction_id"),
    }
}
