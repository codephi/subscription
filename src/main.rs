use anyhow::Result;
use subscription::{
    config::AppConfig,
    db::{init_pool, run_migrations},
    libs::telemetry,
    repositories::database::DatabaseRepository,
    routes::create_router,
    services::outbox::{self, WebhookDestination},
    state::AppState,
};
use tokio::net::TcpListener;

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();
    let config = AppConfig::from_env()?;
    let _telemetry = telemetry::init_tracing(config.otel_enabled)?;
    let pool = init_pool(&config.database_url).await?;

    if let Err(error) = run_migrations(&pool).await {
        tracing::error!("failed to run database migrations: {error}");
        return Err(error);
    }

    let repository = DatabaseRepository::new(pool);
    tokio::spawn(subscription::services::subscription_calendar::run_scheduler(repository.clone()));
    let state = AppState::new(repository.clone())
        .with_accounts_webhook_secret(config.accounts_webhook_secret.clone());

    if let Some(webhook) = &config.outbound_event_webhook {
        tokio::spawn(outbox::run_dispatcher(
            repository,
            WebhookDestination {
                url: webhook.url.clone(),
                secret: webhook.secret.clone(),
            },
            std::time::Duration::from_secs(1),
        ));
    }

    let router = create_router(state, &config);

    let addr = config.listen_addr()?;
    let listener = TcpListener::bind(addr).await?;
    tracing::info!("listening on {}", addr);
    if let Some(mcp_url) = config.mcp_endpoint_url() {
        tracing::info!("mcp enabled at {}", mcp_url);
    }

    axum::serve(listener, router).await?;
    Ok(())
}
