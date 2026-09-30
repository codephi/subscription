mod api;
mod auth;
mod config;
mod database;
mod errors;
mod models;
mod routes;
mod services;
mod state;

use anyhow::Result;
use axum::Router;
use tokio::net::TcpListener;
use tower_http::{limit::RequestBodyLimitLayer, trace::TraceLayer};

use crate::{config::Config, state::AppState};

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();
    let config = Config::from_env()?;
    let pool = database::connect(&config.database_url).await?;
    let api = api::SubscriptionClient::new(&config.subscription_api_url)?;
    let state = AppState::new(pool, api, config.accounts_webhook_secret)
        .with_app_public_url(config.app_public_url);
    if std::env::args().nth(1).as_deref() == Some("setup") {
        services::setup_catalog(&state).await?;
        println!("Catálogo da POC configurado na Subscription.");
        return Ok(());
    }
    let app = router(state);
    let listener = TcpListener::bind((config.host.as_str(), config.port)).await?;
    println!(
        "TaskLab disponível em http://{}:{}",
        config.host, config.port
    );
    axum::serve(listener, app).await?;
    Ok(())
}

fn router(state: AppState) -> Router {
    Router::new()
        .merge(routes::router())
        .layer(RequestBodyLimitLayer::new(65_536))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}
