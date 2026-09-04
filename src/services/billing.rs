use crate::{
    error::{ApiError, ApiResult},
    repositories::{
        billing_attempts::StartedCollectionAttempt,
        billing_connector::{
            BillingConnector, BillingPaymentMethod, CollectionCommand, ConnectorCollectionResult,
            ConnectorCollectionState,
        },
        database::DatabaseRepository,
    },
};

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
