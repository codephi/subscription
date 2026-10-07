use uuid::Uuid;

use crate::{
    dto::wallets::{WalletHierarchyResponse, WalletProvisioningResponse},
    error::ApiResult,
    repositories::database::DatabaseRepository,
};

pub async fn get_wallets(
    repository: &DatabaseRepository,
    account_id: Uuid,
) -> ApiResult<WalletHierarchyResponse> {
    repository.find_wallet_hierarchy(account_id).await
}

pub async fn get_provisioning(
    repository: &DatabaseRepository,
    account_id: Uuid,
) -> ApiResult<WalletProvisioningResponse> {
    repository.find_wallet_provisioning(account_id).await
}

pub async fn reconcile(
    repository: &DatabaseRepository,
    account_id: Uuid,
) -> ApiResult<WalletProvisioningResponse> {
    repository
        .reconcile_wallets(account_id, Some("open-admin-route"))
        .await
}
