use std::env;
use uuid::Uuid;

use crate::{
    dto::{
        billing::{
            CreateInitialCollectionRequest, CreateOnDemandPurchaseRequest,
            CreatePaymentMethodBindingRequest, UpdateStripeIntegrationRequest,
        },
        checkouts::{
            checkout_request_hash, CheckoutQuoteRequest, CheckoutQuoteResponse, CheckoutResponse,
            CreateCheckoutRequest,
        },
    },
    error::{ApiError, ApiResult},
    repositories::{
        billing_checkouts::{CheckoutClaim, CheckoutRecord},
        database::DatabaseRepository,
        stripe::StripeConnector,
    },
    services::billing_integrations,
};

#[derive(Clone)]
pub struct BillingCheckoutConfig {
    pub(crate) api_secret: String,
    pub(crate) webhook_secret: String,
    pub(crate) declines_charge: bool,
}

impl BillingCheckoutConfig {
    pub fn from_environment() -> ApiResult<Option<Self>> {
        if env::var("BILLING_SANDBOX_ENABLED").ok().as_deref() != Some("true") {
            return Ok(None);
        }
        let api_secret = required_test_value("STRIPE_SECRET_KEY", "sk_test_")?;
        let webhook_secret = required_test_value("STRIPE_WEBHOOK_SECRET", "whsec_")?;
        let declines_charge = match env::var("BILLING_SANDBOX_PAYMENT_SCENARIO").as_deref() {
            Ok("APPROVED") => false,
            Ok("DECLINED") => true,
            _ => return Err(invalid_scenario()),
        };
        Ok(Some(Self {
            api_secret,
            webhook_secret,
            declines_charge,
        }))
    }
}

pub async fn create(
    repository: &DatabaseRepository,
    config: Option<&BillingCheckoutConfig>,
    workspace_id: Uuid,
    idempotency_key: &str,
    request: CreateCheckoutRequest,
) -> ApiResult<CheckoutResponse> {
    validate_request(&request, idempotency_key)?;
    if request.payment_method_binding_id.is_none() && request.coupon_code.is_none() {
        required_config(config)?;
    }
    let request_hash = checkout_request_hash(&request);
    let claim = repository
        .claim_checkout(workspace_id, idempotency_key, &request, &request_hash)
        .await?;
    create_or_resume(repository, config, claim, request).await
}

pub async fn get(
    repository: &DatabaseRepository,
    workspace_id: Uuid,
    checkout_id: Uuid,
) -> ApiResult<CheckoutResponse> {
    let record = repository.checkout(checkout_id, Some(workspace_id)).await?;
    response_for_record(repository, record).await
}

pub async fn quote(
    repository: &DatabaseRepository,
    workspace_id: Uuid,
    request: CheckoutQuoteRequest,
) -> ApiResult<CheckoutQuoteResponse> {
    if request.coupon_code.trim().is_empty() || request.coupon_code.len() > 64 {
        return Err(ApiError::unprocessable(
            "invalid_coupon_code",
            format!(
                "coupon code {:?} must contain 1 to 64 characters",
                request.coupon_code
            ),
        ));
    }
    repository.quote_checkout(workspace_id, &request).await
}

async fn create_or_resume(
    repository: &DatabaseRepository,
    config: Option<&BillingCheckoutConfig>,
    claim: CheckoutClaim,
    request: CreateCheckoutRequest,
) -> ApiResult<CheckoutResponse> {
    match claim {
        CheckoutClaim::Existing(record) => response_for_record(repository, record).await,
        CheckoutClaim::Busy(record) => Ok(pending_response(record)),
        CheckoutClaim::Claimed(record) => process_claim(repository, config, record, request).await,
    }
}

async fn process_claim(
    repository: &DatabaseRepository,
    config: Option<&BillingCheckoutConfig>,
    record: CheckoutRecord,
    request: CreateCheckoutRequest,
) -> ApiResult<CheckoutResponse> {
    let checkout_id = record.checkout_id;
    let result = process_claim_inner(repository, config, record, request).await;
    if result.is_err() {
        let _ = repository.release_checkout(checkout_id).await;
    }
    result
}

async fn process_claim_inner(
    repository: &DatabaseRepository,
    config: Option<&BillingCheckoutConfig>,
    record: CheckoutRecord,
    request: CreateCheckoutRequest,
) -> ApiResult<CheckoutResponse> {
    if request.coupon_code.is_some() {
        let quote = repository
            .quote_checkout(
                record.workspace_id,
                &CheckoutQuoteRequest {
                    customer_plan_id: request.customer_plan_id,
                    checkout_kind: request.checkout_kind,
                    on_demand_plan_id: request.on_demand_plan_id,
                    coupon_code: request.coupon_code.clone().unwrap_or_default(),
                },
            )
            .await?;
        if !quote.payment_required {
            repository.complete_free_checkout(&record, &request).await?;
            return response_for_record(
                repository,
                repository
                    .checkout(record.checkout_id, Some(record.workspace_id))
                    .await?,
            )
            .await;
        }
    }
    match begin_collection(repository, config, &record, &request).await {
        Ok(collection) => {
            repository
                .finish_checkout(record.checkout_id, collection.collection_request_id)
                .await?;
            response_for_record(
                repository,
                repository
                    .checkout(record.checkout_id, Some(record.workspace_id))
                    .await?,
            )
            .await
        }
        Err(error) => Err(error),
    }
}

async fn begin_collection(
    repository: &DatabaseRepository,
    config: Option<&BillingCheckoutConfig>,
    record: &CheckoutRecord,
    request: &CreateCheckoutRequest,
) -> ApiResult<crate::dto::billing::CollectionRequestResponse> {
    let binding = prepare_payment_method(repository, config, record).await?;
    create_collection(
        repository,
        record,
        request,
        binding.payment_method_binding_id,
    )
    .await
}

async fn prepare_payment_method(
    repository: &DatabaseRepository,
    config: Option<&BillingCheckoutConfig>,
    record: &CheckoutRecord,
) -> ApiResult<crate::dto::billing::PaymentMethodBindingResponse> {
    if let Some(binding_id) = record.payment_method_binding_id {
        return repository
            .find_payment_method_binding_by_id(record.workspace_id, binding_id)
            .await;
    }
    let config = required_config(config)?;
    let connector = StripeConnector::new(config.api_secret.clone(), None);
    let account_id = connector.identify_account().await.map_err(stripe_error)?;
    let integration =
        ensure_integration(repository, config, record.workspace_id, &account_id).await?;
    let secrets = repository
        .integration_secrets(record.workspace_id, integration.billing_connection_id)
        .await?;
    let customer_id = billing_integrations::ensure_customer(repository, &secrets).await?;
    let prepared = connector
        .prepare_test_payment_method(
            &customer_id,
            &format!("checkout:{}:payment-method:v1", record.checkout_id),
            config.declines_charge,
        )
        .await
        .map_err(stripe_error)?;
    if prepared.customer_id != customer_id {
        return Err(ApiError::conflict(
            "billing_payment_method_customer_mismatch",
            format!(
                "checkout {} received a payment method for another customer",
                record.checkout_id
            ),
        ));
    }
    find_or_create_binding(
        repository,
        record,
        integration.billing_connection_id,
        &prepared.payment_method_id,
    )
    .await
}

async fn ensure_integration(
    repository: &DatabaseRepository,
    config: &BillingCheckoutConfig,
    workspace_id: Uuid,
    account_id: &str,
) -> ApiResult<crate::dto::billing::WorkspaceIntegrationResponse> {
    let current = repository
        .find_test_stripe_integration(workspace_id, account_id)
        .await?;
    let integration = match current {
        Some(integration) => integration,
        None => create_integration(repository, workspace_id, account_id, config).await?,
    };
    if integration_is_ready(&integration) {
        return Ok(integration);
    }
    configure_integration(repository, workspace_id, &integration, config).await
}

async fn create_integration(
    repository: &DatabaseRepository,
    workspace_id: Uuid,
    account_id: &str,
    config: &BillingCheckoutConfig,
) -> ApiResult<crate::dto::billing::WorkspaceIntegrationResponse> {
    repository
        .create_stripe_integration(workspace_id, account_id, "TEST", None, &config.api_secret)
        .await
}

async fn configure_integration(
    repository: &DatabaseRepository,
    workspace_id: Uuid,
    integration: &crate::dto::billing::WorkspaceIntegrationResponse,
    config: &BillingCheckoutConfig,
) -> ApiResult<crate::dto::billing::WorkspaceIntegrationResponse> {
    let configured = billing_integrations::update_stripe(
        repository,
        workspace_id,
        integration.billing_connection_id,
        &UpdateStripeIntegrationRequest {
            expected_version: integration.configuration_version,
            secret_key: None,
            webhook_secret: Some(config.webhook_secret.clone()),
        },
    )
    .await?;
    if integration_is_ready(&configured) {
        return Ok(configured);
    }
    repository
        .activate_stripe_integration(
            workspace_id,
            configured.billing_connection_id,
            configured.configuration_version,
        )
        .await
}

fn integration_is_ready(integration: &crate::dto::billing::WorkspaceIntegrationResponse) -> bool {
    let active = integration.status == "ACTIVE";
    active && integration.webhook_secret_configured
}

async fn find_or_create_binding(
    repository: &DatabaseRepository,
    checkout: &CheckoutRecord,
    connection_id: Uuid,
    payment_method_id: &str,
) -> ApiResult<crate::dto::billing::PaymentMethodBindingResponse> {
    if let Some(binding) = repository
        .find_payment_method_binding(checkout.workspace_id, connection_id, payment_method_id)
        .await?
    {
        return Ok(binding);
    }
    repository
        .create_payment_method_binding(
            checkout.workspace_id,
            &CreatePaymentMethodBindingRequest {
                billing_connection_id: connection_id,
                customer_plan_id: Some(checkout.customer_plan_id),
                provider_payment_method_reference: payment_method_id.to_string(),
            },
        )
        .await
}

async fn create_collection(
    repository: &DatabaseRepository,
    checkout: &CheckoutRecord,
    request: &CreateCheckoutRequest,
    binding_id: Uuid,
) -> ApiResult<crate::dto::billing::CollectionRequestResponse> {
    let coupon = request
        .coupon_code
        .as_deref()
        .map(|code| (checkout.checkout_id, code));
    match request.checkout_kind {
        crate::dto::checkouts::CheckoutKind::Initial => {
            repository
                .create_initial_collection_for_checkout(
                    checkout.workspace_id,
                    checkout.customer_plan_id,
                    &checkout.idempotency_key,
                    &CreateInitialCollectionRequest {
                        payment_method_binding_id: binding_id,
                        transaction_id: checkout.transaction_id.clone(),
                    },
                    coupon,
                )
                .await
        }
        crate::dto::checkouts::CheckoutKind::OnDemand => {
            repository
                .create_on_demand_purchase_for_checkout(
                    checkout.workspace_id,
                    checkout.customer_plan_id,
                    &checkout.idempotency_key,
                    &CreateOnDemandPurchaseRequest {
                        on_demand_plan_id: request.on_demand_plan_id.expect("validated offer"),
                        payment_method_binding_id: binding_id,
                        transaction_id: checkout.transaction_id.clone(),
                    },
                    coupon,
                )
                .await
        }
    }
}

async fn response_for_record(
    repository: &DatabaseRepository,
    record: CheckoutRecord,
) -> ApiResult<CheckoutResponse> {
    if record.completed_without_payment {
        let credits = repository.checkout_credit_units(&record).await?;
        return Ok(CheckoutResponse {
            checkout_id: record.checkout_id,
            customer_plan_id: record.customer_plan_id,
            checkout_kind: record.checkout_kind,
            status: "COMPLETED".to_string(),
            collection_request_id: None,
            amount_minor: Some(0),
            currency: record.checkout_currency,
            granted_credit_units: Some(credits),
            transaction_id: record.transaction_id,
            created_at: record.created_at,
            base_amount_minor: record.base_amount_minor,
            discount_amount_minor: record.discount_amount_minor,
            coupon_code: record.coupon_code,
            payment_required: false,
        });
    }
    if let Some(collection_id) = record.collection_request_id {
        let collection = repository
            .find_collection_request(record.workspace_id, collection_id)
            .await?;
        return Ok(repository.response_for_checkout(record, Some(&collection)));
    }
    Ok(pending_response(record))
}

fn pending_response(record: CheckoutRecord) -> CheckoutResponse {
    CheckoutResponse {
        checkout_id: record.checkout_id,
        customer_plan_id: record.customer_plan_id,
        checkout_kind: record.checkout_kind,
        status: "PENDING".to_string(),
        collection_request_id: None,
        amount_minor: None,
        currency: None,
        granted_credit_units: None,
        transaction_id: record.transaction_id,
        created_at: record.created_at,
        base_amount_minor: record.base_amount_minor,
        discount_amount_minor: record.discount_amount_minor,
        coupon_code: record.coupon_code,
        payment_required: true,
    }
}

fn validate_request(request: &CreateCheckoutRequest, key: &str) -> ApiResult<()> {
    let valid = match request.checkout_kind {
        crate::dto::checkouts::CheckoutKind::Initial => request.on_demand_plan_id.is_none(),
        crate::dto::checkouts::CheckoutKind::OnDemand => request.on_demand_plan_id.is_some(),
    };
    let valid_transaction = (1..=255).contains(&request.transaction_id.len())
        && request.transaction_id.is_ascii()
        && !request.transaction_id.trim().is_empty();
    let valid_key = (1..=255).contains(&key.len()) && key.is_ascii();
    if valid
        && valid_transaction
        && valid_key
        && request.coupon_code.as_deref().is_none_or(valid_coupon_code)
    {
        return Ok(());
    }
    Err(ApiError::unprocessable(
        "invalid_checkout_request",
        format!(
            "checkout transaction {:?}, kind {:?}, and offer {:?} have an invalid shape",
            request.transaction_id, request.checkout_kind, request.on_demand_plan_id
        ),
    ))
}

fn valid_coupon_code(code: &str) -> bool {
    let value = code.trim().to_ascii_uppercase();
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || matches!(c, b'-' | b'_'))
}

fn required_config(config: Option<&BillingCheckoutConfig>) -> ApiResult<&BillingCheckoutConfig> {
    config.ok_or_else(|| {
        ApiError::service_unavailable(
            "billing_checkout_disabled",
            "transparent checkout is not enabled for this environment",
        )
    })
}

fn required_test_value(name: &str, prefix: &str) -> ApiResult<String> {
    let value = env::var(name).unwrap_or_default();
    if value.starts_with(prefix) {
        return Ok(value);
    }
    Err(ApiError::service_unavailable(
        "billing_checkout_configuration_invalid",
        format!("{name} must start with {prefix}"),
    ))
}

fn invalid_scenario() -> ApiError {
    ApiError::service_unavailable(
        "billing_checkout_configuration_invalid",
        "BILLING_SANDBOX_PAYMENT_SCENARIO must be APPROVED or DECLINED",
    )
}

fn stripe_error(error: impl std::fmt::Display) -> ApiError {
    ApiError::external("billing_connector_error", error.to_string())
}

#[cfg(test)]
mod tests {
    use super::{integration_is_ready, validate_request};
    use crate::dto::{
        billing::WorkspaceIntegrationResponse,
        checkouts::{CheckoutKind, CreateCheckoutRequest},
    };
    use uuid::Uuid;

    #[test]
    fn checkout_request_requires_offer_and_bounded_ascii_operation_ids() {
        let mut request = CreateCheckoutRequest {
            customer_plan_id: Uuid::new_v4(),
            checkout_kind: CheckoutKind::Initial,
            on_demand_plan_id: None,
            transaction_id: "operation-1".to_string(),
            coupon_code: None,
            payment_method_binding_id: None,
        };
        assert!(validate_request(&request, "key-1").is_ok());
        request.on_demand_plan_id = Some(Uuid::new_v4());
        assert!(validate_request(&request, "key-1").is_err());
        request.checkout_kind = CheckoutKind::OnDemand;
        assert!(validate_request(&request, "key-1").is_ok());
        request.transaction_id = "ação".to_string();
        assert!(validate_request(&request, "key-1").is_err());
    }

    #[test]
    fn configured_checkout_integration_is_not_activated_twice() {
        let mut integration = WorkspaceIntegrationResponse {
            billing_connection_id: Uuid::new_v4(),
            provider: "STRIPE".into(),
            account_reference: "acct_test".into(),
            environment: "TEST".into(),
            status: "PENDING_SETUP".into(),
            api_secret_configured: true,
            webhook_secret_configured: false,
            customer_reference: None,
            webhook_path: "/v1/billing/webhooks/test".into(),
            webhook_url: None,
            configuration_version: 1,
        };
        assert!(!integration_is_ready(&integration));
        integration.status = "ACTIVE".into();
        integration.webhook_secret_configured = true;
        assert!(integration_is_ready(&integration));
    }
}
