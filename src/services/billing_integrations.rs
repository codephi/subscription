use uuid::Uuid;

use crate::{
    dto::billing::{
        CreateStripeIntegrationRequest, DefaultStripeCredentialsResponse,
        IntegrationProviderResponse, StripeIntegrationTestResponse,
        UpdateDefaultStripeCredentialsRequest, UpdateStripeIntegrationRequest,
        WorkspaceIntegrationResponse,
    },
    error::{ApiError, ApiResult},
    repositories::{
        database::DatabaseRepository, integrations::IntegrationSecrets, stripe::StripeConnector,
    },
};

pub async fn default_stripe_credentials(
    repository: &DatabaseRepository,
) -> ApiResult<DefaultStripeCredentialsResponse> {
    repository.default_stripe_credentials().await
}

pub async fn update_default_stripe_credentials(
    repository: &DatabaseRepository,
    request: UpdateDefaultStripeCredentialsRequest,
) -> ApiResult<DefaultStripeCredentialsResponse> {
    let current = repository.load_default_stripe_secrets().await?;
    let (environment, account_reference) = match request.secret_key.as_deref() {
        Some(secret) => {
            let environment = split_stripe_key(secret)?.0;
            let account = StripeConnector::new(secret.to_string(), None)
                .identify_account()
                .await
                .map_err(invalid_stripe_credentials)?;
            (environment, account)
        }
        None => {
            let current = current.as_ref().ok_or_else(|| {
                ApiError::unprocessable(
                    "default_stripe_secret_required",
                    "secret_key is required to configure default Stripe credentials",
                )
            })?;
            (
                if current.environment == "TEST" {
                    "TEST"
                } else {
                    "LIVE"
                },
                current.account_reference.clone(),
            )
        }
    };
    validate_webhook_secret(request.webhook_secret.as_deref())?;
    require_default_webhook(
        &current,
        environment,
        &account_reference,
        request.webhook_secret.as_deref(),
    )?;
    repository
        .save_default_stripe_credentials(
            request.expected_version,
            environment,
            &account_reference,
            request.secret_key.as_deref(),
            request.webhook_secret.as_deref(),
        )
        .await
}

fn require_default_webhook(
    current: &Option<crate::repositories::default_stripe_credentials::DefaultStripeSecrets>,
    environment: &str,
    account_reference: &str,
    replacement: Option<&str>,
) -> ApiResult<()> {
    let unchanged_scope = current.as_ref().is_some_and(|secrets| {
        secrets.environment == environment && secrets.account_reference == account_reference
    });
    if replacement.is_some() || (unchanged_scope && current_has_webhook(current)) {
        return Ok(());
    }
    Err(ApiError::unprocessable(
        "default_stripe_webhook_secret_required",
        "webhook_secret is required for new or changed Stripe account credentials",
    ))
}

fn current_has_webhook(
    current: &Option<crate::repositories::default_stripe_credentials::DefaultStripeSecrets>,
) -> bool {
    current
        .as_ref()
        .is_some_and(|secrets| secrets.webhook_secret.is_some())
}

pub async fn provision_workspace_defaults(
    repository: &DatabaseRepository,
    workspace_id: Uuid,
) -> ApiResult<()> {
    let Some(defaults) = repository.load_default_stripe_secrets().await? else {
        return Ok(());
    };
    repository
        .provision_workspace_default_stripe(workspace_id, &defaults)
        .await
}

pub async fn active_or_provision_default_stripe(
    repository: &DatabaseRepository,
    workspace_id: Uuid,
) -> ApiResult<Uuid> {
    match repository
        .active_stripe_billing_connection(workspace_id)
        .await
    {
        Ok(connection_id) => Ok(connection_id),
        Err(error) if error.code() == "billing_connection_not_usable" => {
            provision_workspace_defaults(repository, workspace_id).await?;
            repository
                .active_stripe_billing_connection(workspace_id)
                .await
        }
        Err(error) => Err(error),
    }
}

pub fn providers() -> Vec<IntegrationProviderResponse> {
    vec![IntegrationProviderResponse {
        provider: "STRIPE".into(),
        display_name: "Stripe".into(),
        available: true,
    }]
}

pub async fn list(
    repository: &DatabaseRepository,
    workspace_id: Uuid,
) -> ApiResult<Vec<WorkspaceIntegrationResponse>> {
    repository.list_integrations(workspace_id).await
}

pub async fn get(
    repository: &DatabaseRepository,
    workspace_id: Uuid,
    connection_id: Uuid,
) -> ApiResult<WorkspaceIntegrationResponse> {
    repository
        .get_integration(workspace_id, connection_id)
        .await
}

pub async fn create_stripe(
    repository: &DatabaseRepository,
    workspace_id: Uuid,
    request: &CreateStripeIntegrationRequest,
) -> ApiResult<WorkspaceIntegrationResponse> {
    let environment = validate_stripe_key(&request.secret_key, &request.environment)?;
    if let Some(customer_id) = &request.existing_customer_reference {
        validate_customer_id(customer_id)?;
    }
    let connector = StripeConnector::new(request.secret_key.clone(), None);
    let account_id = connector
        .identify_account()
        .await
        .map_err(invalid_stripe_credentials)?;
    if let Some(customer_id) = &request.existing_customer_reference {
        connector
            .validate_customer(customer_id)
            .await
            .map_err(|_| invalid_customer(customer_id))?;
    }
    repository
        .create_stripe_integration(
            workspace_id,
            &account_id,
            environment,
            request.existing_customer_reference.as_deref(),
            &request.secret_key,
        )
        .await
}

pub async fn update_stripe(
    repository: &DatabaseRepository,
    workspace_id: Uuid,
    connection_id: Uuid,
    request: &UpdateStripeIntegrationRequest,
) -> ApiResult<WorkspaceIntegrationResponse> {
    let current = repository
        .integration_secrets(workspace_id, connection_id)
        .await?;
    require_managed_stripe(&current)?;
    if let Some(secret_key) = &request.secret_key {
        validate_rotation(&current, secret_key).await?;
    }
    validate_webhook_secret(request.webhook_secret.as_deref())?;
    repository
        .update_stripe_integration(
            workspace_id,
            connection_id,
            request.expected_version,
            request.secret_key.as_deref(),
            request.webhook_secret.as_deref(),
        )
        .await
}

pub async fn test_stripe(
    repository: &DatabaseRepository,
    workspace_id: Uuid,
    connection_id: Uuid,
) -> ApiResult<StripeIntegrationTestResponse> {
    let current = repository
        .integration_secrets(workspace_id, connection_id)
        .await?;
    require_managed_stripe(&current)?;
    let secret = open_api_secret(repository, &current)?;
    let connector = StripeConnector::new(secret, None);
    let account = connector
        .identify_account()
        .await
        .map_err(invalid_stripe_credentials)?;
    let expected = current.provider_account_reference.as_deref().unwrap_or("");
    if account != expected {
        return Err(ApiError::conflict(
            "stripe_account_changed",
            "Stripe key belongs to a different account; create a new integration",
        ));
    }
    Ok(StripeIntegrationTestResponse {
        successful: true,
        account_reference: account,
        environment: current.environment.unwrap_or_else(|| "UNKNOWN".into()),
    })
}

pub async fn ensure_customer(
    repository: &DatabaseRepository,
    current: &IntegrationSecrets,
) -> ApiResult<String> {
    if let Some(customer_id) = &current.provider_customer_reference {
        return Ok(customer_id.clone());
    }
    let recovered = repository
        .begin_customer_operation(current.workspace_id, current.billing_connection_id)
        .await?;
    if let Some(customer_id) = recovered {
        return Ok(customer_id);
    }
    let secret = open_api_secret(repository, current)?;
    let connector = StripeConnector::new(secret, None);
    let customer_id = connector
        .create_workspace_customer(
            &current.workspace_id.to_string(),
            &current.billing_connection_id.to_string(),
        )
        .await
        .map_err(invalid_stripe_credentials)?;
    repository
        .finish_customer_operation(
            current.workspace_id,
            current.billing_connection_id,
            &customer_id,
        )
        .await?;
    Ok(customer_id)
}

async fn validate_rotation(current: &IntegrationSecrets, secret_key: &str) -> ApiResult<()> {
    let (environment, _) = split_stripe_key(secret_key)?;
    if Some(environment) != current.environment.as_deref() {
        return Err(ApiError::unprocessable(
            "stripe_environment_mismatch",
            "replacement key must use the integration's existing environment",
        ));
    }
    let account = StripeConnector::new(secret_key.to_string(), None)
        .identify_account()
        .await
        .map_err(invalid_stripe_credentials)?;
    if Some(account.as_str()) != current.provider_account_reference.as_deref() {
        return Err(ApiError::unprocessable(
            "stripe_account_mismatch",
            "replacement key must belong to the integration's existing Stripe account",
        ));
    }
    Ok(())
}

fn validate_stripe_key<'a>(secret_key: &'a str, environment: &str) -> ApiResult<&'a str> {
    let (key_environment, _) = split_stripe_key(secret_key)?;
    if key_environment != environment {
        return Err(ApiError::unprocessable(
            "stripe_environment_mismatch",
            "selected environment must match the Stripe key prefix",
        ));
    }
    Ok(key_environment)
}

fn split_stripe_key(secret_key: &str) -> ApiResult<(&'static str, &'static str)> {
    if secret_key.starts_with("sk_test_") {
        return Ok(("TEST", "test"));
    }
    if secret_key.starts_with("sk_live_") {
        return Ok(("LIVE", "live"));
    }
    Err(ApiError::unprocessable(
        "stripe_secret_key_invalid",
        "secret_key must be a Stripe secret key beginning with sk_test_ or sk_live_",
    ))
}

fn validate_customer_id(customer_id: &str) -> ApiResult<()> {
    if customer_id.starts_with("cus_") && customer_id.len() <= 255 {
        return Ok(());
    }
    Err(invalid_customer(customer_id))
}

fn validate_webhook_secret(secret: Option<&str>) -> ApiResult<()> {
    if secret.is_none_or(|value| value.starts_with("whsec_") && value.len() <= 255) {
        return Ok(());
    }
    Err(ApiError::unprocessable(
        "stripe_webhook_secret_invalid",
        "webhook_secret must be a Stripe signing secret beginning with whsec_",
    ))
}

fn require_managed_stripe(current: &IntegrationSecrets) -> ApiResult<()> {
    if current.provider == "STRIPE" && current.environment.is_some() {
        return Ok(());
    }
    Err(ApiError::conflict(
        "integration_not_managed",
        format!(
            "integration {} is a legacy environment-based connection",
            current.billing_connection_id
        ),
    ))
}

fn open_api_secret(
    repository: &DatabaseRepository,
    current: &IntegrationSecrets,
) -> ApiResult<String> {
    repository.credential_vault()?.open(
        current.workspace_id,
        current.billing_connection_id,
        "stripe_api",
        &current.secret_reference,
    )
}

fn invalid_stripe_credentials(_: impl std::fmt::Display) -> ApiError {
    ApiError::unprocessable(
        "stripe_credentials_rejected",
        "Stripe rejected the credentials or the account could not be verified",
    )
}

fn invalid_customer(customer_id: &str) -> ApiError {
    ApiError::unprocessable(
        "stripe_customer_invalid",
        format!("customer reference {customer_id:?} must be an accessible Stripe cus_ id"),
    )
}

#[cfg(test)]
mod tests {
    use super::{
        require_default_webhook, split_stripe_key, validate_customer_id, validate_stripe_key,
        validate_webhook_secret,
    };
    use crate::repositories::default_stripe_credentials::DefaultStripeSecrets;

    #[test]
    fn stripe_environment_must_match_secret_key_prefix() {
        assert!(validate_stripe_key("sk_test_example", "TEST").is_ok());
        assert!(validate_stripe_key("sk_live_example", "LIVE").is_ok());
        assert!(validate_stripe_key("sk_test_example", "LIVE").is_err());
        assert!(split_stripe_key("pk_test_example").is_err());
    }

    #[test]
    fn optional_customer_and_webhook_values_require_provider_identifiers() {
        assert!(validate_customer_id("cus_existing").is_ok());
        assert!(validate_customer_id("pm_not_a_customer").is_err());
        assert!(validate_webhook_secret(None).is_ok());
        assert!(validate_webhook_secret(Some("whsec_existing")).is_ok());
        assert!(validate_webhook_secret(Some("sk_test_example")).is_err());
    }

    #[test]
    fn default_credentials_require_a_webhook_secret_before_first_save() {
        assert!(require_default_webhook(&None, "TEST", "acct_example", None).is_err());
        assert!(
            require_default_webhook(&None, "TEST", "acct_example", Some("whsec_example")).is_ok()
        );
        let current = Some(DefaultStripeSecrets {
            environment: "TEST".into(),
            account_reference: "acct_example".into(),
            api_secret: "sk_test_example".into(),
            webhook_secret: Some("whsec_example".into()),
        });
        assert!(require_default_webhook(&current, "TEST", "acct_example", None).is_ok());
        assert!(require_default_webhook(&current, "LIVE", "acct_example", None).is_err());
    }
}
