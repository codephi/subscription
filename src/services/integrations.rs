use uuid::Uuid;

use crate::{
    dto::events::WorkspaceEventResponse, error::ApiResult,
    repositories::database::DatabaseRepository,
};

pub async fn replay_workspace_event(
    repository: &DatabaseRepository,
    event_id: Uuid,
) -> ApiResult<WorkspaceEventResponse> {
    repository.replay_workspace_event(event_id).await
}

pub async fn replay_outbox_event(repository: &DatabaseRepository, event_id: Uuid) -> ApiResult<()> {
    repository.replay_dead_letter(event_id).await
}
