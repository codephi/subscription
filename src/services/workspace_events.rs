use crate::{
    dto::events::{WorkspaceEventEnvelope, WorkspaceEventResponse},
    error::{ApiError, ApiResult},
    repositories::database::DatabaseRepository,
};

pub async fn process_workspace_event(
    repository: &DatabaseRepository,
    event: WorkspaceEventEnvelope,
) -> ApiResult<WorkspaceEventResponse> {
    validate_event(&event)?;
    repository.apply_workspace_event(&event).await
}

fn validate_event(event: &WorkspaceEventEnvelope) -> ApiResult<()> {
    if event.schema_version != 1 {
        return Err(ApiError::unprocessable(
            "unsupported_event_schema",
            format!("schema_version {} must equal 1", event.schema_version),
        ));
    }
    if event.sequence < 1 {
        return Err(ApiError::unprocessable(
            "invalid_event_sequence",
            format!("sequence {} must be at least 1", event.sequence),
        ));
    }
    if event.aggregate_id != event.workspace_id || event.payload.workspace_id != event.workspace_id
    {
        return Err(ApiError::unprocessable(
            "workspace_context_mismatch",
            format!(
                "event workspace {} must match aggregate and payload",
                event.workspace_id
            ),
        ));
    }
    Ok(())
}
