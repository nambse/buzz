#![deny(unsafe_code)]
//! Local/private product API entry point. Runtime workers are composed separately.

use std::{future::IntoFuture, net::SocketAddr, sync::Arc, time::Duration};

use buzz_pubsub::RedisNip98ReplayGuard;
use ortak_control::PgControlPlane;
use ortak_server::{
    connect_private_database, product_router,
    shutdown::{DrainOutcome, Outcome, Shutdown},
    ApiConfig,
};

#[tokio::main]
async fn main() {
    // Register both process signals before any configuration or connection I/O.
    let result = match Shutdown::install() {
        Ok(shutdown) => serve(shutdown).await,
        Err(_) => Err("shutdown signal registration failed"),
    };
    if let Err(message) = result {
        eprintln!("ortak-server: {message}");
        std::process::exit(1);
    }
}

async fn serve(mut shutdown: Shutdown) -> Result<(), &'static str> {
    let (listener, router) = match shutdown
        .until(prepare())
        .await
        .map_err(|_| "shutdown signal failed")?
    {
        Outcome::Completed(result) => result?,
        Outcome::Interrupted => return Ok(()),
    };
    let (stop, stopped) = tokio::sync::oneshot::channel();
    let server = axum::serve(listener, router)
        .with_graceful_shutdown(async move {
            let _ = stopped.await;
        })
        .into_future();
    match shutdown
        .drain(
            server,
            || {
                let _ = stop.send(());
            },
            Duration::from_secs(15),
        )
        .await
        .map_err(|_| "shutdown signal failed")?
    {
        DrainOutcome::Completed(result) => result.map_err(|_| "API listener failed"),
        DrainOutcome::TimedOut => {
            Err("API drain deadline elapsed; in-flight requests may require retry")
        }
    }
}

async fn prepare() -> Result<(tokio::net::TcpListener, axum::Router), &'static str> {
    let config =
        std::env::var("ORTAK_API_CONFIG_JSON").map_err(|_| "ORTAK_API_CONFIG_JSON is required")?;
    if config.len() > 65_536 {
        return Err("API configuration exceeds limit");
    }
    let config: ApiConfig =
        serde_json::from_str(&config).map_err(|_| "invalid API configuration JSON")?;
    let config = config.validate()?;
    let bind: SocketAddr = std::env::var("ORTAK_API_BIND")
        .unwrap_or_else(|_| "127.0.0.1:8787".to_owned())
        .parse()
        .map_err(|_| "invalid API bind address")?;
    if !bind.ip().is_loopback() {
        return Err("private MVP requires a loopback bind address");
    }
    let database_url =
        std::env::var("ORTAK_DATABASE_URL").map_err(|_| "ORTAK_DATABASE_URL is required")?;
    let pool = connect_private_database(&database_url)
        .await
        .map_err(|_| "database connection failed")?;
    let redis_url = std::env::var("ORTAK_REDIS_URL").map_err(|_| "ORTAK_REDIS_URL is required")?;
    let redis = deadpool_redis::Config::from_url(redis_url)
        .create_pool(Some(deadpool_redis::Runtime::Tokio1))
        .map_err(|_| "Redis pool configuration failed")?;
    let router = product_router(
        PgControlPlane::new(pool),
        config,
        Arc::new(RedisNip98ReplayGuard::new(redis)),
    )?;
    let listener = tokio::net::TcpListener::bind(bind)
        .await
        .map_err(|_| "API listener bind failed")?;
    Ok((listener, router))
}
