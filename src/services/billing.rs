use chrono::Datelike;

use crate::{
    dto::billing::{
        BillingCapabilitiesResponse, BillingConnectionResponse, CollectionRequestResponse,
        CreateBillingConnectionRequest, CreateInitialCollectionRequest,
        CreateOnDemandPurchaseRequest, CreatePaymentMethodBindingRequest,
        CreatePaymentMethodFromCardRequest, CreatePaymentMethodFromCardResponse,
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
    workspace_id: uuid::Uuid,
    connection_id: uuid::Uuid,
    request: &CreatePaymentMethodSetupSessionRequest,
) -> ApiResult<PaymentMethodSetupSessionResponse> {
    let configuration = repository
        .billing_connector_configuration(connection_id)
        .await?;
    if configuration.workspace_id != workspace_id
        || configuration.provider != "STRIPE"
        || configuration.status != "ACTIVE"
    {
        return Err(ApiError::conflict(
            "billing_connection_not_usable",
            format!("billing connection {connection_id} must be an ACTIVE STRIPE connection"),
        ));
    }
    let secret = resolve_connection_secret(
        repository,
        configuration.workspace_id,
        connection_id,
        "stripe_api",
        &configuration.secret_reference,
        configuration.managed,
    )?;
    let customer_reference = match configuration.managed {
        true => {
            crate::services::billing_integrations::ensure_customer(
                repository,
                &repository
                    .integration_secrets(configuration.workspace_id, connection_id)
                    .await?,
            )
            .await?
        }
        false => configuration.external_account_reference.clone(),
    };
    repository
        .ensure_customer_plan_workspace(workspace_id, request.customer_plan_id)
        .await?;
    validate_setup_return_urls(&request.success_url, &request.cancel_url)?;
    let payment_method_setup_id = uuid::Uuid::new_v4();
    let success_url = setup_success_return_url(&request.success_url, payment_method_setup_id)?;
    let connector = crate::repositories::stripe::StripeConnector::new(
        secret,
        (!configuration.managed)
            .then(|| stripe_account(&configuration.external_account_reference))
            .flatten(),
    );
    let session = connector
        .create_setup_session(&SetupSessionCommand {
            customer_reference,
            client_reference_id: request.customer_plan_id.to_string(),
            billing_connection_id: connection_id.to_string(),
            success_url,
            cancel_url: request.cancel_url.clone(),
        })
        .await
        .map_err(|error| ApiError::external("billing_connector_error", error.to_string()))?;
    repository
        .record_payment_method_setup_session(
            payment_method_setup_id,
            workspace_id,
            connection_id,
            request.customer_plan_id,
            &session.provider_setup_id,
        )
        .await?;
    Ok(PaymentMethodSetupSessionResponse {
        payment_method_setup_id,
        redirect_url: session.redirect_url,
    })
}

pub async fn create_workspace_payment_method_setup_session(
    repository: &DatabaseRepository,
    workspace_id: uuid::Uuid,
    request: &CreatePaymentMethodSetupSessionRequest,
) -> ApiResult<PaymentMethodSetupSessionResponse> {
    let connection_id = repository
        .active_stripe_billing_connection(workspace_id)
        .await?;
    create_payment_method_setup_session(repository, workspace_id, connection_id, request).await
}

pub(crate) fn resolve_connection_secret(
    repository: &DatabaseRepository,
    workspace_id: uuid::Uuid,
    connection_id: uuid::Uuid,
    purpose: &str,
    reference: &str,
    managed: bool,
) -> ApiResult<String> {
    if managed {
        return repository.credential_vault()?.open(
            workspace_id,
            connection_id,
            purpose,
            reference,
        );
    }
    resolve_secret(reference)
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
    let setup = repository
        .find_payment_method_setup_session(
            workspace_id,
            request.customer_plan_id,
            request.payment_method_setup_id,
        )
        .await?;
    let connection_id = setup.billing_connection_id;
    let configuration = repository
        .billing_connector_configuration(connection_id)
        .await?;
    if configuration.workspace_id != workspace_id
        || configuration.provider != "STRIPE"
        || configuration.status != "ACTIVE"
    {
        return Err(ApiError::conflict(
            "billing_connection_not_usable",
            format!("billing connection {connection_id} must be an ACTIVE STRIPE connection for workspace {workspace_id}"),
        ));
    }
    let customer_plan_id = request.customer_plan_id;
    repository
        .ensure_customer_plan_workspace(workspace_id, customer_plan_id)
        .await?;
    let secrets = if configuration.managed {
        Some(
            repository
                .integration_secrets(workspace_id, connection_id)
                .await?,
        )
    } else {
        None
    };
    let secret = resolve_connection_secret(
        repository,
        workspace_id,
        connection_id,
        "stripe_api",
        &configuration.secret_reference,
        configuration.managed,
    )?;
    let expected_customer = match secrets.as_ref() {
        Some(secrets) => {
            crate::services::billing_integrations::ensure_customer(repository, secrets).await?
        }
        None => configuration.external_account_reference.clone(),
    };
    let connector = crate::repositories::stripe::StripeConnector::new(
        secret,
        (!configuration.managed)
            .then(|| stripe_account(&configuration.external_account_reference))
            .flatten(),
    );
    let prepared = connector
        .retrieve_checkout_setup_intent(
            &setup.provider_setup_id,
            &expected_customer,
            &customer_plan_id.to_string(),
            &connection_id.to_string(),
        )
        .await
        .map_err(|error| ApiError::external("billing_connector_error", error.to_string()))?;
    if prepared.customer_id != expected_customer {
        return Err(ApiError::conflict(
            "billing_setup_customer_mismatch",
            format!(
                "SetupIntent {} belongs to customer {}, expected {expected_customer}",
                prepared.setup_intent_id, prepared.customer_id
            ),
        ));
    }
    repository
        .create_verified_payment_method_binding(
            workspace_id,
            connection_id,
            Some(customer_plan_id),
            &prepared.payment_method_id,
        )
        .await
}

pub async fn create_payment_method_from_card(
    repository: &DatabaseRepository,
    workspace_id: uuid::Uuid,
    idempotency_key: &str,
    request: &CreatePaymentMethodFromCardRequest,
) -> ApiResult<CreatePaymentMethodFromCardResponse> {
    validate_card_entry(request)?;
    repository
        .ensure_customer_plan_workspace(workspace_id, request.customer_plan_id)
        .await?;
    let context = card_setup_context(repository, workspace_id).await?;
    let prepared = confirm_card_setup(&context, request, idempotency_key).await?;
    finish_card_setup(repository, workspace_id, request, &context, &prepared).await
}

struct CardSetupContext {
    connection_id: uuid::Uuid,
    connector: crate::repositories::stripe::StripeConnector,
    customer_id: String,
}

async fn card_setup_context(
    repository: &DatabaseRepository,
    workspace_id: uuid::Uuid,
) -> ApiResult<CardSetupContext> {
    let connection_id = repository
        .active_stripe_billing_connection(workspace_id)
        .await?;
    let configuration = repository
        .billing_connector_configuration(connection_id)
        .await?;
    let customer_id =
        card_setup_customer(repository, workspace_id, connection_id, &configuration).await?;
    let connector = card_setup_connector(repository, workspace_id, connection_id, &configuration)?;
    Ok(CardSetupContext {
        connection_id,
        connector,
        customer_id,
    })
}

async fn card_setup_customer(
    repository: &DatabaseRepository,
    workspace_id: uuid::Uuid,
    connection_id: uuid::Uuid,
    configuration: &crate::repositories::billing_connections::BillingConnectorConfiguration,
) -> ApiResult<String> {
    if !configuration.managed {
        return Ok(configuration.external_account_reference.clone());
    }
    let secrets = repository
        .integration_secrets(workspace_id, connection_id)
        .await?;
    crate::services::billing_integrations::ensure_customer(repository, &secrets).await
}

fn card_setup_connector(
    repository: &DatabaseRepository,
    workspace_id: uuid::Uuid,
    connection_id: uuid::Uuid,
    configuration: &crate::repositories::billing_connections::BillingConnectorConfiguration,
) -> ApiResult<crate::repositories::stripe::StripeConnector> {
    let secret = resolve_connection_secret(
        repository,
        workspace_id,
        connection_id,
        "stripe_api",
        &configuration.secret_reference,
        configuration.managed,
    )?;
    validate_raw_card_secret(&secret)?;
    Ok(crate::repositories::stripe::StripeConnector::new(
        secret,
        (!configuration.managed)
            .then(|| stripe_account(&configuration.external_account_reference))
            .flatten(),
    ))
}

fn validate_raw_card_secret(secret: &str) -> ApiResult<()> {
    if secret.starts_with("sk_test_") {
        return Ok(());
    }
    Err(ApiError::conflict(
        "raw_card_setup_sandbox_only",
        "direct card entry requires a Stripe test secret beginning with sk_test_",
    ))
}

async fn confirm_card_setup(
    context: &CardSetupContext,
    request: &CreatePaymentMethodFromCardRequest,
    idempotency_key: &str,
) -> ApiResult<crate::repositories::stripe::PreparedStripePaymentMethod> {
    let card_number: String = request
        .card_number
        .chars()
        .filter(char::is_ascii_digit)
        .collect();
    context
        .connector
        .create_card_setup_intent(
            &context.customer_id,
            request.cardholder_name.trim(),
            &card_number,
            request.exp_month,
            request.exp_year,
            &request.cvc,
            idempotency_key,
        )
        .await
        .map_err(|error| ApiError::external("billing_connector_error", error.to_string()))
}

async fn finish_card_setup(
    repository: &DatabaseRepository,
    workspace_id: uuid::Uuid,
    request: &CreatePaymentMethodFromCardRequest,
    context: &CardSetupContext,
    prepared: &crate::repositories::stripe::PreparedStripePaymentMethod,
) -> ApiResult<CreatePaymentMethodFromCardResponse> {
    if request.save_for_future {
        return save_card_binding(repository, workspace_id, request, context, prepared).await;
    }
    context
        .connector
        .detach_payment_method(&prepared.payment_method_id)
        .await
        .map_err(|error| ApiError::external("billing_connector_error", error.to_string()))?;
    Ok(CreatePaymentMethodFromCardResponse {
        payment_method_binding_id: None,
        saved: false,
    })
}

async fn save_card_binding(
    repository: &DatabaseRepository,
    workspace_id: uuid::Uuid,
    request: &CreatePaymentMethodFromCardRequest,
    context: &CardSetupContext,
    prepared: &crate::repositories::stripe::PreparedStripePaymentMethod,
) -> ApiResult<CreatePaymentMethodFromCardResponse> {
    let display_name = card_display_name(request);
    let binding = repository
        .create_verified_payment_method_binding_with_name(
            workspace_id,
            context.connection_id,
            Some(request.customer_plan_id),
            &prepared.payment_method_id,
            Some(&display_name),
        )
        .await?;
    Ok(CreatePaymentMethodFromCardResponse {
        payment_method_binding_id: Some(binding.payment_method_binding_id),
        saved: true,
    })
}

fn validate_card_entry(request: &CreatePaymentMethodFromCardRequest) -> ApiResult<()> {
    validate_cardholder_name(&request.cardholder_name)?;
    validate_card_display_name(request)?;
    validate_card_number(&request.card_number)?;
    validate_card_security_code(&request.cvc)?;
    validate_card_expiry(request.exp_month, request.exp_year)
}

fn validate_card_display_name(request: &CreatePaymentMethodFromCardRequest) -> ApiResult<()> {
    let Some(name) = request
        .card_name
        .as_deref()
        .map(str::trim)
        .filter(|name| !name.is_empty())
    else {
        return Ok(());
    };
    let has_control_character = name.chars().any(char::is_control);
    if name.chars().count() <= 50 && !has_control_character {
        return Ok(());
    }
    Err(ApiError::unprocessable(
        "invalid_card_display_name",
        format!("card_name {name:?} must contain at most 50 characters without control characters"),
    ))
}

fn card_display_name(request: &CreatePaymentMethodFromCardRequest) -> String {
    let name = request
        .card_name
        .as_deref()
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .unwrap_or("Cartão");
    let last_four: String = request
        .card_number
        .chars()
        .filter(char::is_ascii_digit)
        .rev()
        .take(4)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    format!("{name} •••• {last_four}")
}

fn validate_cardholder_name(name: &str) -> ApiResult<()> {
    if name.trim().is_empty() || name.len() > 100 {
        return Err(ApiError::unprocessable(
            "invalid_cardholder_name",
            "cardholder_name must contain 1 to 100 characters",
        ));
    }
    Ok(())
}

fn validate_card_number(number: &str) -> ApiResult<()> {
    let digits = number.chars().filter(char::is_ascii_digit).count();
    let allowed_characters = number
        .chars()
        .all(|character| character.is_ascii_digit() || character == ' ' || character == '-');
    if allowed_characters && (12..=19).contains(&digits) {
        return Ok(());
    }
    Err(ApiError::unprocessable(
        "invalid_card_number",
        "card_number must contain 12 to 19 digits",
    ))
}

fn validate_card_security_code(cvc: &str) -> ApiResult<()> {
    let valid_cvc =
        (3..=4).contains(&cvc.len()) && cvc.chars().all(|character| character.is_ascii_digit());
    if valid_cvc {
        return Ok(());
    }
    Err(ApiError::unprocessable(
        "invalid_card_security_code",
        "cvc must contain 3 to 4 digits",
    ))
}

fn validate_card_expiry(month: u8, year: u16) -> ApiResult<()> {
    if !(1..=12).contains(&month) {
        return Err(ApiError::unprocessable(
            "invalid_card_expiry_month",
            format!("exp_month {month} must be between 1 and 12"),
        ));
    }
    let today = chrono::Utc::now();
    let expiry_is_past =
        year < today.year() as u16 || (year == today.year() as u16 && month < today.month() as u8);
    if expiry_is_past {
        return Err(ApiError::unprocessable(
            "card_expired",
            format!(
                "card expiry {:02}/{} must be in the current month or future",
                month, year
            ),
        ));
    }
    Ok(())
}

fn validate_setup_return_urls(success_url: &str, cancel_url: &str) -> ApiResult<()> {
    let success = url::Url::parse(success_url)
        .map_err(|error| invalid_setup_return_url(success_url, error))?;
    let cancel =
        url::Url::parse(cancel_url).map_err(|error| invalid_setup_return_url(cancel_url, error))?;
    let same_origin = success.scheme() == cancel.scheme()
        && success.host_str() == cancel.host_str()
        && success.port_or_known_default() == cancel.port_or_known_default();
    let secure = success.scheme() == "https"
        || (success.scheme() == "http" && success.host_str().is_some_and(is_local_host));
    if same_origin && secure && success.fragment().is_none() && cancel.fragment().is_none() {
        return Ok(());
    }
    Err(ApiError::unprocessable(
        "invalid_payment_setup_return_urls",
        "success_url and cancel_url must share a secure origin and must not contain fragments",
    ))
}

fn setup_success_return_url(success_url: &str, setup_id: uuid::Uuid) -> ApiResult<String> {
    let mut url = url::Url::parse(success_url)
        .map_err(|error| invalid_setup_return_url(success_url, error))?;
    if url
        .query_pairs()
        .any(|(key, _)| key == "payment_method_setup_id")
    {
        return Err(ApiError::unprocessable(
            "invalid_payment_setup_return_url",
            "success_url must not predefine payment_method_setup_id",
        ));
    }
    url.query_pairs_mut()
        .append_pair("payment_method_setup_id", &setup_id.to_string());
    Ok(url.to_string())
}

fn is_local_host(host: &str) -> bool {
    matches!(host, "localhost" | "127.0.0.1" | "::1")
}

fn invalid_setup_return_url(url: &str, error: url::ParseError) -> ApiError {
    ApiError::unprocessable(
        "invalid_payment_setup_return_url",
        format!("return URL {url:?} is invalid: {error}"),
    )
}

pub async fn list_payment_method_bindings(
    repository: &DatabaseRepository,
    workspace_id: uuid::Uuid,
) -> ApiResult<Vec<PaymentMethodBindingResponse>> {
    repository.list_payment_method_bindings(workspace_id).await
}

pub async fn remove_payment_method_binding(
    repository: &DatabaseRepository,
    workspace_id: uuid::Uuid,
    binding_id: uuid::Uuid,
) -> ApiResult<()> {
    let binding = repository
        .find_payment_method_binding_for_removal(workspace_id, binding_id)
        .await?;
    if binding.binding.status == "DETACHED" {
        return Ok(());
    }
    ensure_binding_can_be_detached(&binding.binding)?;
    detach_provider_payment_method(repository, workspace_id, &binding).await?;
    repository
        .mark_payment_method_binding_detached(workspace_id, binding_id)
        .await
}

fn ensure_binding_can_be_detached(binding: &PaymentMethodBindingResponse) -> ApiResult<()> {
    if binding.status == "ACTIVE" {
        return Ok(());
    }
    Err(ApiError::conflict(
        "payment_method_binding_inactive",
        format!(
            "payment method binding {} is {}",
            binding.payment_method_binding_id, binding.status
        ),
    ))
}

async fn detach_provider_payment_method(
    repository: &DatabaseRepository,
    workspace_id: uuid::Uuid,
    binding: &crate::repositories::billing_connections::PaymentMethodBindingRemoval,
) -> ApiResult<()> {
    let configuration = repository
        .billing_connector_configuration(binding.binding.billing_connection_id)
        .await?;
    validate_binding_connection(workspace_id, &binding.binding, &configuration)?;
    let connector = payment_method_removal_connector(repository, workspace_id, &configuration)?;
    connector
        .detach_payment_method(&binding.provider_payment_method_reference)
        .await
        .map_err(|error| ApiError::external("billing_connector_error", error.to_string()))
}

fn validate_binding_connection(
    workspace_id: uuid::Uuid,
    binding: &PaymentMethodBindingResponse,
    configuration: &crate::repositories::billing_connections::BillingConnectorConfiguration,
) -> ApiResult<()> {
    if configuration.workspace_id == workspace_id && configuration.provider == "STRIPE" {
        return Ok(());
    }
    Err(ApiError::conflict(
        "billing_connection_not_usable",
        format!(
            "billing connection {} must belong to workspace {workspace_id} and use STRIPE",
            binding.billing_connection_id
        ),
    ))
}

fn payment_method_removal_connector(
    repository: &DatabaseRepository,
    workspace_id: uuid::Uuid,
    configuration: &crate::repositories::billing_connections::BillingConnectorConfiguration,
) -> ApiResult<crate::repositories::stripe::StripeConnector> {
    let secret = resolve_connection_secret(
        repository,
        workspace_id,
        configuration.billing_connection_id,
        "stripe_api",
        &configuration.secret_reference,
        configuration.managed,
    )?;
    Ok(crate::repositories::stripe::StripeConnector::new(
        secret,
        (!configuration.managed)
            .then(|| stripe_account(&configuration.external_account_reference))
            .flatten(),
    ))
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
