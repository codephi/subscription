use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use utoipa::ToSchema;
use uuid::Uuid;

#[derive(Clone, Copy, Debug, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceEventType {
    #[serde(rename = "workspace.created")]
    Created,
    #[serde(rename = "workspace.activated")]
    Activated,
    #[serde(rename = "workspace.blocked")]
    Blocked,
    #[serde(rename = "workspace.terminated")]
    Terminated,
}

impl WorkspaceEventType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Created => "workspace.created",
            Self::Activated => "workspace.activated",
            Self::Blocked => "workspace.blocked",
            Self::Terminated => "workspace.terminated",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, ToSchema)]
pub struct WorkspaceEventPayload {
    pub workspace_id: Uuid,
}

#[derive(Clone, Debug, Deserialize, Serialize, ToSchema)]
pub struct WorkspaceEventEnvelope {
    pub event_id: Uuid,
    pub event_type: WorkspaceEventType,
    pub schema_version: u16,
    pub aggregate_id: Uuid,
    pub sequence: i64,
    pub occurred_at: DateTime<Utc>,
    pub workspace_id: Uuid,
    pub correlation_id: Uuid,
    pub causation_id: Option<Uuid>,
    pub payload: WorkspaceEventPayload,
}

#[derive(Clone, Copy, Debug, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceEventOutcome {
    Applied,
    Duplicate,
    Stale,
    Quarantined,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct WorkspaceEventResponse {
    pub event_id: Uuid,
    pub outcome: WorkspaceEventOutcome,
    pub workspace_status: Option<String>,
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
    pub workspace_id: Uuid,
    pub correlation_id: Uuid,
    pub causation_id: Option<Uuid>,
    pub payload: Value,
}
