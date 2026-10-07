use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use utoipa::ToSchema;
use uuid::Uuid;

#[derive(Clone, Copy, Debug, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum AccountEventType {
    #[serde(rename = "account.created")]
    Created,
    #[serde(rename = "account.activated")]
    Activated,
    #[serde(rename = "account.blocked")]
    Blocked,
    #[serde(rename = "account.terminated")]
    Terminated,
}

impl AccountEventType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Created => "account.created",
            Self::Activated => "account.activated",
            Self::Blocked => "account.blocked",
            Self::Terminated => "account.terminated",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, ToSchema)]
pub struct AccountEventPayload {
    pub account_id: Uuid,
}

#[derive(Clone, Debug, Deserialize, Serialize, ToSchema)]
pub struct AccountEventEnvelope {
    pub event_id: Uuid,
    pub event_type: AccountEventType,
    pub schema_version: u16,
    pub aggregate_id: Uuid,
    pub sequence: i64,
    pub occurred_at: DateTime<Utc>,
    pub account_id: Uuid,
    pub correlation_id: Uuid,
    pub causation_id: Option<Uuid>,
    pub payload: AccountEventPayload,
}

#[derive(Clone, Copy, Debug, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum AccountEventOutcome {
    Applied,
    Duplicate,
    Stale,
    Quarantined,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct AccountEventResponse {
    pub event_id: Uuid,
    pub outcome: AccountEventOutcome,
    pub account_status: Option<String>,
    pub external_sequence: Option<i64>,
}

#[derive(Clone, Debug, Serialize)]
pub struct DomainEventEnvelope {
    pub event_id: Uuid,
    pub event_type: String,
    pub schema_version: u16,
    pub aggregate_type: String,
    pub aggregate_id: Uuid,
    pub sequence: i64,
    pub occurred_at: DateTime<Utc>,
    pub account_id: Uuid,
    pub correlation_id: Uuid,
    pub causation_id: Option<Uuid>,
    pub payload: Value,
}
