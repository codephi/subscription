use crate::{
    dto::billing::{
        BillingCapabilitiesResponse, BillingConnectionResponse, CollectionRequestResponse,
        CreateBillingConnectionRequest, CreateInitialCollectionRequest,
        CreateOnDemandPurchaseRequest, CreatePaymentMethodBindingRequest,
        CreatePaymentMethodSetupSessionRequest, CreateRenewalRegularizationRequest,
        PaymentMethodBindingResponse, PaymentMethodSetupSessionResponse,
        UnmatchedPaymentCaseResponse,
    },
    error::{ApiError, ApiResult},
    repositories::{
        billing_attempts::StartedCollectionAttempt,
        billing_confirmation::{ConfirmationOutcome, ConfirmedBillingWebhook},
        billing_connector::{
            BillingConnector, BillingPaymentMethod, CollectionCommand, ConnectorCollectionResult,
            ConnectorCollectionState, SetupSessionCommand,
        },
        billing_expiration::CollectionExpirationSummary,
        database::DatabaseRepository,
    },
};

pub async fn create_initial_collection(
    repository: &DatabaseRepository,
    workspace_id: uuid::Uuid,
    customer_plan_id: uuid::Uuid,
    idempotency_key: &str,
    request: &CreateInitialCollectionRequest,
) -> ApiResult<CollectionRequestResponse> {
    if idempotency_key.is_empty() || request.transaction_id.is_empty() {
        return Err(ApiError::unprocessable(
            "invalid_billing_idempotency",
            format!(
                "collection key {idempotency_key:?} and transaction {:?} must be non-empty",
                request.transaction_id
            ),
        ));
    }
    repository
        .create_initial_collection(workspace_id, customer_plan_id, idempotency_key, request)
        .await
}

pub async fn create_on_demand_purchase(
    repository: &DatabaseRepository,
    workspace_id: uuid::Uuid,
    customer_plan_id: uuid::Uuid,
    idempotency_key: &str,
    request: &CreateOnDemandPurchaseRequest,
) -> ApiResult<CollectionRequestResponse> {
    if idempotency_key.is_empty() || request.transaction_id.is_empty() {
        return Err(ApiError::unprocessable(
            "invalid_billing_idempotency",
            format!(
                "idempotency key {idempotency_key:?} and transaction id {:?} must be non-empty",
                request.transaction_id
            ),
        ));
    }
    repository
        .create_on_demand_purchase(workspace_id, customer_plan_id, idempotency_key, request)
        .await
}

pub async fn create_paid_plan_upgrade(
    repository: &DatabaseRepository,
    workspace_id: uuid::Uuid,
    customer_plan_id: uuid::Uuid,
    idempotency_key: &str,
    request: &crate::dto::plans::CreatePlanTransitionRequest,
) -> ApiResult<CollectionRequestResponse> {
    if idempotency_key.is_empty()
        || request.transaction_id.is_empty()
        || request.actor_reference.is_empty()
    {
        return Err(ApiError::unprocessable(
            "invalid_billing_idempotency",
            format!(
                "upgrade key {idempotency_key:?}, transaction {:?}, and actor {:?} must be non-empty",
                request.transaction_id, request.actor_reference
            ),
        ));
    }
    repository
        .create_paid_plan_upgrade(workspace_id, customer_plan_id, idempotency_key, request)
        .await
}

pub async fn create_payment_method_setup_session(
    repository: &DatabaseRepository,
    connection_id: uuid::Uuid,
    request: &CreatePaymentMethodSetupSessionRequest,
) -> ApiResult<PaymentMethodSetupSessionResponse> {
    if !request.return_url.starts_with("https://") {
        return Err(ApiError::unprocessable(
            "invalid_setup_return_url",
            format!("return_url {:?} must use HTTPS", request.return_url),
        ));
    }
    let configuration = repository
        .billing_connector_configuration(connection_id)
        .await?;
    if configuration.provider != "STRIPE" || configuration.status != "ACTIVE" {
        return Err(ApiError::conflict(
            "billing_connection_not_usable",
            format!("billing connection {connection_id} must be an ACTIVE STRIPE connection"),
        ));
    }
    let secret = resolve_secret(&configuration.secret_reference)?;
    let connector = crate::repositories::stripe::StripeConnector::new(
        secret,
        stripe_account(&configuration.external_account_reference),
    );
    let session = connector
        .create_setup_session(&SetupSessionCommand {
            customer_reference: configuration.external_account_reference,
            return_url: request.return_url.clone(),
        })
        .await
        .map_err(|error| ApiError::external("billing_connector_error", error.to_string()))?;
    Ok(PaymentMethodSetupSessionResponse {
        provider_setup_id: session.provider_setup_id,
        client_secret: session.client_secret,
    })
}

pub(crate) fn resolve_secret(reference: &str) -> ApiResult<String> {
    let variable = reference.strip_prefix("env://").ok_or_else(|| {
        ApiError::unprocessable(
            "invalid_secret_reference",
            format!("secret reference {reference:?} must use env://VARIABLE"),
        )
    })?;
    std::env::var(variable).map_err(|_| {
        ApiError::service_unavailable(
            "billing_secret_unavailable",
            format!("secret reference {reference:?} is not available in this process"),
        )
    })
}

fn stripe_account(reference: &str) -> Option<String> {
    reference
        .starts_with("acct_")
        .then(|| reference.to_string())
}

pub async fn create_billing_connection(
    repository: &DatabaseRepository,
    workspace_id: uuid::Uuid,
    request: &CreateBillingConnectionRequest,
) -> ApiResult<BillingConnectionResponse> {
    if request.provider != "STRIPE" {
        return Err(ApiError::unprocessable(
            "billing_provider_not_supported",
            format!("provider {:?} must be STRIPE in V1", request.provider),
        ));
    }
    validate_reference(
        "external_account_reference",
        &request.external_account_reference,
    )?;
    validate_secret_reference("secret_reference", &request.secret_reference)?;
    validate_secret_reference(
        "webhook_secret_reference",
        &request.webhook_secret_reference,
    )?;
    repository
        .create_billing_connection(workspace_id, request)
        .await
}

pub async fn get_billing_connection(
    repository: &DatabaseRepository,
    workspace_id: uuid::Uuid,
    connection_id: uuid::Uuid,
) -> ApiResult<BillingConnectionResponse> {
    repository
        .find_billing_connection(workspace_id, connection_id)
        .await
}

pub fn billing_capabilities() -> BillingCapabilitiesResponse {
    BillingCapabilitiesResponse {
        payment_methods: vec!["CARD".to_string()],
        supports_setup_session: true,
        supports_vault: true,
        supports_off_session_charge: true,
        supports_webhook: true,
    }
}

pub async fn create_payment_method_binding(
    repository: &DatabaseRepository,
    workspace_id: uuid::Uuid,
    request: &CreatePaymentMethodBindingRequest,
) -> ApiResult<PaymentMethodBindingResponse> {
    repository
        .create_payment_method_binding(workspace_id, request)
        .await
}

pub async fn list_payment_method_bindings(
    repository: &DatabaseRepository,
    workspace_id: uuid::Uuid,
) -> ApiResult<Vec<PaymentMethodBindingResponse>> {
    repository.list_payment_method_bindings(workspace_id).await
}

pub async fn list_unmatched_payments(
    repository: &DatabaseRepository,
    workspace_id: uuid::Uuid,
) -> ApiResult<Vec<UnmatchedPaymentCaseResponse>> {
    repository.list_unmatched_payments(workspace_id).await
}

fn validate_reference(name: &str, reference: &str) -> ApiResult<()> {
    if !reference.trim().is_empty() && reference.len() <= 255 {
        return Ok(());
    }
    Err(ApiError::unprocessable(
        "invalid_billing_reference",
        format!("{name} {reference:?} must contain 1 to 255 characters"),
    ))
}

fn validate_secret_reference(name: &str, reference: &str) -> ApiResult<()> {
    let variable = reference.strip_prefix("env://").unwrap_or_default();
    let mut characters = variable.chars();
    let valid_start = characters
        .next()
        .is_some_and(|character| character.is_ascii_alphabetic() || character == '_');
    if valid_start
        && characters.all(|character| character.is_ascii_alphanumeric() || character == '_')
        && reference.len() <= 255
    {
        return Ok(());
    }
    Err(ApiError::unprocessable(
        "invalid_secret_reference",
        format!("{name} {reference:?} must use env://VARIABLE without containing a secret"),
    ))
}

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

/// Opens or returns the operational case for a confirmed payment without a collection.
pub async fn record_unmatched_payment(
    repository: &DatabaseRepository,
    billing_connection_id: uuid::Uuid,
    webhook: &ConfirmedBillingWebhook,
) -> ApiResult<UnmatchedPaymentCaseResponse> {
    validate_unmatched_webhook(webhook)?;
    let record = repository
        .record_unmatched_payment(billing_connection_id, webhook)
        .await?;
    Ok(unmatched_payment_response(record))
}

fn validate_unmatched_webhook(webhook: &ConfirmedBillingWebhook) -> ApiResult<()> {
    if webhook.event_type == "payment.confirmed"
        && webhook.amount_minor > 0
        && webhook.currency.len() == 3
        && !webhook.provider_payment_id.is_empty()
    {
        return Ok(());
    }
    Err(ApiError::unprocessable(
        "invalid_unmatched_payment",
        format!(
            "provider event {:?} must be payment.confirmed with positive amount, ISO currency, and payment id",
            webhook.provider_event_id
        ),
    ))
}

fn unmatched_payment_response(
    record: crate::repositories::billing_unmatched::UnmatchedPaymentRecord,
) -> UnmatchedPaymentCaseResponse {
    UnmatchedPaymentCaseResponse {
        unmatched_payment_case_id: record.unmatched_payment_case_id,
        workspace_id: record.workspace_id,
        billing_connection_id: record.billing_connection_id,
        provider: record.provider,
        provider_event_id: record.provider_event_id,
        provider_payment_id: record.provider_payment_id,
        amount_minor: record.amount_minor,
        currency: record.currency,
        reason: record.reason,
        status: record.status,
        created_at: record.created_at,
    }
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
#[path = "billing_tests.rs"]
mod tests;
