use axum::{http::StatusCode, response::IntoResponse, Json};
use serde::Serialize;
use sqlx::Error as SqlxError;
use thiserror::Error;
use utoipa::ToSchema;

pub type ApiResult<T> = Result<T, ApiError>;

#[derive(Debug, Error)]
pub enum ApiError {
    #[error("bad request: {0}")]
    BadRequest(String),
    #[error("not found: {0}")]
    NotFound(String),
    #[error("{code}: {message}")]
    Domain {
        status: StatusCode,
        code: &'static str,
        message: String,
    },
    #[error(transparent)]
    Database(#[from] SqlxError),
    #[error(transparent)]
    Unexpected(#[from] anyhow::Error),
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ErrorResponse {
    pub error: ErrorBody,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ErrorBody {
    pub code: String,
    pub message: String,
}

impl ApiError {
    pub fn code(&self) -> &str {
        match self {
            Self::BadRequest(_) => "bad_request",
            Self::NotFound(_) => "not_found",
            Self::Domain { code, .. } => code,
            Self::Database(_) => "database_error",
            Self::Unexpected(_) => "unexpected_error",
        }
    }

    pub fn unauthorized(code: &'static str, message: impl Into<String>) -> Self {
        Self::domain(StatusCode::UNAUTHORIZED, code, message)
    }

    pub fn unprocessable(code: &'static str, message: impl Into<String>) -> Self {
        Self::domain(StatusCode::UNPROCESSABLE_ENTITY, code, message)
    }

    pub fn conflict(code: &'static str, message: impl Into<String>) -> Self {
        Self::domain(StatusCode::CONFLICT, code, message)
    }

    pub fn service_unavailable(code: &'static str, message: impl Into<String>) -> Self {
        Self::domain(StatusCode::SERVICE_UNAVAILABLE, code, message)
    }

    pub fn external(code: &'static str, message: impl Into<String>) -> Self {
        Self::domain(StatusCode::BAD_GATEWAY, code, message)
    }

    pub fn invalid_json(error: serde_json::Error) -> Self {
        Self::domain(
            StatusCode::BAD_REQUEST,
            "invalid_json",
            format!("request body must be valid JSON: {error}"),
        )
    }

    pub fn serialization(error: serde_json::Error) -> Self {
        Self::Unexpected(anyhow::anyhow!(error))
    }

    pub fn unexpected(message: impl Into<String>) -> Self {
        Self::Unexpected(anyhow::anyhow!(message.into()))
    }

    fn domain(status: StatusCode, code: &'static str, message: impl Into<String>) -> Self {
        Self::Domain {
            status,
            code,
            message: message.into(),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> axum::response::Response {
        use ApiError::*;

        let (status, code, message) = match &self {
            BadRequest(message) => (StatusCode::BAD_REQUEST, self.code(), message.clone()),
            NotFound(message) => (StatusCode::NOT_FOUND, self.code(), message.clone()),
            Domain {
                status,
                code,
                message,
            } => (*status, *code, message.clone()),
            Database(error) => match error {
                SqlxError::RowNotFound => (
                    StatusCode::NOT_FOUND,
                    "not_found",
                    "resource not found".to_string(),
                ),
                SqlxError::PoolTimedOut | SqlxError::PoolClosed => (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "database_connection_error",
                    "database connection error".to_string(),
                ),
                SqlxError::Database(db_error) => match db_error.code().as_deref() {
                    Some("23505") => (
                        StatusCode::CONFLICT,
                        "unique_constraint_violated",
                        "unique constraint violated".to_string(),
                    ),
                    Some("23514") => (
                        StatusCode::BAD_REQUEST,
                        "check_constraint_violated",
                        db_error.message().to_string(),
                    ),
                    Some("23503") => (
                        StatusCode::BAD_REQUEST,
                        "foreign_key_constraint_violated",
                        "foreign key constraint violated".to_string(),
                    ),
                    Some("02000") => (
                        StatusCode::NOT_FOUND,
                        "not_found",
                        "resource not found".to_string(),
                    ),
                    _ => (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "database_error",
                        "database error".to_string(),
                    ),
                },
                _ => (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "database_error",
                    "database error".to_string(),
                ),
            },
            Unexpected(_) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "unexpected_error",
                "unexpected error".to_string(),
            ),
        };

        let payload = Json(ErrorResponse {
            error: ErrorBody {
                code: code.to_string(),
                message,
            },
        });
        (status, payload).into_response()
    }
}
