use uuid::Uuid;

use crate::{
    dto::events::AccountEventResponse, error::ApiResult, repositories::database::DatabaseRepository,
};

pub async fn replay_account_event(
    repository: &DatabaseRepository,
    event_id: Uuid,
) -> ApiResult<AccountEventResponse> {
    repository.replay_account_event(event_id).await
}

pub async fn replay_outbox_event(repository: &DatabaseRepository, event_id: Uuid) -> ApiResult<()> {
    repository.replay_dead_letter(event_id).await
}
