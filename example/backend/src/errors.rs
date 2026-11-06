use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::Serialize;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("usuário ou senha inválidos")]
    InvalidCredentials,
    #[error("sessão inválida ou expirada")]
    Unauthorized,
    #[error("conflito: {0}")]
    Conflict(String),
    #[error("solicitação inválida: {0}")]
    Invalid(String),
    #[error("integração indisponível: {0}")]
    Integration(String),
    #[error(transparent)]
    Internal(#[from] anyhow::Error),
    #[error(transparent)]
    Database(#[from] sqlx::Error),
}

#[derive(Serialize)]
struct ErrorBody {
    message: String,
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let status = match self {
            Self::InvalidCredentials | Self::Unauthorized => StatusCode::UNAUTHORIZED,
            Self::Conflict(_) => StatusCode::CONFLICT,
            Self::Invalid(_) => StatusCode::UNPROCESSABLE_ENTITY,
            Self::Integration(_) => StatusCode::BAD_GATEWAY,
            Self::Internal(_) | Self::Database(_) => StatusCode::INTERNAL_SERVER_ERROR,
        };
        let body = ErrorBody {
            message: self.to_string(),
        };
        (status, Json(body)).into_response()
    }
}
