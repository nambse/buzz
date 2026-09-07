#![deny(unsafe_code)]
//! Default-off operator command for one durable prepared-resource activation.

use std::time::Duration;

use ortak_server::shutdown::{Outcome, Shutdown};

#[tokio::main]
async fn main() {
    let result = match Shutdown::install() {
        Ok(mut shutdown) => shutdown.until(run()).await,
        Err(_) => {
            eprintln!("ortak-provision: shutdown registration failed");
            std::process::exit(1);
        }
    };
    match result {
        Ok(Outcome::Completed(Ok(()))) => {}
        Ok(Outcome::Completed(Err(code))) => {
            eprintln!("ortak-provision: {code}");
            std::process::exit(1);
        }
        _ => {
            eprintln!("ortak-provision: interrupted; replay the same configuration to recover durable state");
            std::process::exit(1);
        }
    }
}

async fn run() -> Result<(), &'static str> {
    if std::env::var("ORTAK_PROVISIONING_ENABLED").as_deref() != Ok("true") {
        return Err("explicit ORTAK_PROVISIONING_ENABLED=true is required");
    }
    let config = std::env::var("ORTAK_PROVISIONING_CONFIG_JSON")
        .map_err(|_| "provisioning configuration required")?;
    let database =
        std::env::var("ORTAK_DATABASE_URL").map_err(|_| "database selection required")?;
    let compensate = match std::env::var("ORTAK_PROVISIONING_ACTION").as_deref() {
        Ok("resume") | Err(_) => false,
        Ok("compensate") => true,
        _ => return Err("provisioning action must be resume or compensate"),
    };
    let result = tokio::time::timeout(Duration::from_secs(180), async {
        let pool = ortak_server::connect_private_database(&database)
            .await
            .map_err(|_| "database connection failed")?;
        ortak_server::provisioning::provision_once(pool, &config, compensate).await
    })
    .await
    .map_err(|_| "command deadline elapsed; replay original configuration to recover")??;
    println!(
        "{}",
        serde_json::to_string(&result).map_err(|_| "result serialization failed")?
    );
    if result.status == "failed" {
        return Err("a durable provisioning step failed; inspect the operation before retry");
    }
    Ok(())
}
