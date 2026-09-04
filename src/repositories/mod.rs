//! Persistence and integration-facing structs live here.

pub mod catalog;
mod catalog_rows;
mod credit_rows;
mod credit_writes;
pub mod credits;
pub mod database;
pub mod outbox;
mod wallet_rows;
pub mod wallets;
pub mod workspace_events;
