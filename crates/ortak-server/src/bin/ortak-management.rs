#![deny(unsafe_code)]
//! Explicit catalog import or bounded, restartable prepared-command executor.
use ortak_control::PgControlPlane;
use ortak_server::{
    management,
    shutdown::{Outcome, Shutdown},
};
use std::time::Duration;

#[tokio::main]
async fn main() {
    let outcome = match Shutdown::install() {
        Ok(mut shutdown) => shutdown.until(run()).await,
        Err(_) => {
            eprintln!("ortak-management: shutdown unavailable");
            std::process::exit(1)
        }
    };
    match outcome {
        Ok(Outcome::Completed(Ok(()))) | Ok(Outcome::Interrupted) => {}
        _ => {
            eprintln!("ortak-management: stopped; inspect retained command state");
            std::process::exit(1)
        }
    }
}
async fn run() -> Result<(), &'static str> {
    if std::env::var("ORTAK_MANAGEMENT_ENABLED").as_deref() != Ok("true") {
        return Err("management is disabled");
    }
    let database = std::env::var("ORTAK_DATABASE_URL").map_err(|_| "database required")?;
    let control = PgControlPlane::new(
        ortak_server::connect_private_database(&database)
            .await
            .map_err(|_| "database unavailable")?,
    );
    match std::env::var("ORTAK_MANAGEMENT_ACTION").as_deref() {
        Ok("import_catalog") => {
            let json =
                std::env::var("ORTAK_PREPARED_CATALOG_JSON").map_err(|_| "catalog required")?;
            let count = management::import_prepared_catalog(&control, &json).await?;
            println!("prepared choices imported: {count}");
            Ok(())
        }
        Ok("work") => {
            let community = std::env::var("ORTAK_MANAGEMENT_COMMUNITY_ID")
                .map_err(|_| "community required")?
                .parse()
                .map_err(|_| "invalid community")?;
            let mut failures = 0u32;
            loop {
                match management::execute_next(&control, community).await {
                    Ok(_) => failures = 0,
                    Err(_) => {
                        failures = failures.saturating_add(1);
                        eprintln!("ortak-management: queue pass unavailable; retained commands will retry");
                    }
                }
                tokio::time::sleep(Duration::from_secs(if failures == 0 {
                    2
                } else {
                    (2u64.pow(failures.min(5))).min(30)
                }))
                .await;
            }
        }
        _ => Err("explicit import_catalog or work action required"),
    }
}
