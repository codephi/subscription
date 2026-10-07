use argon2::{
    password_hash::{PasswordHash, PasswordVerifier},
    Argon2,
};
use axum::http::{header, HeaderMap, HeaderValue};
use chrono::{Duration, Utc};
use sha2::{Digest, Sha256};
use sqlx::{FromRow, SqlitePool};
use uuid::Uuid;

use crate::{database::hash_password, errors::AppError, models::AccountResponse, state::AppState};

#[derive(Clone, Debug, FromRow)]
pub struct AuthenticatedUser {
    pub user_id: String,
    pub username: String,
    password_hash: String,
    pub account_id: String,
    pub created_event_id: String,
    pub activated_event_id: String,
    pub correlation_id: String,
    pub event_occurred_at: String,
    pub plan_model: Option<String>,
    pub customer_plan_id: Option<String>,
}

pub async fn register(
    pool: &SqlitePool,
    username: &str,
    password: &str,
) -> Result<AuthenticatedUser, AppError> {
    validate_credentials(username, password)?;
    let user = new_user(username, password)?;
    insert_user(pool, &user).await?;
    load_by_id(pool, &user.user_id).await
}

pub async fn authenticate(
    pool: &SqlitePool,
    username: &str,
    password: &str,
) -> Result<AuthenticatedUser, AppError> {
    let user = load_by_username(pool, username).await?;
    verify_password(&user, password)
}

pub async fn current_user(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<AuthenticatedUser, AppError> {
    let token = cookie_token(headers).ok_or(AppError::Unauthorized)?;
    let session_hash = hash_token(token);
    let query = "SELECT u.* FROM sessions s JOIN users u USING(user_id) \
        WHERE s.session_hash=$1 AND CAST(s.expires_at AS INTEGER)>unixepoch()";
    sqlx::query_as::<_, AuthenticatedUser>(query)
        .bind(session_hash)
        .fetch_optional(&state.pool)
        .await?
        .ok_or(AppError::Unauthorized)
}

pub async fn create_session(
    pool: &SqlitePool,
    user: &AuthenticatedUser,
) -> Result<HeaderValue, AppError> {
    let token = Uuid::new_v4().to_string();
    let expires_at = (Utc::now() + Duration::hours(8)).timestamp().to_string();
    sqlx::query("INSERT INTO sessions (session_hash,user_id,expires_at) VALUES ($1,$2,$3)")
        .bind(hash_token(&token))
        .bind(&user.user_id)
        .bind(expires_at)
        .execute(pool)
        .await?;
    let cookie = format!("tasklab_session={token}; HttpOnly; SameSite=Lax; Path=/; Max-Age=28800");
    HeaderValue::from_str(&cookie).map_err(|error| AppError::Internal(error.into()))
}

pub async fn delete_session(
    pool: &SqlitePool,
    headers: &HeaderMap,
) -> Result<HeaderValue, AppError> {
    if let Some(token) = cookie_token(headers) {
        sqlx::query("DELETE FROM sessions WHERE session_hash=$1")
            .bind(hash_token(token))
            .execute(pool)
            .await?;
    }
    Ok(HeaderValue::from_static(
        "tasklab_session=; HttpOnly; SameSite=Lax; Path=/; Max-Age=0",
    ))
}

pub fn account_response(user: AuthenticatedUser) -> Result<AccountResponse, AppError> {
    let plan_model = user.plan_model.as_deref().map(parse_plan).transpose()?;
    Ok(AccountResponse {
        user_id: parse_uuid("user_id", &user.user_id)?,
        username: user.username,
        plan_model,
        customer_plan_id: user
            .customer_plan_id
            .as_deref()
            .map(|value| parse_uuid("customer_plan_id", value))
            .transpose()?,
        account_id: parse_uuid("account_id", &user.account_id)?,
    })
}

async fn insert_user(pool: &SqlitePool, user: &NewUser) -> Result<(), AppError> {
    sqlx::query(
        "INSERT INTO users (user_id,username,password_hash,account_id,created_event_id, \
        activated_event_id,correlation_id) VALUES ($1,$2,$3,$4,$5,$6,$7)",
    )
    .bind(&user.user_id)
    .bind(&user.username)
    .bind(&user.password_hash)
    .bind(&user.account_id)
    .bind(&user.created_event_id)
    .bind(&user.activated_event_id)
    .bind(&user.correlation_id)
    .execute(pool)
    .await
    .map_err(map_register_error)?;
    Ok(())
}

async fn load_by_username(
    pool: &SqlitePool,
    username: &str,
) -> Result<AuthenticatedUser, AppError> {
    sqlx::query_as::<_, AuthenticatedUser>("SELECT * FROM users WHERE username=?")
        .bind(username)
        .fetch_optional(pool)
        .await?
        .ok_or(AppError::InvalidCredentials)
}

async fn load_by_id(pool: &SqlitePool, user_id: &str) -> Result<AuthenticatedUser, AppError> {
    sqlx::query_as::<_, AuthenticatedUser>("SELECT * FROM users WHERE user_id=?")
        .bind(user_id)
        .fetch_one(pool)
        .await
        .map_err(AppError::from)
}

fn new_user(username: &str, password: &str) -> Result<NewUser, AppError> {
    Ok(NewUser {
        user_id: Uuid::new_v4().to_string(),
        username: username.to_string(),
        password_hash: hash_password(password).map_err(AppError::Internal)?,
        account_id: Uuid::new_v4().to_string(),
        created_event_id: Uuid::new_v4().to_string(),
        activated_event_id: Uuid::new_v4().to_string(),
        correlation_id: Uuid::new_v4().to_string(),
    })
}

fn verify_password(
    user: &AuthenticatedUser,
    password: &str,
) -> Result<AuthenticatedUser, AppError> {
    let stored = PasswordHash::new(&user.password_hash)
        .map_err(|error| AppError::Internal(anyhow::anyhow!(error.to_string())))?;
    let valid = Argon2::default()
        .verify_password(password.as_bytes(), &stored)
        .is_ok();
    if valid {
        return Ok(user.clone());
    }
    Err(AppError::InvalidCredentials)
}

fn validate_credentials(username: &str, password: &str) -> Result<(), AppError> {
    let valid_name = (3..=32).contains(&username.len())
        && username
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || "._-".contains(character));
    if valid_name && (8..=128).contains(&password.len()) {
        return Ok(());
    }
    Err(AppError::Invalid(
        "nome de usuário ou senha fora dos limites".to_string(),
    ))
}

fn cookie_token(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(header::COOKIE)?
        .to_str()
        .ok()?
        .split(';')
        .filter_map(|pair| pair.trim().split_once('='))
        .find_map(|(name, value)| (name == "tasklab_session").then_some(value))
}

fn hash_token(token: &str) -> String {
    format!("{:x}", Sha256::digest(token.as_bytes()))
}

fn map_register_error(error: sqlx::Error) -> AppError {
    if error
        .as_database_error()
        .is_some_and(|cause| cause.is_unique_violation())
    {
        return AppError::Conflict("usuário já cadastrado".to_string());
    }
    AppError::Database(error)
}

fn parse_uuid(field: &str, value: &str) -> Result<Uuid, AppError> {
    Uuid::parse_str(value)
        .map_err(|_| AppError::Internal(anyhow::anyhow!("SQLite contém UUID inválido em {field}")))
}

fn parse_plan(value: &str) -> Result<crate::models::PlanModel, AppError> {
    match value {
        "PREPAID" => Ok(crate::models::PlanModel::Prepaid),
        "SUBSCRIPTION" => Ok(crate::models::PlanModel::Subscription),
        _ => Err(AppError::Internal(anyhow::anyhow!(
            "modelo de conta inválido: {value}"
        ))),
    }
}

struct NewUser {
    user_id: String,
    username: String,
    password_hash: String,
    account_id: String,
    created_event_id: String,
    activated_event_id: String,
    correlation_id: String,
}
