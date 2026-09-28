use sqlx::SqlitePool;

use crate::api::SubscriptionClient;

#[derive(Clone)]
pub struct AppState {
    pub pool: SqlitePool,
    pub subscription: SubscriptionClient,
    pub accounts_webhook_secret: String,
    pub tasklab_web_url: String,
}

impl AppState {
    pub fn new(
        pool: SqlitePool,
        subscription: SubscriptionClient,
        accounts_webhook_secret: String,
        tasklab_web_url: String,
    ) -> Self {
        Self {
            pool,
            subscription,
            accounts_webhook_secret,
            tasklab_web_url,
        }
    }
}
