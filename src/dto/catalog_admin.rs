use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

#[derive(Clone, Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct CatalogPageQuery {
    pub cursor: Option<Uuid>,
    pub limit: Option<u16>,
    pub parent_id: Option<Uuid>,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct CatalogEntryResponse {
    pub id: Uuid,
    pub kind: String,
    pub parent_id: Option<Uuid>,
    pub name: String,
    pub status: String,
    pub created_at: DateTime<Utc>,
    pub published_at: Option<DateTime<Utc>>,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct CatalogPageResponse {
    pub items: Vec<CatalogEntryResponse>,
    pub next_cursor: Option<Uuid>,
}
