use serde_json::Value;

use crate::repositories::billing_connector::{
    BillingCapabilities, BillingConnector, BillingConnectorError, BillingPaymentMethod,
    CollectionCommand, ConnectorCollectionResult, ConnectorCollectionState, ConnectorFuture,
    SetupSessionCommand, SetupSessionFuture, SetupSessionResult,
};

pub struct StripeConnector {
    client: reqwest::Client,
    secret_key: String,
    api_base: String,
    connected_account: Option<String>,
}

impl StripeConnector {
    pub fn new(secret_key: String, connected_account: Option<String>) -> Self {
        Self::with_api_base(secret_key, connected_account, "https://api.stripe.com")
    }

    pub fn with_api_base(
        secret_key: String,
        connected_account: Option<String>,
        api_base: impl Into<String>,
    ) -> Self {
        Self {
            client: reqwest::Client::new(),
            secret_key,
            api_base: api_base.into(),
            connected_account,
        }
    }

    async fn post_form(
        &self,
        path: &str,
        fields: &[(&str, String)],
        idempotency_key: Option<&str>,
    ) -> Result<Value, BillingConnectorError> {
        let mut request = self
            .client
            .post(format!("{}{}", self.api_base, path))
            .basic_auth(&self.secret_key, Some(""))
            .form(fields);
        if let Some(key) = idempotency_key {
            request = request.header("Idempotency-Key", key);
        }
        if let Some(account) = &self.connected_account {
            request = request.header("Stripe-Account", account);
        }
        let response = request.send().await.map_err(transport_error)?;
        let status = response.status();
        let body: Value = response.json().await.map_err(transport_error)?;
        if status.is_success() {
            return Ok(body);
        }
        Err(api_error(status.as_u16(), &body))
    }
}

impl BillingConnector for StripeConnector {
    fn capabilities(&self) -> BillingCapabilities {
        BillingCapabilities {
            payment_methods: vec![BillingPaymentMethod::Card],
            supports_setup_session: true,
            supports_vault: true,
            supports_off_session_charge: true,
            supports_webhook: true,
        }
    }

    fn start_collection<'a>(&'a self, command: &'a CollectionCommand) -> ConnectorFuture<'a> {
        Box::pin(async move {
            let amount = command.amount_minor.to_string();
            let fields = vec![
                ("amount", amount),
                ("currency", command.currency.to_ascii_lowercase()),
                ("payment_method", command.payment_method_reference.clone()),
                ("confirm", "true".to_string()),
                ("off_session", "true".to_string()),
                (
                    "metadata[collection_request_id]",
                    collection_id_from_key(&command.provider_idempotency_key),
                ),
            ];
            let value = self
                .post_form(
                    "/v1/payment_intents",
                    &fields,
                    Some(&command.provider_idempotency_key),
                )
                .await?;
            collection_result(&value)
        })
    }

    fn create_setup_session<'a>(
        &'a self,
        command: &'a SetupSessionCommand,
    ) -> SetupSessionFuture<'a> {
        Box::pin(async move {
            let fields = vec![
                ("customer", command.customer_reference.clone()),
                ("payment_method_types[]", "card".to_string()),
                ("usage", "off_session".to_string()),
                ("return_url", command.return_url.clone()),
            ];
            let value = self.post_form("/v1/setup_intents", &fields, None).await?;
            Ok(SetupSessionResult {
                provider_setup_id: required_string(&value, "id")?,
                client_secret: required_string(&value, "client_secret")?,
            })
        })
    }
}

fn collection_result(value: &Value) -> Result<ConnectorCollectionResult, BillingConnectorError> {
    let status = required_string(value, "status")?;
    let state = match status.as_str() {
        "requires_action" | "requires_source_action" => ConnectorCollectionState::RequiresAction,
        "requires_payment_method" | "canceled" => ConnectorCollectionState::Failed,
        "processing" | "requires_capture" | "succeeded" => ConnectorCollectionState::Pending,
        other => return Err(invalid_response("status", other)),
    };
    let next_action_url = value
        .pointer("/next_action/redirect_to_url/url")
        .and_then(Value::as_str)
        .map(str::to_string);
    Ok(ConnectorCollectionResult {
        provider_payment_id: Some(required_string(value, "id")?),
        state,
        failure_code: value
            .pointer("/last_payment_error/code")
            .and_then(Value::as_str)
            .map(str::to_string),
        next_action_url,
    })
}

fn collection_id_from_key(key: &str) -> String {
    key.strip_prefix("collection:")
        .and_then(|value| value.strip_suffix(":attempt:1"))
        .unwrap_or(key)
        .to_string()
}

fn required_string(value: &Value, field: &str) -> Result<String, BillingConnectorError> {
    value
        .get(field)
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| invalid_response(field, &value.to_string()))
}

fn invalid_response(field: &str, value: &str) -> BillingConnectorError {
    BillingConnectorError {
        code: "stripe_invalid_response".to_string(),
        message: format!("Stripe response field {field} has unsupported value {value:?}"),
        retryable: false,
        outcome_uncertain: false,
    }
}

fn transport_error(error: reqwest::Error) -> BillingConnectorError {
    BillingConnectorError {
        code: "stripe_transport_error".to_string(),
        message: error.to_string(),
        retryable: true,
        outcome_uncertain: error.is_timeout() || error.is_request(),
    }
}

fn api_error(status: u16, body: &Value) -> BillingConnectorError {
    let details = body.get("error").unwrap_or(body);
    let code = details
        .get("code")
        .and_then(Value::as_str)
        .unwrap_or("stripe_api_error");
    let message = details
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or("Stripe rejected the request");
    let retryable = status == 409 || status == 429 || status >= 500;
    BillingConnectorError {
        code: code.to_string(),
        message: message.to_string(),
        retryable,
        outcome_uncertain: retryable,
    }
}
