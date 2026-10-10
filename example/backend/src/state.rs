use sqlx::SqlitePool;

use crate::api::SubscriptionClient;

#[derive(Clone)]
pub struct AppState {
    pub pool: SqlitePool,
    pub subscription: SubscriptionClient,
    pub accounts_webhook_secret: String,
    pub app_public_url: String,
}

impl AppState {
    pub fn new(
        pool: SqlitePool,
        subscription: SubscriptionClient,
        accounts_webhook_secret: String,
    ) -> Self {
        Self {
            pool,
            subscription,
            accounts_webhook_secret,
            app_public_url: "http://localhost:5174".into(),
        }
    }

    pub fn with_app_public_url(mut self, app_public_url: String) -> Self {
        self.app_public_url = app_public_url;
        self
    }
}

#[cfg(test)]
mod tests {
    use sqlx::sqlite::SqlitePoolOptions;

    use super::AppState;
    use crate::api::SubscriptionClient;

    #[tokio::test]
    async fn public_app_url_can_be_set_for_hosted_setup_returns() {
        let pool = SqlitePoolOptions::new()
            .connect_lazy("sqlite::memory:")
            .expect("lazy database pool");
        let subscription = SubscriptionClient::new("http://127.0.0.1:3000").expect("API URL");
        let state = AppState::new(pool, subscription, "secret".into())
            .with_app_public_url("https://tasklab.example".into());

        assert_eq!(state.app_public_url, "https://tasklab.example");
    }
}
