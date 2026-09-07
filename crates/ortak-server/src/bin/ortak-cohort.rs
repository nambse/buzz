#![deny(unsafe_code)]
//! Explicit bounded cohort capture, reconciliation and cutover command.

use std::{collections::BTreeSet, time::Duration};

use ortak_control::{
    cohort::{MAX_INBOX_RECONCILIATION_BATCH, MAX_ROUTING_COHORT_SIZE},
    ports::CompanyDirectory,
    PgControlPlane,
};
use ortak_domain::EmployeeId;
use ortak_server::shutdown::{Outcome, Shutdown};
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Config {
    community_id: Uuid,
    action: Action,
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum Action {
    Capture {
        // The database cannot prove which relay artifact handles ingress.
        // The operator must deploy its atomic selected hook before capture.
        relay_capture_hook_installed: bool,
        channel_ids: Vec<Uuid>,
        employee_ids: Vec<EmployeeId>,
    },
    // Empty struct variants keep deny_unknown_fields active for zero-field actions.
    Status {},
    Reconcile {
        capture_id: Uuid,
        channel_id: Uuid,
        #[serde(default = "page_limit")]
        limit: u16,
    },
    Enable {
        capture_id: Uuid,
    },
    Disable {},
}

fn page_limit() -> u16 {
    MAX_INBOX_RECONCILIATION_BATCH
}

fn parse_config(enabled: Option<&str>, raw: &str) -> Result<Config, &'static str> {
    if enabled != Some("true") {
        return Err("explicit ORTAK_COHORT_ENABLED=true is required");
    }
    if raw.len() > 65_536 {
        return Err("cohort configuration exceeds limit");
    }
    let config: Config = serde_json::from_str(raw).map_err(|_| "invalid cohort configuration")?;
    if config.community_id.is_nil() {
        return Err("explicit community selection required");
    }
    match &config.action {
        Action::Capture {
            relay_capture_hook_installed,
            channel_ids,
            employee_ids,
        } => {
            if !relay_capture_hook_installed {
                return Err("capture requires the deployed atomic relay ingress hook declaration");
            }
            if channel_ids.is_empty()
                || employee_ids.is_empty()
                || channel_ids.len() > MAX_ROUTING_COHORT_SIZE
                || employee_ids.len() > MAX_ROUTING_COHORT_SIZE
                || channel_ids.iter().any(Uuid::is_nil)
                || channel_ids.iter().collect::<BTreeSet<_>>().len() != channel_ids.len()
                || employee_ids.iter().collect::<BTreeSet<_>>().len() != employee_ids.len()
            {
                return Err("capture requires unique bounded channel and employee selections");
            }
        }
        Action::Reconcile {
            capture_id,
            channel_id,
            limit,
        } => {
            if capture_id.is_nil()
                || channel_id.is_nil()
                || !(1..=MAX_INBOX_RECONCILIATION_BATCH).contains(limit)
            {
                return Err(
                    "reconcile requires exact capture and channel identities and a bounded page",
                );
            }
        }
        Action::Enable { capture_id } if capture_id.is_nil() => {
            return Err("enable requires an exact retained capture identity");
        }
        Action::Enable { .. } | Action::Status {} | Action::Disable {} => {}
    }
    Ok(config)
}

#[tokio::main]
async fn main() {
    let result = match Shutdown::install() {
        Ok(mut shutdown) => {
            shutdown
                .until(tokio::time::timeout(Duration::from_secs(30), run()))
                .await
        }
        Err(_) => {
            print_error("shutdown_registration_failed");
            std::process::exit(1);
        }
    };
    match result {
        Ok(Outcome::Completed(Ok(Ok(result)))) => println!("{result}"),
        Ok(Outcome::Completed(Ok(Err(code)))) => {
            print_error(code);
            std::process::exit(1);
        }
        Ok(Outcome::Completed(Err(_))) => {
            print_error("command_deadline_elapsed_inspect_status_and_resume_same_capture");
            std::process::exit(1);
        }
        _ => {
            print_error("interrupted_inspect_status_and_resume_same_capture");
            std::process::exit(1);
        }
    }
}

fn print_error(code: &'static str) {
    eprintln!("{}", json!({"ok":false,"error":code}));
}

async fn run() -> Result<Value, &'static str> {
    let enabled = std::env::var("ORTAK_COHORT_ENABLED").ok();
    if enabled.as_deref() != Some("true") {
        return Err("explicit ORTAK_COHORT_ENABLED=true is required");
    }
    let raw =
        std::env::var("ORTAK_COHORT_CONFIG_JSON").map_err(|_| "cohort configuration required")?;
    // Parsing and validation precede any database credential lookup or I/O.
    let config = parse_config(enabled.as_deref(), &raw)?;
    let database =
        std::env::var("ORTAK_DATABASE_URL").map_err(|_| "database selection required")?;
    let pool = ortak_server::connect_private_database(&database)
        .await
        .map_err(|_| "database connection failed")?;
    let control = PgControlPlane::new(pool);
    let scope = control
        .resolve_company_for_community(config.community_id)
        .await
        .map_err(|_| "selected community does not resolve to an active company")?;
    let result = match config.action {
        Action::Capture {
            channel_ids,
            employee_ids,
            ..
        } => {
            let cohort = control
                .begin_routing_capture(&scope, &channel_ids, &employee_ids)
                .await
                .map_err(|_| "capture refused; inspect selected resources and current cohort")?;
            json!({"ok":true,"action":"capture","cohort":cohort})
        }
        Action::Status {} => {
            let cohort = control
                .routing_cohort(&scope)
                .await
                .map_err(|_| "cohort status unavailable")?;
            json!({"ok":true,"action":"status","company_id":scope.company_id(),
                "community_id":config.community_id,"cohort":cohort})
        }
        Action::Reconcile {
            capture_id,
            channel_id,
            limit,
        } => {
            // Pin once, then perform exactly one page. A replay returns the
            // durable original window/cursor; it never extends that window.
            control
                .start_inbox_reconciliation(&scope, capture_id, channel_id)
                .await
                .map_err(|_| "reconciliation window refused; inspect current capture")?;
            let progress = control
                .reconcile_inbox_batch(&scope, capture_id, channel_id, limit)
                .await
                .map_err(|_| {
                    "reconciliation page refused; retry same capture and channel after inspection"
                })?;
            json!({"ok":true,"action":"reconcile","company_id":scope.company_id(),"progress":progress})
        }
        Action::Enable { capture_id } => {
            control
                .enable_routing_cohort(&scope, capture_id)
                .await
                .map_err(|_| {
                    "enable refused; all selected channels need completed current reconciliation"
                })?;
            json!({"ok":true,"action":"enable","company_id":scope.company_id(),"capture_id":capture_id})
        }
        Action::Disable {} => {
            control
                .disable_routing_cohort(&scope)
                .await
                .map_err(|_| "cohort disable failed")?;
            json!({"ok":true,"action":"disable","company_id":scope.company_id()})
        }
    };
    Ok(result)
}

#[cfg(test)]
#[path = "ortak_cohort/tests.rs"]
mod tests;
