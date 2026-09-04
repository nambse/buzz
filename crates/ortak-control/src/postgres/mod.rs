//! sqlx PostgreSQL adapters for the Milestone 1 control-plane schema.
//!
//! Every statement is scoped by the resolved company id and uses runtime
//! `sqlx::query` (no compile-time database), matching the inherited crates.

mod company;
mod inbox;
mod outbox;
mod provisioning;
mod routing;
mod run_events;

use serde::de::DeserializeOwned;
use serde::Serialize;
use sqlx::PgPool;

use crate::error::{ControlError, Result};

pub use inbox::insert_accepted_event_on;

/// PostgreSQL implementation of every control-plane repository port.
#[derive(Clone, Debug)]
pub struct PgControlPlane {
    pool: PgPool,
}

impl PgControlPlane {
    /// Wraps a connection pool.
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Returns the underlying pool.
    pub fn pool(&self) -> &PgPool {
        &self.pool
    }
}

/// Serializes a closed-vocabulary enum to its snake_case column value.
pub(crate) fn column_value<T: Serialize>(value: &T) -> Result<String> {
    match serde_json::to_value(value)? {
        serde_json::Value::String(text) => Ok(text),
        other => Err(ControlError::InvalidData(format!(
            "expected a string enum value, got {other}"
        ))),
    }
}

/// Parses a snake_case column value into a closed-vocabulary enum.
pub(crate) fn parse_column<T: DeserializeOwned>(column: &str, value: &str) -> Result<T> {
    serde_json::from_value(serde_json::Value::String(value.to_owned()))
        .map_err(|_| ControlError::InvalidData(format!("{column} holds unknown value {value:?}")))
}

/// Converts a 32-byte `BYTEA` column into a fixed array.
pub(crate) fn bytes32(column: &str, value: &[u8]) -> Result<[u8; 32]> {
    <[u8; 32]>::try_from(value)
        .map_err(|_| ControlError::InvalidData(format!("{column} must be 32 bytes")))
}

/// Interval seconds for `make_interval(secs => $n)`.
pub(crate) fn interval_seconds(duration: std::time::Duration) -> f64 {
    duration.as_secs_f64()
}

/// Returns true when the error is a unique-constraint violation.
pub(crate) fn is_unique_violation(error: &sqlx::Error) -> bool {
    error
        .as_database_error()
        .is_some_and(|database| database.is_unique_violation())
}
