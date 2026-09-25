use sqlx::Row;

use crate::{
    dto::audit_admin::{AuditEventResponse, AuditPageQuery, AuditPageResponse},
    error::ApiResult,
    repositories::database::DatabaseRepository,
};

impl DatabaseRepository {
    /// Page audit events by stable ID; e.g. `repo.list_audit_events(query, 20).await`.
    pub async fn list_audit_events(
        &self,
        query: AuditPageQuery,
        limit: i64,
    ) -> ApiResult<AuditPageResponse> {
        let rows = sqlx::query("SELECT audit_event_id,workspace_id,actor_reference,action,resource_kind,resource_id,correlation_id,details,occurred_at FROM audit_events WHERE ($1::uuid IS NULL OR audit_event_id>$1) AND ($2::uuid IS NULL OR workspace_id=$2) AND ($3::text IS NULL OR action=$3) ORDER BY audit_event_id LIMIT $4")
            .bind(query.cursor).bind(query.workspace_id).bind(query.action).bind(limit + 1)
            .fetch_all(&self.pool()).await?;
        let has_more = rows.len() as i64 > limit;
        let items = rows
            .iter()
            .take(limit as usize)
            .map(|row| AuditEventResponse {
                audit_event_id: row.get("audit_event_id"),
                workspace_id: row.get("workspace_id"),
                actor_reference: row.get("actor_reference"),
                action: row.get("action"),
                resource_kind: row.get("resource_kind"),
                resource_id: row.get("resource_id"),
                correlation_id: row.get("correlation_id"),
                details: row.get("details"),
                occurred_at: row.get("occurred_at"),
            })
            .collect::<Vec<_>>();
        let next_cursor = has_more
            .then(|| items.last().map(|item| item.audit_event_id))
            .flatten();
        Ok(AuditPageResponse { items, next_cursor })
    }
}
