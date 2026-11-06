use std::time::Duration;

use chrono::{TimeDelta, Utc};
use reqwest::Client;
use uuid::Uuid;

use crate::{
    error::{ApiError, ApiResult},
    repositories::database::DatabaseRepository,
    services::signatures::{current_timestamp, sign_body},
};

#[derive(Clone, Debug)]
pub struct WebhookDestination {
    pub url: String,
    pub secret: String,
}

pub async fn run_dispatcher(
    repository: DatabaseRepository,
    destination: WebhookDestination,
    poll_interval: Duration,
) {
    let client = Client::new();
    let worker_id = Uuid::new_v4();
    loop {
        if let Err(error) = dispatch_once(&repository, &client, &destination, worker_id).await {
            tracing::error!(error = %error, "outbox dispatch failed");
        }
        tokio::time::sleep(poll_interval).await;
    }
}

pub async fn dispatch_once(
    repository: &DatabaseRepository,
    client: &Client,
    destination: &WebhookDestination,
    worker_id: Uuid,
) -> ApiResult<bool> {
    let Some(event) = repository.claim_outbox_event(worker_id).await? else {
        return Ok(false);
    };
    let body = serde_json::to_vec(&event.payload).map_err(ApiError::serialization)?;
    match deliver(client, destination, event.event_id, &body).await {
        Ok(()) => repository.mark_outbox_delivered(event.event_id).await?,
        Err(error) => mark_failure(repository, event, error).await?,
    }
    Ok(true)
}

async fn deliver(
    client: &Client,
    destination: &WebhookDestination,
    event_id: Uuid,
    body: &[u8],
) -> ApiResult<()> {
    let timestamp = current_timestamp()?;
    let signature = sign_body(&destination.secret, timestamp, body)?;
    let response = client
        .post(&destination.url)
        .header("content-type", "application/json")
        .header("x-runvibe-event-id", event_id.to_string())
        .header("x-runvibe-timestamp", timestamp.to_string())
        .header("x-runvibe-signature", signature)
        .body(body.to_vec())
        .send()
        .await
        .map_err(|error| ApiError::external("event_delivery_failed", error.to_string()))?;
    if response.status().is_success() {
        return Ok(());
    }
    Err(ApiError::external(
        "event_delivery_failed",
        format!("destination returned status {}", response.status()),
    ))
}

async fn mark_failure(
    repository: &DatabaseRepository,
    event: crate::repositories::outbox::ClaimedOutboxEvent,
    error: ApiError,
) -> ApiResult<()> {
    let attempts = event.attempts + 1;
    let delay_seconds = 2_i64.pow(u32::try_from(attempts.min(12)).unwrap_or(12));
    let available_at = Utc::now() + TimeDelta::seconds(delay_seconds.min(3600));
    repository
        .mark_outbox_failed(event.event_id, attempts, available_at, &error.to_string())
        .await
}
