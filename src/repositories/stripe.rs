use serde_json::Value;

use crate::repositories::billing_connector::{
    BillingCapabilities, BillingConnector, BillingConnectorError, BillingPaymentMethod,
    CollectionCommand, ConnectorCollectionResult, ConnectorCollectionState, ConnectorFuture,
    SetupSessionCommand, SetupSessionFuture, SetupSessionResult,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreparedStripePaymentMethod {
    pub setup_intent_id: String,
    pub payment_method_id: String,
    pub customer_id: String,
}

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

    pub async fn identify_account(&self) -> Result<String, BillingConnectorError> {
        let response = self
            .client
            .get(format!("{}/v1/account", self.api_base))
            .basic_auth(&self.secret_key, Some(""))
            .send()
            .await
            .map_err(transport_error)?;
        let status = response.status();
        let body: Value = response.json().await.map_err(transport_error)?;
        if !status.is_success() {
            return Err(api_error(status.as_u16(), &body));
        }
        let account_id = required_string(&body, "id")?;
        if !account_id.starts_with("acct_") {
            return Err(invalid_response("id", &account_id));
        }
        Ok(account_id)
    }

    pub async fn validate_customer(&self, customer_id: &str) -> Result<(), BillingConnectorError> {
        let response = self
            .client
            .get(format!("{}/v1/customers/{customer_id}", self.api_base))
            .basic_auth(&self.secret_key, Some(""))
            .send()
            .await
            .map_err(transport_error)?;
        let status = response.status();
        let body: Value = response.json().await.map_err(transport_error)?;
        if !status.is_success() {
            return Err(api_error(status.as_u16(), &body));
        }
        if body.get("id").and_then(Value::as_str) != Some(customer_id) {
            return Err(invalid_response("customer.id", &body.to_string()));
        }
        Ok(())
    }

    pub async fn retrieve_setup_intent(
        &self,
        setup_intent_id: &str,
    ) -> Result<PreparedStripePaymentMethod, BillingConnectorError> {
        let mut request = self
            .client
            .get(format!(
                "{}/v1/setup_intents/{setup_intent_id}",
                self.api_base
            ))
            .basic_auth(&self.secret_key, Some(""));
        if let Some(account) = &self.connected_account {
            request = request.header("Stripe-Account", account);
        }
        let response = request.send().await.map_err(transport_error)?;
        let status = response.status();
        let body: Value = response.json().await.map_err(transport_error)?;
        if !status.is_success() {
            return Err(api_error(status.as_u16(), &body));
        }
        let setup = prepared_payment_method(&body, "")?;
        if setup.setup_intent_id != setup_intent_id
            || body.get("usage").and_then(Value::as_str) != Some("off_session")
        {
            return Err(invalid_response("SetupIntent", &body.to_string()));
        }
        Ok(setup)
    }

    pub async fn retrieve_checkout_setup_intent(
        &self,
        checkout_session_id: &str,
        expected_customer: &str,
        expected_client_reference: &str,
        expected_connection_id: &str,
    ) -> Result<PreparedStripePaymentMethod, BillingConnectorError> {
        let mut request = self
            .client
            .get(format!(
                "{}/v1/checkout/sessions/{checkout_session_id}?expand%5B%5D=setup_intent",
                self.api_base
            ))
            .basic_auth(&self.secret_key, Some(""));
        if let Some(account) = &self.connected_account {
            request = request.header("Stripe-Account", account);
        }
        let response = request.send().await.map_err(transport_error)?;
        let status = response.status();
        let body: Value = response.json().await.map_err(transport_error)?;
        if !status.is_success() {
            return Err(api_error(status.as_u16(), &body));
        }
        validate_checkout_session(
            &body,
            checkout_session_id,
            expected_customer,
            expected_client_reference,
            expected_connection_id,
        )
    }

    pub async fn create_workspace_customer(
        &self,
        workspace_id: &str,
        connection_id: &str,
    ) -> Result<String, BillingConnectorError> {
        let fields = vec![
            ("name", format!("Workspace {workspace_id}")),
            ("metadata[workspace_id]", workspace_id.to_string()),
            ("metadata[billing_connection_id]", connection_id.to_string()),
        ];
        let value = self
            .post_form(
                "/v1/customers",
                &fields,
                Some(&format!("billing-connection:{connection_id}:customer:v1")),
            )
            .await?;
        required_string(&value, "id")
    }

    pub async fn prepare_test_payment_method(
        &self,
        customer_id: &str,
        idempotency_key: &str,
        declines_charge: bool,
    ) -> Result<PreparedStripePaymentMethod, BillingConnectorError> {
        let payment_method = if declines_charge {
            "pm_card_chargeCustomerFail"
        } else {
            "pm_card_visa"
        };
        let fields = vec![
            ("customer", customer_id.to_string()),
            ("payment_method_types[]", "card".to_string()),
            ("usage", "off_session".to_string()),
            ("payment_method", payment_method.to_string()),
            ("confirm", "true".to_string()),
        ];
        let value = self
            .post_form("/v1/setup_intents", &fields, Some(idempotency_key))
            .await?;
        if value.get("status").and_then(Value::as_str) != Some("succeeded") {
            let code = value
                .pointer("/last_setup_error/code")
                .and_then(Value::as_str)
                .unwrap_or("card_setup_failed");
            return Err(BillingConnectorError {
                code: code.to_string(),
                message: "Stripe could not set up the supplied card".to_string(),
                retryable: false,
                outcome_uncertain: false,
            });
        }
        prepared_payment_method(&value, customer_id)
    }

    pub async fn create_card_setup_intent(
        &self,
        customer_id: &str,
        cardholder_name: &str,
        card_number: &str,
        exp_month: u8,
        exp_year: u16,
        cvc: &str,
        idempotency_key: &str,
    ) -> Result<PreparedStripePaymentMethod, BillingConnectorError> {
        let fields = vec![
            ("customer", customer_id.to_string()),
            ("usage", "off_session".to_string()),
            ("payment_method_types[]", "card".to_string()),
            ("payment_method_data[type]", "card".to_string()),
            (
                "payment_method_data[billing_details][name]",
                cardholder_name.to_string(),
            ),
            ("payment_method_data[card][number]", card_number.to_string()),
            (
                "payment_method_data[card][exp_month]",
                exp_month.to_string(),
            ),
            ("payment_method_data[card][exp_year]", exp_year.to_string()),
            ("payment_method_data[card][cvc]", cvc.to_string()),
            ("confirm", "true".to_string()),
        ];
        let value = self
            .post_form("/v1/setup_intents", &fields, Some(idempotency_key))
            .await?;
        if value.get("status").and_then(Value::as_str) != Some("succeeded") {
            let code = value
                .pointer("/last_setup_error/code")
                .and_then(Value::as_str)
                .unwrap_or("card_setup_failed");
            return Err(BillingConnectorError {
                code: code.to_string(),
                message: "Stripe could not set up the supplied card".to_string(),
                retryable: false,
                outcome_uncertain: false,
            });
        }
        let returned_customer = value
            .get("customer")
            .and_then(Value::as_str)
            .ok_or_else(|| invalid_response("SetupIntent.customer", "missing"))?;
        let usage = value
            .get("usage")
            .and_then(Value::as_str)
            .ok_or_else(|| invalid_response("SetupIntent.usage", "missing"))?;
        let setup_intent_id = value
            .get("id")
            .and_then(Value::as_str)
            .filter(|id| id.starts_with("seti_"))
            .ok_or_else(|| invalid_response("SetupIntent.id", "missing or unsupported"))?;
        let payment_method_id = value
            .get("payment_method")
            .and_then(Value::as_str)
            .filter(|id| id.starts_with("pm_"))
            .ok_or_else(|| {
                invalid_response("SetupIntent.payment_method", "missing or unsupported")
            })?;
        if returned_customer != customer_id || usage != "off_session" {
            return Err(invalid_response(
                "SetupIntent",
                "customer or usage does not match the request",
            ));
        }
        Ok(PreparedStripePaymentMethod {
            setup_intent_id: setup_intent_id.to_string(),
            payment_method_id: payment_method_id.to_string(),
            customer_id: returned_customer.to_string(),
        })
    }

    pub async fn detach_payment_method(
        &self,
        payment_method_id: &str,
    ) -> Result<(), BillingConnectorError> {
        let mut request = self
            .client
            .post(format!(
                "{}/v1/payment_methods/{payment_method_id}/detach",
                self.api_base
            ))
            .basic_auth(&self.secret_key, Some(""));
        if let Some(account) = &self.connected_account {
            request = request.header("Stripe-Account", account);
        }
        let response = request.send().await.map_err(transport_error)?;
        let status = response.status();
        let body: Value = response.json().await.map_err(transport_error)?;
        if status.is_success() {
            return Ok(());
        }
        Err(api_error(status.as_u16(), &body))
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

fn validate_checkout_session(
    session: &Value,
    expected_session_id: &str,
    expected_customer: &str,
    expected_client_reference: &str,
    expected_connection_id: &str,
) -> Result<PreparedStripePaymentMethod, BillingConnectorError> {
    let setup = session
        .get("setup_intent")
        .ok_or_else(|| invalid_response("CheckoutSession.setup_intent", &session.to_string()))?;
    let shape_is_valid = required_string(session, "id")? == expected_session_id
        && required_string(session, "mode")? == "setup"
        && required_string(session, "status")? == "complete"
        && required_string(session, "customer")? == expected_customer
        && required_string(session, "client_reference_id")? == expected_client_reference
        && session
            .pointer("/metadata/billing_connection_id")
            .and_then(Value::as_str)
            == Some(expected_connection_id);
    if !shape_is_valid {
        return Err(invalid_response("CheckoutSession", &session.to_string()));
    }
    let prepared = prepared_payment_method(setup, expected_customer)?;
    if setup.get("usage").and_then(Value::as_str) != Some("off_session") {
        return Err(invalid_response("SetupIntent.usage", &setup.to_string()));
    }
    Ok(prepared)
}

fn prepared_payment_method(
    value: &Value,
    expected_customer: &str,
) -> Result<PreparedStripePaymentMethod, BillingConnectorError> {
    let status = required_string(value, "status")?;
    let customer_id = required_string(value, "customer")?;
    if status != "succeeded" || (!expected_customer.is_empty() && customer_id != expected_customer)
    {
        return Err(invalid_response("SetupIntent", &value.to_string()));
    }
    let payment_method_id = required_string(value, "payment_method")?;
    if !payment_method_id.starts_with("pm_") {
        return Err(invalid_response(
            "SetupIntent.payment_method",
            &value.to_string(),
        ));
    }
    Ok(PreparedStripePaymentMethod {
        setup_intent_id: required_string(value, "id")?,
        payment_method_id,
        customer_id,
    })
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
            let mut fields = vec![
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
            if let Some(customer) = &command.customer_reference {
                fields.push(("customer", customer.clone()));
            }
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
                ("mode", "setup".to_string()),
                ("customer", command.customer_reference.clone()),
                ("client_reference_id", command.client_reference_id.clone()),
                (
                    "metadata[billing_connection_id]",
                    command.billing_connection_id.clone(),
                ),
                ("success_url", command.success_url.clone()),
                ("cancel_url", command.cancel_url.clone()),
                ("payment_method_types[]", "card".to_string()),
                ("setup_intent_data[usage]", "off_session".to_string()),
            ];
            let value = self
                .post_form("/v1/checkout/sessions", &fields, None)
                .await?;
            Ok(SetupSessionResult {
                provider_setup_id: required_string(&value, "id")?,
                redirect_url: required_string(&value, "url")?,
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
