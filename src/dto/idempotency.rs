use serde::Serialize;
use utoipa::ToSchema;
use uuid::Uuid;

/// Identifies the committed operation behind a duplicate, without copying its payload.
#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct ExistingOperationReference {
    pub account_id: Uuid,
    pub operation_kind: String,
    pub resource_id: Uuid,
    pub transaction_id: String,
}
