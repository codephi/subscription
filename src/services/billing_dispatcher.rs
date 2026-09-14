use std::time::Duration;

use crate::{
    repositories::{database::DatabaseRepository, stripe::StripeConnector},
    services::billing,
};

pub async fn run_billing_dispatcher(repository: DatabaseRepository) {
    loop {
        if let Err(error) = dispatch_once(&repository).await {
            tracing::warn!(
                error_code = error.code(),
                "billing dispatcher iteration failed"
            );
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}

async fn dispatch_once(repository: &DatabaseRepository) -> crate::error::ApiResult<()> {
    let now = repository.current_time().await?;
    billing::expire_collections(repository, now).await?;
    let Some(due) = repository.find_due_collection_attempt().await? else {
        return Ok(());
    };
    if due.provider != "STRIPE" {
        return Ok(());
    }
    let secret = billing::resolve_secret(&due.secret_reference)?;
    let connected_account = due
        .external_account_reference
        .starts_with("acct_")
        .then_some(due.external_account_reference);
    let connector = StripeConnector::new(secret, connected_account);
    let _ = billing::execute_collection_attempt(repository, &connector, due.collection_request_id)
        .await?;
    Ok(())
}
