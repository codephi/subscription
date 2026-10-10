use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

#[derive(Clone, Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct AuditPageQuery {
    pub cursor: Option<Uuid>,
    pub limit: Option<u16>,
    pub account_id: Option<Uuid>,
    pub action: Option<String>,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct AuditEventResponse {
    pub audit_event_id: Uuid,
    pub account_id: Option<Uuid>,
    pub actor_reference: Option<String>,
    pub action: String,
    pub resource_kind: String,
    pub resource_id: Option<Uuid>,
    pub correlation_id: Uuid,
    #[schema(value_type = Object)]
    pub details: Value,
    pub occurred_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct AuditPageResponse {
    pub items: Vec<AuditEventResponse>,
    pub next_cursor: Option<Uuid>,
}
