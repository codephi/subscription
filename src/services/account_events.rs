use crate::{
    dto::events::{AccountEventEnvelope, AccountEventResponse, AccountEventType},
    error::{ApiError, ApiResult},
    repositories::database::DatabaseRepository,
};

pub async fn process_account_event(
    repository: &DatabaseRepository,
    event: AccountEventEnvelope,
) -> ApiResult<AccountEventResponse> {
    validate_event(&event)?;
    let response = repository.apply_account_event(&event).await?;
    if matches!(event.event_type, AccountEventType::Created) {
        crate::services::billing_integrations::provision_account_defaults(
            repository,
            event.account_id,
        )
        .await?;
    }
    Ok(response)
}

fn validate_event(event: &AccountEventEnvelope) -> ApiResult<()> {
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
    if event.aggregate_id != event.account_id || event.payload.account_id != event.account_id {
        return Err(ApiError::unprocessable(
            "account_context_mismatch",
            format!(
                "event account {} must match aggregate and payload",
                event.account_id
            ),
        ));
    }
    Ok(())
}
