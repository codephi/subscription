use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

#[derive(Clone, Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct InboxPageQuery {
    pub cursor: Option<Uuid>,
    pub limit: Option<u16>,
    pub account_id: Option<Uuid>,
    pub status: Option<String>,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct InboxEventResponse {
    pub event_id: Uuid,
    pub account_id: Uuid,
    pub event_type: String,
    pub external_sequence: i64,
    pub processing_status: String,
    pub correlation_id: Uuid,
    pub received_at: DateTime<Utc>,
    pub processed_at: Option<DateTime<Utc>>,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct InboxPageResponse {
    pub items: Vec<InboxEventResponse>,
    pub next_cursor: Option<Uuid>,
}
