use crate::{
    dto::billing::{CollectionRequestResponse, CreateRenewalRegularizationRequest},
    error::{ApiError, ApiResult},
    repositories::{
        billing_attempts::StartedCollectionAttempt,
        billing_confirmation::{ConfirmationOutcome, ConfirmedBillingWebhook},
        billing_connector::{
            BillingConnector, BillingPaymentMethod, CollectionCommand, ConnectorCollectionResult,
            ConnectorCollectionState,
        },
        billing_expiration::CollectionExpirationSummary,
        database::DatabaseRepository,
    },
};

use crate::services::calendar::cycle_end;

/// Validates connector capability and starts one logical collection attempt.
pub async fn start_collection(
    connector: &dyn BillingConnector,
    command: &CollectionCommand,
) -> ApiResult<ConnectorCollectionResult> {
    validate_collection_command(connector, command)?;
    connector
        .start_collection(command)
        .await
        .map_err(|error| ApiError::external("billing_connector_error", error.to_string()))
}

pub async fn execute_collection_attempt(
    repository: &DatabaseRepository,
    connector: &dyn BillingConnector,
    collection_request_id: uuid::Uuid,
) -> ApiResult<Option<ConnectorCollectionResult>> {
    validate_connector_capabilities(connector, BillingPaymentMethod::Card)?;
    let Some(attempt) = repository
        .begin_collection_attempt(collection_request_id)
        .await?
    else {
        return Ok(None);
    };
    execute_started_attempt(repository, connector, &attempt).await
}

pub async fn apply_confirmed_webhook(
    repository: &DatabaseRepository,
    webhook: &ConfirmedBillingWebhook,
) -> ApiResult<ConfirmationOutcome> {
    let (recurrence, anchor_at, cycle_ordinal) = repository
        .confirmation_schedule(webhook.collection_request_id, webhook.occurred_at)
        .await?;
    let period_end = cycle_end(anchor_at, recurrence, cycle_ordinal)?;
    repository
        .apply_payment_confirmation(webhook, period_end)
        .await
}

/// Creates or returns the manual collection used to recover a past-due plan.
pub async fn create_renewal_regularization(
    repository: &DatabaseRepository,
    workspace_id: uuid::Uuid,
    customer_plan_id: uuid::Uuid,
    idempotency_key: &str,
    request: &CreateRenewalRegularizationRequest,
) -> ApiResult<CollectionRequestResponse> {
    if idempotency_key.is_empty() || request.transaction_id.is_empty() {
        return Err(ApiError::unprocessable(
            "invalid_billing_idempotency",
            format!(
                "idempotency key {:?} and transaction id {:?} must be non-empty",
                idempotency_key, request.transaction_id
            ),
        ));
    }
    repository
        .create_renewal_regularization(workspace_id, customer_plan_id, idempotency_key, request)
        .await
}

/// Applies the commercial payment deadline without contacting the provider.
///
/// Pass the scheduler's current time as `as_of`; repeated calls are idempotent.
pub async fn expire_collections(
    repository: &DatabaseRepository,
    as_of: chrono::DateTime<chrono::Utc>,
) -> ApiResult<CollectionExpirationSummary> {
    repository.expire_due_collections(as_of).await
}

async fn execute_started_attempt(
    repository: &DatabaseRepository,
    connector: &dyn BillingConnector,
    attempt: &StartedCollectionAttempt,
) -> ApiResult<Option<ConnectorCollectionResult>> {
    match connector.start_collection(&attempt.command).await {
        Ok(result) => {
            repository
                .record_collection_result(attempt, &result)
                .await?;
            Ok(Some(result))
        }
        Err(error) => {
            record_connector_error(repository, attempt, &error).await?;
            Err(ApiError::external(
                "billing_connector_error",
                error.to_string(),
            ))
        }
    }
}

async fn record_connector_error(
    repository: &DatabaseRepository,
    attempt: &StartedCollectionAttempt,
    error: &crate::repositories::billing_connector::BillingConnectorError,
) -> ApiResult<()> {
    let state = if error.outcome_uncertain {
        ConnectorCollectionState::Uncertain
    } else {
        ConnectorCollectionState::Failed
    };
    repository
        .record_collection_result(
            attempt,
            &ConnectorCollectionResult {
                provider_payment_id: None,
                state,
                failure_code: Some(error.code.clone()),
                next_action_url: None,
            },
        )
        .await
}

fn validate_collection_command(
    connector: &dyn BillingConnector,
    command: &CollectionCommand,
) -> ApiResult<()> {
    if command.amount_minor <= 0 {
        return Err(ApiError::unprocessable(
            "invalid_collection_amount",
            format!("amount_minor {} must be positive", command.amount_minor),
        ));
    }
    validate_connector_capabilities(connector, command.payment_method)
}

fn validate_connector_capabilities(
    connector: &dyn BillingConnector,
    payment_method: BillingPaymentMethod,
) -> ApiResult<()> {
    let capabilities = connector.capabilities();
    if capabilities.payment_methods.contains(&payment_method)
        && capabilities.supports_vault
        && capabilities.supports_off_session_charge
        && capabilities.supports_webhook
    {
        return Ok(());
    }
    Err(ApiError::unprocessable(
        "billing_capability_not_supported",
        format!(
            "payment method {:?} is not supported by the selected billing connector",
            payment_method
        ),
    ))
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;
    use crate::repositories::billing_connector::{
        BillingCapabilities, BillingPaymentMethod, ConnectorCollectionState, ConnectorFuture,
    };

    struct FakeBillingConnector {
        capabilities: BillingCapabilities,
        calls: Arc<Mutex<Vec<CollectionCommand>>>,
    }

    impl BillingConnector for FakeBillingConnector {
        fn capabilities(&self) -> BillingCapabilities {
            self.capabilities.clone()
        }

        fn start_collection<'a>(&'a self, command: &'a CollectionCommand) -> ConnectorFuture<'a> {
            let calls = Arc::clone(&self.calls);
            let command = command.clone();
            Box::pin(async move {
                calls.lock().expect("fake calls lock").push(command);
                Ok(ConnectorCollectionResult {
                    provider_payment_id: Some("fake-payment-1".to_string()),
                    state: ConnectorCollectionState::Pending,
                    failure_code: None,
                    next_action_url: None,
                })
            })
        }
    }

    fn command(amount_minor: i64) -> CollectionCommand {
        CollectionCommand {
            provider_idempotency_key: "collection-request-1:1".to_string(),
            payment_method: BillingPaymentMethod::Card,
            payment_method_reference: "pm_fake".to_string(),
            amount_minor,
            currency: "BRL".to_string(),
        }
    }

    fn billing_capabilities(methods: Vec<BillingPaymentMethod>) -> BillingCapabilities {
        BillingCapabilities {
            payment_methods: methods,
            supports_setup_session: true,
            supports_vault: true,
            supports_off_session_charge: true,
            supports_webhook: true,
        }
    }

    #[tokio::test]
    async fn fake_billing_connector_receives_stable_collection_command() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let connector = FakeBillingConnector {
            capabilities: billing_capabilities(vec![BillingPaymentMethod::Card]),
            calls: Arc::clone(&calls),
        };
        let result = start_collection(&connector, &command(1_500))
            .await
            .expect("fake collection");
        assert_eq!(result.state, ConnectorCollectionState::Pending);
        assert_eq!(calls.lock().expect("fake calls lock").len(), 1);
    }

    #[tokio::test]
    async fn unsupported_capability_and_invalid_amount_skip_external_call() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let connector = FakeBillingConnector {
            capabilities: billing_capabilities(Vec::new()),
            calls: Arc::clone(&calls),
        };
        let capability = start_collection(&connector, &command(1_500))
            .await
            .expect_err("unsupported card");
        assert_eq!(capability.code(), "billing_capability_not_supported");
        let amount = start_collection(&connector, &command(0))
            .await
            .expect_err("zero amount");
        assert_eq!(amount.code(), "invalid_collection_amount");
        assert!(calls.lock().expect("fake calls lock").is_empty());
    }
}
