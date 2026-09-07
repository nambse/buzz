//! Employee-owned export journal. The host explicitly selects this worker;
//! signed product commands share the existing private API middleware.
//! Network I/O occurs between short transactions; cleanup retains original pins.
mod commands;
pub(crate) use commands::router;
mod jobs;
mod targets;
mod types;
mod worker;
pub use jobs::{acknowledge, claim, fail, prepare};
pub use targets::{refresh_target, register_target};
pub use types::*;
pub use worker::{schedule_one, EmployeeExportAdapter, HonchoEmployeeExportAdapter};

use chrono::{DateTime, Utc};
use ortak_control::{CompanyScope, PgControlPlane};
use ortak_memory::*;
use ortak_work::{Result, WorkError};
use serde_json::{json, Value};
use sqlx::{PgConnection, Row};
use std::time::Duration;
use uuid::Uuid;

fn invalid() -> WorkError {
    WorkError::InvalidQuery("employee memory export rejected")
}
async fn bounds(c: &mut PgConnection) -> Result<()> {
    sqlx::query("SELECT set_config('lock_timeout','500ms',true),set_config('statement_timeout','2s',true),set_config('idle_in_transaction_session_timeout','5s',true)")
        .execute(c).await?;
    Ok(())
}
async fn bounded<T>(work: impl std::future::Future<Output = Result<T>>) -> Result<T> {
    tokio::time::timeout(Duration::from_secs(5), work)
        .await
        .map_err(|_| WorkError::OperationTimedOut)?
}
fn memory_error(_: ortak_control::memory::MemoryError) -> WorkError {
    invalid()
}
fn bytes(hash: &str) -> Result<Vec<u8>> {
    let value = hex::decode(hash).map_err(|_| invalid())?;
    if value.len() != 32 || hex::encode(&value) != hash {
        return Err(invalid());
    }
    Ok(value)
}
