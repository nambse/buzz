//! Shared database connection bounds for the private API and runtime worker.

use std::time::Duration;

use sqlx::{
    postgres::{PgConnectOptions, PgPoolOptions},
    PgPool,
};

/// Connects a private API or worker pool with server-enforced connection bounds.
///
/// Row locks wait at most 500ms, statements at most five seconds, and idle
/// transactions at most ten seconds. These defaults also protect routing,
/// dispatch, event ingestion, authenticated reads and cancellation paths without
/// local overrides. An API authority transaction that idles past its bound cannot
/// commit or release a successful response after its authorization fence is lost.
/// Operations may impose stricter transaction-local bounds. At most eight
/// connections are opened; pool acquisition waits at most five seconds.
/// This function does not run migrations or initialize any external service.
pub async fn connect_private_database(url: &str) -> Result<PgPool, sqlx::Error> {
    let options: PgConnectOptions = url.parse()?;
    let options = options.options([
        ("statement_timeout", "5s"),
        ("lock_timeout", "500ms"),
        ("idle_in_transaction_session_timeout", "10s"),
    ]);
    PgPoolOptions::new()
        .max_connections(8)
        .acquire_timeout(Duration::from_secs(5))
        .connect_with(options)
        .await
}

/// Connects the worker using the same bounds as the private API.
///
/// Kept as a compatibility entry point for worker callers; all policy lives in
/// [`connect_private_database`].
pub async fn connect_worker_database(url: &str) -> Result<PgPool, sqlx::Error> {
    connect_private_database(url).await
}
