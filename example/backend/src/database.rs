use anyhow::Result;
use argon2::{
    password_hash::{rand_core::OsRng, PasswordHasher, SaltString},
    Argon2,
};
use sqlx::{
    sqlite::{SqliteConnectOptions, SqlitePoolOptions},
    SqlitePool,
};
use std::str::FromStr;
use uuid::Uuid;

pub async fn connect(database_url: &str) -> Result<SqlitePool> {
    let options = SqliteConnectOptions::from_str(database_url)?
        .create_if_missing(true)
        .foreign_keys(true);
    let pool = SqlitePoolOptions::new()
        .max_connections(4)
        .connect_with(options)
        .await?;
    initialize(&pool).await?;
    Ok(pool)
}

pub async fn initialize(pool: &SqlitePool) -> Result<()> {
    sqlx::migrate!("./migrations").run(pool).await?;
    seed_admin(pool).await
}

async fn seed_admin(pool: &SqlitePool) -> Result<()> {
    let password_hash = hash_password("admin")?;
    sqlx::query(
        "INSERT INTO users (user_id,username,password_hash,account_id,created_event_id, \
        activated_event_id,correlation_id) VALUES ($1,'admin',$2,$3,$4,$5,$6) \
        ON CONFLICT(username) DO NOTHING",
    )
    .bind(Uuid::new_v4().to_string())
    .bind(password_hash)
    .bind(Uuid::new_v4().to_string())
    .bind(Uuid::new_v4().to_string())
    .bind(Uuid::new_v4().to_string())
    .bind(Uuid::new_v4().to_string())
    .execute(pool)
    .await?;
    Ok(())
}

pub fn hash_password(password: &str) -> Result<String> {
    let salt = SaltString::generate(&mut OsRng);
    Ok(Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map_err(|error| anyhow::anyhow!(error.to_string()))?
        .to_string())
}
