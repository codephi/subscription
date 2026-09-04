use uuid::Uuid;

use crate::{
    dto::wallets::{WalletHierarchyResponse, WalletProvisioningResponse},
    error::ApiResult,
    repositories::database::DatabaseRepository,
};

pub async fn get_wallets(
    repository: &DatabaseRepository,
    workspace_id: Uuid,
) -> ApiResult<WalletHierarchyResponse> {
    repository.find_wallet_hierarchy(workspace_id).await
}

pub async fn get_provisioning(
    repository: &DatabaseRepository,
    workspace_id: Uuid,
) -> ApiResult<WalletProvisioningResponse> {
    repository.find_wallet_provisioning(workspace_id).await
}

pub async fn reconcile(
    repository: &DatabaseRepository,
    workspace_id: Uuid,
) -> ApiResult<WalletProvisioningResponse> {
    repository
        .reconcile_wallets(workspace_id, Some("open-admin-route"))
        .await
}
