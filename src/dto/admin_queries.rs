use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

#[derive(Clone, Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct AdminPageQuery {
    pub cursor: Option<Uuid>,
    pub limit: Option<u16>,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct WorkspaceProjectionResponse {
    pub workspace_id: Uuid,
    pub operational_status: String,
    pub external_sequence: i64,
    pub external_occurred_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Deserialize, ToSchema)]
pub struct CreateWorkspaceRequest {
    pub actor_reference: String,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct WorkspacePageResponse {
    pub items: Vec<WorkspaceProjectionResponse>,
    pub next_cursor: Option<Uuid>,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct CustomerPlanPageResponse {
    pub items: Vec<crate::dto::plans::CustomerPlanResponse>,
    pub next_cursor: Option<Uuid>,
}
