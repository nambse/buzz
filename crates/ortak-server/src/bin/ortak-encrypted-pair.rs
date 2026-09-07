#![deny(unsafe_code)]
//! Explicit operator registration of one existing encrypted human/employee pair.

use std::time::Duration;

use ortak_control::{office_identity::OfficePublicKey, ports::CompanyDirectory, PgControlPlane};
use ortak_domain::{CredentialRef, EmployeeId};
use ortak_office::encrypted::jobs::{ConfiguredDmPair, PgDecryptJobs};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Config {
    community_id: Uuid,
    selection_id: Uuid,
    channel_id: Uuid,
    employee_id: EmployeeId,
    human_public_key: OfficePublicKey,
    employee_public_key: OfficePublicKey,
    office_binding_id: Uuid,
    key_version: i64,
    decrypt_ref: CredentialRef,
}

fn parse(enabled: Option<&str>, raw: &str) -> Result<Config, &'static str> {
    if enabled != Some("true") || raw.len() > 4096 {
        return Err("explicit_bounded_encrypted_pair_selection_required");
    }
    let config: Config = serde_json::from_str(raw).map_err(|_| "invalid_pair_selection")?;
    if [
        config.community_id,
        config.selection_id,
        config.channel_id,
        config.office_binding_id,
    ]
    .iter()
    .any(Uuid::is_nil)
        || config.key_version < 0
        || config.human_public_key == config.employee_public_key
    {
        return Err("invalid_pair_identity");
    }
    Ok(config)
}

async fn run() -> Result<serde_json::Value, &'static str> {
    let enabled = std::env::var("ORTAK_ENCRYPTED_PAIR_ENABLED").ok();
    let raw =
        std::env::var("ORTAK_ENCRYPTED_PAIR_CONFIG_JSON").map_err(|_| "pair_selection_required")?;
    // All public input checks precede database selection. No signer/OAuth is read.
    let config = parse(enabled.as_deref(), &raw)?;
    let database =
        std::env::var("ORTAK_DATABASE_URL").map_err(|_| "database_selection_required")?;
    let pool = ortak_server::connect_private_database(&database)
        .await
        .map_err(|_| "database_unavailable")?;
    let control = PgControlPlane::new(pool.clone());
    let scope = control
        .resolve_company_for_community(config.community_id)
        .await
        .map_err(|_| "company_unavailable")?;
    let pair = ConfiguredDmPair {
        selection_id: config.selection_id,
        channel_id: config.channel_id,
        employee_id: config.employee_id,
        human_public_key: config.human_public_key,
        employee_public_key: config.employee_public_key,
        office_binding_id: config.office_binding_id,
        key_version: config.key_version,
        decrypt_ref: config.decrypt_ref,
    };
    // Canonical membership, identity, lifecycle, cohort and retained tuple checks
    // remain in the production registration transaction. Replay never re-enables.
    let generation = PgDecryptJobs::new(pool)
        .register_pair(&scope, &pair)
        .await
        .map_err(|_| "pair_registration_refused")?;
    Ok(
        json!({"ok":true,"selection_id":pair.selection_id,"generation":generation,
        "company_id":scope.company_id(),"channel_id":pair.channel_id,"worker_configuration_changed":false}),
    )
}

#[tokio::main]
async fn main() {
    use ortak_server::shutdown::{Outcome, Shutdown};
    let result = match Shutdown::install() {
        Ok(mut shutdown) => {
            shutdown
                .until(tokio::time::timeout(Duration::from_secs(30), run()))
                .await
        }
        Err(_) => {
            eprintln!("encrypted_pair_shutdown_registration_failed");
            std::process::exit(1);
        }
    };
    match result {
        Ok(Outcome::Completed(Ok(Ok(value)))) => println!("{value}"),
        _ => {
            eprintln!("encrypted_pair_unconfirmed_inspect_and_retry_same_selection");
            std::process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_closed_pair_configuration_is_checked_before_any_database_io() {
        let valid = json!({"community_id":Uuid::new_v4(),"selection_id":Uuid::new_v4(),
            "channel_id":Uuid::new_v4(),"employee_id":"deniz-private",
            "human_public_key":"11".repeat(32),"employee_public_key":"22".repeat(32),
            "office_binding_id":Uuid::new_v4(),"key_version":0,"decrypt_ref":"secret://fixture/office"});
        assert!(parse(Some("true"), &valid.to_string()).is_ok());
        assert!(parse(None, &valid.to_string()).is_err());
        assert!(parse(Some("true"), &" ".repeat(4097)).is_err());
        for (field, value) in [
            ("channel_id", json!(Uuid::nil())),
            ("key_version", json!(-1)),
            ("human_public_key", valid["employee_public_key"].clone()),
            ("secret_key", json!("fixture")),
            ("company_id", json!(Uuid::new_v4())),
        ] {
            let mut changed = valid.clone();
            changed[field] = value;
            assert!(
                parse(Some("true"), &changed.to_string()).is_err(),
                "{field}"
            );
        }
    }
}
