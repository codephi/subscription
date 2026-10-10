use std::{future::Future, pin::Pin};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BillingPaymentMethod {
    Card,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BillingCapabilities {
    pub payment_methods: Vec<BillingPaymentMethod>,
    pub supports_setup_session: bool,
    pub supports_vault: bool,
    pub supports_off_session_charge: bool,
    pub supports_webhook: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CollectionCommand {
    pub provider_idempotency_key: String,
    pub payment_method: BillingPaymentMethod,
    pub payment_method_reference: String,
    pub customer_reference: Option<String>,
    pub amount_minor: i64,
    pub currency: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ConnectorCollectionState {
    Pending,
    RequiresAction,
    Failed,
    Uncertain,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConnectorCollectionResult {
    pub provider_payment_id: Option<String>,
    pub state: ConnectorCollectionState,
    pub failure_code: Option<String>,
    pub next_action_url: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SetupSessionCommand {
    pub customer_reference: String,
    pub client_reference_id: String,
    pub billing_connection_id: String,
    pub success_url: String,
    pub cancel_url: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SetupSessionResult {
    pub provider_setup_id: String,
    pub redirect_url: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HostedPaymentSessionCommand {
    pub customer_reference: String,
    pub client_reference_id: String,
    pub collection_request_id: String,
    pub amount_minor: i64,
    pub currency: String,
    pub success_url: String,
    pub cancel_url: String,
    pub expires_at: i64,
    pub provider_idempotency_key: String,
    pub allow_payment_method_save: bool,
    pub is_subscription: bool,
    pub customer_plan_id: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HostedPaymentSessionResult {
    pub provider_session_id: String,
    pub redirect_url: String,
}

pub type HostedPaymentSessionFuture<'a> = Pin<
    Box<dyn Future<Output = Result<HostedPaymentSessionResult, BillingConnectorError>> + Send + 'a>,
>;

#[derive(Clone, Debug, thiserror::Error)]
#[error("billing connector rejected collection: {code}: {message}")]
pub struct BillingConnectorError {
    pub code: String,
    pub message: String,
    pub retryable: bool,
    pub outcome_uncertain: bool,
}

pub type ConnectorFuture<'a> = Pin<
    Box<dyn Future<Output = Result<ConnectorCollectionResult, BillingConnectorError>> + Send + 'a>,
>;

pub type SetupSessionFuture<'a> =
    Pin<Box<dyn Future<Output = Result<SetupSessionResult, BillingConnectorError>> + Send + 'a>>;

pub trait BillingConnector: Send + Sync {
    /// Declares payment methods supported before any external request is made.
    fn capabilities(&self) -> BillingCapabilities;

    /// Starts the single provider attempt represented by the stable idempotency key.
    fn start_collection<'a>(&'a self, command: &'a CollectionCommand) -> ConnectorFuture<'a>;

    fn create_setup_session<'a>(
        &'a self,
        _command: &'a SetupSessionCommand,
    ) -> SetupSessionFuture<'a> {
        Box::pin(async {
            Err(BillingConnectorError {
                code: "setup_session_not_supported".to_string(),
                message: "connector does not implement setup sessions".to_string(),
                retryable: false,
                outcome_uncertain: false,
            })
        })
    }

    fn create_hosted_payment_session<'a>(
        &'a self,
        _command: &'a HostedPaymentSessionCommand,
    ) -> HostedPaymentSessionFuture<'a> {
        Box::pin(async {
            Err(BillingConnectorError {
                code: "hosted_payment_not_supported".to_string(),
                message: "connector does not implement hosted payments".to_string(),
                retryable: false,
                outcome_uncertain: false,
            })
        })
    }
}
