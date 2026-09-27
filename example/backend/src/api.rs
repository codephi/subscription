use anyhow::Result;
use reqwest::{Client, Method, Response, Url};
use serde::Serialize;
use serde_json::Value;
use uuid::Uuid;

use crate::errors::AppError;

#[derive(Clone)]
pub struct SubscriptionClient {
    client: Client,
    base_url: Url,
}

impl SubscriptionClient {
    pub fn new(base_url: &str) -> Result<Self> {
        let base_url = Url::parse(&format!("{}/", base_url.trim_end_matches('/')))?;
        Ok(Self {
            client: Client::new(),
            base_url,
        })
    }

    pub async fn get(&self, path: &str) -> Result<Value, AppError> {
        self.send(Method::GET, path, None, None).await
    }

    pub async fn post<T: Serialize>(
        &self,
        path: &str,
        idempotency_key: Option<&str>,
        body: &T,
    ) -> Result<Value, AppError> {
        self.send(
            Method::POST,
            path,
            idempotency_key,
            Some(serde_json::to_value(body).map_err(invalid_json)?),
        )
        .await
    }

    pub async fn patch<T: Serialize>(&self, path: &str, body: &T) -> Result<Value, AppError> {
        self.send(
            Method::PATCH,
            path,
            None,
            Some(serde_json::to_value(body).map_err(invalid_json)?),
        )
        .await
    }

    pub async fn put<T: Serialize>(&self, path: &str, body: &T) -> Result<Value, AppError> {
        self.send(
            Method::PUT,
            path,
            None,
            Some(serde_json::to_value(body).map_err(invalid_json)?),
        )
        .await
    }

    pub async fn post_signed(
        &self,
        path: &str,
        body: &[u8],
        timestamp: &str,
        signature: &str,
    ) -> Result<Value, AppError> {
        let url = self
            .base_url
            .join(path.trim_start_matches('/'))
            .map_err(invalid_url)?;
        let response = self
            .client
            .post(url)
            .header("content-type", "application/json")
            .header("x-runvibe-timestamp", timestamp)
            .header("x-runvibe-signature", signature)
            .body(body.to_vec())
            .send()
            .await
            .map_err(map_transport)?;
        decode_response(response).await
    }

    async fn send(
        &self,
        method: Method,
        path: &str,
        idempotency_key: Option<&str>,
        body: Option<Value>,
    ) -> Result<Value, AppError> {
        let url = self
            .base_url
            .join(path.trim_start_matches('/'))
            .map_err(invalid_url)?;
        let response = self.request(method, url, idempotency_key, body).await?;
        decode_response(response).await
    }

    async fn request(
        &self,
        method: Method,
        url: Url,
        idempotency_key: Option<&str>,
        body: Option<Value>,
    ) -> Result<Response, AppError> {
        let mut request = self.client.request(method, url);
        if let Some(key) = idempotency_key {
            request = request.header("Idempotency-Key", key);
        }
        if let Some(body) = body {
            request = request.json(&body);
        }
        request.send().await.map_err(map_transport)
    }
}

async fn decode_response(response: Response) -> Result<Value, AppError> {
    let status = response.status();
    let body: Value = response.json().await.map_err(map_transport)?;
    if status.is_success() {
        return Ok(body);
    }
    Err(map_status(status.as_u16(), &body))
}

fn map_transport(error: reqwest::Error) -> AppError {
    AppError::Integration(format!("Subscription API: {error}"))
}

fn invalid_json(error: serde_json::Error) -> AppError {
    AppError::Internal(error.into())
}
fn invalid_url(error: url::ParseError) -> AppError {
    AppError::Invalid(format!("caminho da Subscription inválido: {error}"))
}

fn map_status(status: u16, body: &Value) -> AppError {
    let message = body
        .pointer("/error/message")
        .or_else(|| body.get("message"))
        .and_then(Value::as_str)
        .unwrap_or("Subscription API rejeitou a operação");
    if status == 409 {
        return AppError::Conflict(message.to_string());
    }
    AppError::Integration(message.to_string())
}

pub fn required_uuid(body: &Value, field: &str) -> Result<Uuid, AppError> {
    let raw = body
        .get(field)
        .and_then(Value::as_str)
        .ok_or_else(|| AppError::Integration(format!("Subscription omitiu {field}")))?;
    Uuid::parse_str(raw)
        .map_err(|_| AppError::Integration(format!("Subscription retornou {field} inválido")))
}
