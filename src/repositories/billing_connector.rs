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

pub trait BillingConnector: Send + Sync {
    /// Declares payment methods supported before any external request is made.
    fn capabilities(&self) -> BillingCapabilities;

    /// Starts the single provider attempt represented by the stable idempotency key.
    fn start_collection<'a>(&'a self, command: &'a CollectionCommand) -> ConnectorFuture<'a>;
}
