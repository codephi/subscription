use std::env;
use uuid::Uuid;

use crate::{
    dto::{
        billing::{
            CreateInitialCollectionRequest, CreateOnDemandPurchaseRequest,
            UpdateStripeIntegrationRequest,
        },
        checkouts::{
            checkout_request_hash, CheckoutQuoteRequest, CheckoutQuoteResponse, CheckoutResponse,
            CreateCheckoutRequest,
        },
        plans::{CreatePlanTransitionRequest, PlanTransitionKind},
    },
    error::{ApiError, ApiResult},
    repositories::{
        billing_checkouts::{CheckoutClaim, CheckoutRecord},
        billing_connector::{BillingConnector, HostedPaymentSessionCommand},
        database::DatabaseRepository,
        stripe::StripeConnector,
    },
    services::billing_integrations,
};

#[derive(Clone)]
pub struct BillingCheckoutConfig {
    pub(crate) api_secret: String,
    pub(crate) webhook_secret: String,
}

impl BillingCheckoutConfig {
    pub fn from_environment() -> ApiResult<Option<Self>> {
        if env::var("BILLING_SANDBOX_ENABLED").ok().as_deref() != Some("true") {
            return Ok(None);
        }
        let api_secret = required_test_value("STRIPE_SECRET_KEY", "sk_test_")?;
        let webhook_secret = required_test_value("STRIPE_WEBHOOK_SECRET", "whsec_")?;
        Ok(Some(Self {
            api_secret,
            webhook_secret,
        }))
    }
}

pub async fn create(
    repository: &DatabaseRepository,
    config: Option<&BillingCheckoutConfig>,
    account_id: Uuid,
    idempotency_key: &str,
    request: CreateCheckoutRequest,
) -> ApiResult<CheckoutResponse> {
    validate_request(&request, idempotency_key)?;
    if request.payment_method_binding_id.is_none() {
        validate_return_urls(&request)?;
    }
    if request.payment_method_binding_id.is_none() && request.coupon_code.is_none() {
        required_config(config)?;
    }
    let request_hash = checkout_request_hash(&request);
    let claim = repository
        .claim_checkout(account_id, idempotency_key, &request, &request_hash)
        .await?;
    create_or_resume(repository, config, claim, request).await
}

pub async fn get(
    repository: &DatabaseRepository,
    account_id: Uuid,
    checkout_id: Uuid,
) -> ApiResult<CheckoutResponse> {
    let record = repository.checkout(checkout_id, Some(account_id)).await?;
    response_for_record(repository, record).await
}

pub async fn quote(
    repository: &DatabaseRepository,
    account_id: Uuid,
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
    repository.quote_checkout(account_id, &request).await
}

async fn create_or_resume(
    repository: &DatabaseRepository,
    config: Option<&BillingCheckoutConfig>,
    claim: CheckoutClaim,
    request: CreateCheckoutRequest,
) -> ApiResult<CheckoutResponse> {
    match claim {
        CheckoutClaim::Existing(record) => {
            resume_hosted_checkout(repository, config, &record, &request).await?;
            response_for_record(repository, record).await
        }
        CheckoutClaim::Busy(record) => Ok(pending_response(record)),
        CheckoutClaim::Claimed(record) => process_claim(repository, config, record, request).await,
    }
}

async fn resume_hosted_checkout(
    repository: &DatabaseRepository,
    config: Option<&BillingCheckoutConfig>,
    record: &CheckoutRecord,
    request: &CreateCheckoutRequest,
) -> ApiResult<()> {
    let Some(recovery) = repository
        .recoverable_hosted_checkout(record.checkout_id)
        .await?
    else {
        return Ok(());
    };
    let secrets = repository
        .integration_secrets(record.account_id, recovery.billing_connection_id)
        .await?;
    let customer_id = billing_integrations::ensure_customer(repository, &secrets).await?;
    create_hosted_session(
        repository,
        config,
        record,
        &recovery.collection,
        request,
        &HostedCheckout { customer_id },
    )
    .await
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
                record.account_id,
                &CheckoutQuoteRequest {
                    customer_plan_id: request.customer_plan_id,
                    checkout_kind: request.checkout_kind,
                    on_demand_plan_id: request.on_demand_plan_id,
                    target_plan_version_id: request.target_plan_version_id,
                    quantity: request.quantity,
                    coupon_code: request.coupon_code.clone().unwrap_or_default(),
                },
            )
            .await?;
        if !quote.payment_required {
            repository.complete_free_checkout(&record, &request).await?;
            return response_for_record(
                repository,
                repository
                    .checkout(record.checkout_id, Some(record.account_id))
                    .await?,
            )
            .await;
        }
    }
    match begin_collection(repository, config, &record, &request).await {
        Ok((collection, hosted)) => {
            repository
                .finish_checkout(record.checkout_id, collection.collection_request_id)
                .await?;
            if let Some(hosted) = hosted {
                repository
                    .mark_hosted_collection_pending(
                        record.checkout_id,
                        collection.collection_request_id,
                    )
                    .await?;
                let collection = repository
                    .hosted_checkout_collection(record.checkout_id)
                    .await?;
                create_hosted_session(repository, config, &record, &collection, &request, &hosted)
                    .await?;
            }
            response_for_record(
                repository,
                repository
                    .checkout(record.checkout_id, Some(record.account_id))
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
) -> ApiResult<(
    crate::dto::billing::CollectionRequestResponse,
    Option<HostedCheckout>,
)> {
    let (binding, hosted) = prepare_payment_method(repository, config, record).await?;
    let collection = create_collection(
        repository,
        record,
        request,
        binding.payment_method_binding_id,
    )
    .await?;
    Ok((collection, hosted))
}

async fn prepare_payment_method(
    repository: &DatabaseRepository,
    config: Option<&BillingCheckoutConfig>,
    record: &CheckoutRecord,
) -> ApiResult<(
    crate::dto::billing::PaymentMethodBindingResponse,
    Option<HostedCheckout>,
)> {
    if let Some(binding_id) = record.payment_method_binding_id {
        return repository
            .find_payment_method_binding_by_id(record.account_id, binding_id)
            .await
            .map(|binding| (binding, None));
    }
    let config = required_config(config)?;
    let connector = StripeConnector::new(config.api_secret.clone(), None);
    let stripe_account_id = connector
        .identify_stripe_account()
        .await
        .map_err(stripe_error)?;
    let integration =
        ensure_integration(repository, config, record.account_id, &stripe_account_id).await?;
    let secrets = repository
        .integration_secrets(record.account_id, integration.billing_connection_id)
        .await?;
    let customer_id = billing_integrations::ensure_customer(repository, &secrets).await?;
    let binding_id = repository
        .ensure_hosted_payment_binding(
            record.checkout_id,
            record.account_id,
            integration.billing_connection_id,
        )
        .await?;
    let binding = repository
        .find_payment_method_binding_by_id(record.account_id, binding_id)
        .await?;
    Ok((binding, Some(HostedCheckout { customer_id })))
}

async fn create_hosted_session(
    repository: &DatabaseRepository,
    config: Option<&BillingCheckoutConfig>,
    checkout: &CheckoutRecord,
    collection: &crate::dto::billing::CollectionRequestResponse,
    request: &CreateCheckoutRequest,
    hosted: &HostedCheckout,
) -> ApiResult<()> {
    let config = required_config(config)?;
    let connector = StripeConnector::new(config.api_secret.clone(), None);
    let success_url = return_url(
        request
            .success_url
            .as_deref()
            .expect("validated success URL"),
        checkout.checkout_id,
        true,
    )?;
    let cancel_url = request.cancel_url.as_deref().expect("validated cancel URL");
    let result = connector
        .create_hosted_payment_session(&HostedPaymentSessionCommand {
            customer_reference: hosted.customer_id.clone(),
            client_reference_id: checkout.checkout_id.to_string(),
            collection_request_id: collection.collection_request_id.to_string(),
            amount_minor: collection.amount_minor,
            currency: collection.currency.clone(),
            success_url,
            cancel_url: cancel_url.to_string(),
            expires_at: collection.payment_expires_at.timestamp(),
            provider_idempotency_key: format!(
                "checkout:{}:hosted-session:v2",
                checkout.checkout_id
            ),
        })
        .await
        .map_err(stripe_error)?;
    repository
        .finish_hosted_payment_session(
            checkout.checkout_id,
            &result.provider_session_id,
            &result.redirect_url,
        )
        .await
}

struct HostedCheckout {
    customer_id: String,
}

fn validate_return_urls(request: &CreateCheckoutRequest) -> ApiResult<()> {
    for (name, value) in [
        ("success_url", &request.success_url),
        ("cancel_url", &request.cancel_url),
    ] {
        let value = value.as_deref().ok_or_else(|| {
            ApiError::unprocessable(
                "checkout_return_url_required",
                format!("{name} must be provided for hosted checkout"),
            )
        })?;
        let parsed = url::Url::parse(value).map_err(|error| {
            ApiError::unprocessable(
                "invalid_checkout_return_url",
                format!(
                    "{name} {value:?} must be an absolute HTTPS URL or loopback HTTP URL: {error}"
                ),
            )
        })?;
        let loopback = matches!(parsed.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"));
        if parsed.scheme() != "https" && !(loopback && parsed.scheme() == "http") {
            return Err(ApiError::unprocessable(
                "invalid_checkout_return_url",
                format!("{name} {value:?} must use HTTPS except on loopback"),
            ));
        }
    }
    Ok(())
}

fn return_url(value: &str, checkout_id: Uuid, success: bool) -> ApiResult<String> {
    let mut parsed = url::Url::parse(value).map_err(|error| {
        ApiError::unprocessable(
            "invalid_checkout_return_url",
            format!("return URL {value:?} must be absolute: {error}"),
        )
    })?;
    if success {
        parsed
            .query_pairs_mut()
            .append_pair("subscription_checkout_id", &checkout_id.to_string());
    }
    Ok(parsed.to_string())
}

async fn ensure_integration(
    repository: &DatabaseRepository,
    config: &BillingCheckoutConfig,
    account_id: Uuid,
    stripe_account_id: &str,
) -> ApiResult<crate::dto::billing::AccountIntegrationResponse> {
    let current = repository
        .find_test_stripe_integration(account_id, stripe_account_id)
        .await?;
    let integration = match current {
        Some(integration) => integration,
        None => create_integration(repository, account_id, stripe_account_id, config).await?,
    };
    if integration_is_ready(&integration) {
        return Ok(integration);
    }
    configure_integration(repository, account_id, &integration, config).await
}

pub(crate) async fn ensure_sandbox_integration(
    repository: &DatabaseRepository,
    config: &BillingCheckoutConfig,
    account_id: Uuid,
) -> ApiResult<crate::dto::billing::AccountIntegrationResponse> {
    let stripe_account_id = StripeConnector::new(config.api_secret.clone(), None)
        .identify_stripe_account()
        .await
        .map_err(stripe_error)?;
    ensure_integration(repository, config, account_id, &stripe_account_id).await
}

async fn create_integration(
    repository: &DatabaseRepository,
    account_id: Uuid,
    stripe_account_id: &str,
    config: &BillingCheckoutConfig,
) -> ApiResult<crate::dto::billing::AccountIntegrationResponse> {
    repository
        .create_stripe_integration(
            account_id,
            stripe_account_id,
            "TEST",
            None,
            &config.api_secret,
        )
        .await
}

async fn configure_integration(
    repository: &DatabaseRepository,
    account_id: Uuid,
    integration: &crate::dto::billing::AccountIntegrationResponse,
    config: &BillingCheckoutConfig,
) -> ApiResult<crate::dto::billing::AccountIntegrationResponse> {
    let configured = billing_integrations::update_stripe(
        repository,
        account_id,
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
            account_id,
            configured.billing_connection_id,
            configured.configuration_version,
        )
        .await
}

fn integration_is_ready(integration: &crate::dto::billing::AccountIntegrationResponse) -> bool {
    let active = integration.status == "ACTIVE";
    active && integration.webhook_secret_configured
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
                    checkout.account_id,
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
                    checkout.account_id,
                    checkout.customer_plan_id,
                    &checkout.idempotency_key,
                    &CreateOnDemandPurchaseRequest {
                        on_demand_plan_id: request.on_demand_plan_id.expect("validated offer"),
                        quantity: request.quantity.unwrap_or(1),
                        payment_method_binding_id: binding_id,
                        transaction_id: checkout.transaction_id.clone(),
                    },
                    coupon,
                )
                .await
        }
        crate::dto::checkouts::CheckoutKind::PlanUpgrade => {
            crate::services::billing::create_paid_plan_upgrade(
                repository,
                checkout.account_id,
                checkout.customer_plan_id,
                &checkout.idempotency_key,
                &CreatePlanTransitionRequest {
                    new_plan_version_id: request
                        .target_plan_version_id
                        .expect("validated target plan"),
                    transition_kind: PlanTransitionKind::Upgrade,
                    payment_method_binding_id: Some(binding_id),
                    transaction_id: checkout.transaction_id.clone(),
                    actor_reference: format!("account:{}", checkout.account_id),
                },
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
            redirect_url: None,
        });
    }
    if let Some(collection_id) = record.collection_request_id {
        let collection = repository
            .find_collection_request(record.account_id, collection_id)
            .await?;
        let mut response = repository.response_for_checkout(record.clone(), Some(&collection));
        response.redirect_url = repository
            .hosted_payment_redirect_url(record.checkout_id)
            .await?;
        return Ok(response);
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
        redirect_url: None,
    }
}

fn validate_request(request: &CreateCheckoutRequest, key: &str) -> ApiResult<()> {
    let valid = match request.checkout_kind {
        crate::dto::checkouts::CheckoutKind::Initial => {
            request.on_demand_plan_id.is_none()
                && request.target_plan_version_id.is_none()
                && request.quantity.is_none_or(|value| value == 1)
        }
        crate::dto::checkouts::CheckoutKind::OnDemand => {
            request.on_demand_plan_id.is_some() && request.target_plan_version_id.is_none()
        }
        crate::dto::checkouts::CheckoutKind::PlanUpgrade => {
            request.on_demand_plan_id.is_none()
                && request.target_plan_version_id.is_some()
                && request.quantity.is_none_or(|value| value == 1)
                && request.coupon_code.is_none()
        }
    };
    let valid_quantity = request
        .quantity
        .is_none_or(|quantity| (1..=10_000).contains(&quantity));
    let valid_transaction = (1..=255).contains(&request.transaction_id.len())
        && request.transaction_id.is_ascii()
        && !request.transaction_id.trim().is_empty();
    let valid_key = (1..=255).contains(&key.len()) && key.is_ascii();
    if valid
        && valid_transaction
        && valid_key
        && valid_quantity
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

fn stripe_error(error: impl std::fmt::Display) -> ApiError {
    ApiError::external("billing_connector_error", error.to_string())
}

#[cfg(test)]
mod tests {
    use super::{integration_is_ready, return_url, validate_request, validate_return_urls};
    use crate::dto::{
        billing::AccountIntegrationResponse,
        checkouts::{CheckoutKind, CreateCheckoutRequest},
    };
    use uuid::Uuid;

    #[test]
    fn checkout_request_requires_offer_and_bounded_ascii_operation_ids() {
        let mut request = CreateCheckoutRequest {
            customer_plan_id: Uuid::new_v4(),
            checkout_kind: CheckoutKind::Initial,
            on_demand_plan_id: None,
            target_plan_version_id: None,
            quantity: None,
            success_url: None,
            cancel_url: None,
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
        let mut integration = AccountIntegrationResponse {
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

    #[test]
    fn hosted_checkout_return_urls_require_https_outside_loopback() {
        let mut request = CreateCheckoutRequest {
            customer_plan_id: Uuid::new_v4(),
            checkout_kind: CheckoutKind::OnDemand,
            on_demand_plan_id: Some(Uuid::new_v4()),
            target_plan_version_id: None,
            quantity: Some(2),
            success_url: Some("https://client.example/checkout/success".into()),
            cancel_url: Some("http://localhost:5174/checkout/cancel".into()),
            transaction_id: "operation-2".into(),
            coupon_code: None,
            payment_method_binding_id: None,
        };
        assert!(validate_return_urls(&request).is_ok());
        request.cancel_url = Some("http://client.example/cancel".into());
        assert!(validate_return_urls(&request).is_err());
    }

    #[test]
    fn hosted_success_return_adds_only_subscription_polling_references() {
        let checkout_id = Uuid::new_v4();
        let url = return_url("https://client.example/done?source=web", checkout_id, true)
            .expect("valid URL");
        let parsed = url::Url::parse(&url).expect("returned URL");
        assert_eq!(
            parsed
                .query_pairs()
                .find(|(key, _)| key == "source")
                .unwrap()
                .1,
            "web"
        );
        assert!(parsed
            .query()
            .unwrap()
            .contains(&format!("subscription_checkout_id={checkout_id}")));
        assert!(!parsed.query().unwrap().contains("session_id"));
    }
}
