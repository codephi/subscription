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
    repository.schedule_due_paid_renewals(now).await?;
    billing::expire_collections(repository, now).await?;
    let Some(due) = repository.find_due_collection_attempt().await? else {
        return Ok(());
    };
    if due.provider != "STRIPE" {
        return Ok(());
    }
    let managed = due.secret_reference.starts_with("v1:");
    let secret = billing::resolve_connection_secret(
        repository,
        due.account_id,
        due.billing_connection_id,
        "stripe_api",
        &due.secret_reference,
        managed,
    )?;
    let connected_account = if managed {
        None
    } else {
        due.external_account_reference
            .starts_with("acct_")
            .then_some(due.external_account_reference)
    };
    let connector = StripeConnector::new(secret, connected_account);
    let _ = billing::execute_collection_attempt(repository, &connector, due.collection_request_id)
        .await?;
    Ok(())
}
