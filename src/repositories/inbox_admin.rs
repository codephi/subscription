use sqlx::Row;

use crate::{
    dto::inbox_admin::{InboxEventResponse, InboxPageQuery, InboxPageResponse},
    error::ApiResult,
    repositories::database::DatabaseRepository,
};

impl DatabaseRepository {
    /// Page integration inbox metadata by UUID; e.g. `repo.list_inbox_events(query, 20).await`.
    pub async fn list_inbox_events(
        &self,
        query: InboxPageQuery,
        limit: i64,
    ) -> ApiResult<InboxPageResponse> {
        let rows = sqlx::query("SELECT event_id,workspace_id,event_type,external_sequence,processing_status,correlation_id,received_at,processed_at FROM integration_inbox WHERE ($1::uuid IS NULL OR event_id>$1) AND ($2::uuid IS NULL OR workspace_id=$2) AND ($3::text IS NULL OR processing_status=$3) ORDER BY event_id LIMIT $4")
            .bind(query.cursor).bind(query.workspace_id).bind(query.status).bind(limit + 1)
            .fetch_all(&self.pool()).await?;
        let has_more = rows.len() as i64 > limit;
        let items = rows
            .iter()
            .take(limit as usize)
            .map(|row| InboxEventResponse {
                event_id: row.get("event_id"),
                workspace_id: row.get("workspace_id"),
                event_type: row.get("event_type"),
                external_sequence: row.get("external_sequence"),
                processing_status: row.get("processing_status"),
                correlation_id: row.get("correlation_id"),
                received_at: row.get("received_at"),
                processed_at: row.get("processed_at"),
            })
            .collect::<Vec<_>>();
        let next_cursor = has_more
            .then(|| items.last().map(|item| item.event_id))
            .flatten();
        Ok(InboxPageResponse { items, next_cursor })
    }
}
