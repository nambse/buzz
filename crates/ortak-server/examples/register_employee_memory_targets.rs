#![deny(unsafe_code)]
//! Explicit one-shot original-namespace registration. No provisioning, target
//! renewal, runtime advertisement, source approval or fact publication.

#[path = "register_employee_memory_targets/config.rs"]
mod config;
#[path = "register_employee_memory_targets/readback.rs"]
mod readback;

use config::{Config, Target};
use ortak_control::{ports::CompanyDirectory, CompanyScope, PgControlPlane};
use ortak_domain::ProvisioningMode;
use ortak_memory::{
    HonchoDeploymentSelection, HonchoEmployeeBinding, HonchoMemoryAdapter, HonchoMemoryConfig,
    ResolvedHonchoToken, HONCHO_VERSION, PROTOCOL,
};
use serde_json::{json, Value};
use std::{collections::BTreeSet, path::PathBuf, time::Duration};

type Result<T> = std::result::Result<T, &'static str>;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Action {
    Plan,
    Register,
    Readback,
    Recover,
}

fn arguments() -> Result<(PathBuf, Action)> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if !(args.len() == 2 || args.len() == 4) || args[0] != "--config" {
        return Err("usage_config_then_optional_action");
    }
    let action = if args.len() == 2 {
        Action::Plan
    } else {
        if args[2] != "--action" {
            return Err("usage_config_then_optional_action");
        }
        match args[3].to_str() {
            Some("plan") => Action::Plan,
            Some("register") => Action::Register,
            Some("readback") => Action::Readback,
            Some("recover") => Action::Recover,
            _ => return Err("action_invalid"),
        }
    };
    Ok((PathBuf::from(&args[1]), action))
}

fn emit(value: Value) -> Result<()> {
    let value = serde_json::to_string(&value).map_err(|_| "output_encoding")?;
    if value.len() > 4096 {
        return Err("output_bound");
    }
    println!("{value}");
    Ok(())
}

fn adapter(config: &Config, target: &Target, scope: &CompanyScope) -> Result<HonchoMemoryAdapter> {
    let d = &config.deployment;
    let token = ResolvedHonchoToken::from_env(d.token_ref.clone(), &d.token_env)
        .map_err(|_| "selected_memory_credential")?;
    HonchoMemoryAdapter::new(
        scope,
        HonchoMemoryConfig {
            deployment: HonchoDeploymentSelection {
                deployment_id: d.deployment_id,
                protocol: PROTOCOL.into(),
                honcho_version: HONCHO_VERSION.into(),
                endpoint_ref: d.endpoint_ref.clone(),
                origin: d.origin.clone(),
                token_ref: d.token_ref.clone(),
            },
            employees: vec![HonchoEmployeeBinding {
                employee_id: target.original.employee_id.clone(),
                binding: target.original.binding.clone(),
                mode: ProvisioningMode::Adopt,
                allow_company_truth: false,
                allowed_projects: BTreeSet::new(),
            }],
            request_timeout: Duration::from_secs(3),
            witness_lifetime: Duration::from_secs(60),
        },
        token,
    )
    .map_err(|_| "adapter_selection")
}

async fn one(
    control: &PgControlPlane,
    scope: &CompanyScope,
    config: &Config,
    target: &Target,
    action: Action,
) -> Result<Value> {
    if let Some(retained) = readback::existing(control.pool(), config, target).await? {
        return Ok(retained);
    }
    if action == Action::Readback {
        return Ok(
            json!({"status":"not_registered","employee_id":target.original.employee_id,
            "operation_id":target.diagnostic.operation_id}),
        );
    }
    if action == Action::Register {
        let now = chrono::Utc::now();
        if target.valid_until <= now || target.valid_until > now + chrono::Duration::days(90) {
            return Err("fixed_expiry_unavailable");
        }
        readback::current_employee(control.pool(), config, target).await?;
    }
    let adapter = adapter(config, target, scope)?;
    let namespace = adapter
        .inspect_reviewed_employee_namespace(&target.original)
        .await
        .map_err(|_| "original_namespace_inspection")?;
    if action == Action::Recover {
        let receipt = adapter
            .recover_employee_namespace_diagnostic(&namespace, &target.diagnostic)
            .await
            .map_err(|_| "diagnostic_cleanup_unconfirmed")?;
        return Ok(
            json!({"status":"diagnostic_cleaned_no_target","employee_id":target.original.employee_id,
            "operation_id":receipt.operation_id,"erased":receipt.erased,
            "cleanup_receipt":receipt,"target_registered":false}),
        );
    }
    let witness = adapter
        .validate_reviewed_employee_namespace(&namespace, &target.diagnostic)
        .await
        .map_err(|_| "diagnostic_unresolved_use_same_config_recover")?;
    // Identical in-process witness/expiry, including validated_at, survives a
    // lost COMMIT response. Never repeat the remote diagnostic for this retry.
    for attempt in 0..2 {
        let result = ortak_server::employee_memory_exports::register_target(
            control,
            scope,
            &adapter,
            &witness,
            target.destination_channel_id,
            target.valid_until,
        )
        .await;
        if result.is_ok() {
            return readback::existing(control.pool(), config, target)
                .await?
                .ok_or("target_readback_missing");
        }
        if attempt == 0 {
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    }
    Err("registration_uncertain_use_same_config_readback")
}

async fn run() -> Result<()> {
    let (path, action) = arguments()?;
    let (config, fingerprint) = config::read(&path)?;
    emit(
        json!({"format":"ortak-employee-target-operator-receipt/1","config_sha256":fingerprint,
        "company_id":config.company_id,"community_id":config.community_id,"target_count":config.targets.len(),
        "targets":config.targets.iter().map(|t|json!({"employee_id":t.original.employee_id,
            "destination_channel_id":t.destination_channel_id,"operation_id":t.diagnostic.operation_id,
            "employee_revision_id":t.diagnostic.employee_revision_id,
            "employee_lifecycle_epoch":t.diagnostic.employee_lifecycle_epoch,"valid_until":t.valid_until})).collect::<Vec<_>>(),
        "action":match action {Action::Plan=>"plan",Action::Register=>"register",Action::Readback=>"readback",Action::Recover=>"recover"}}),
    )?;
    if action == Action::Plan {
        return Ok(());
    }
    let database = config.database()?;
    let pool = ortak_server::connect_private_database(&database)
        .await
        .map_err(|_| "database_connection")?;
    drop(database);
    let control = PgControlPlane::new(pool);
    let scope = control
        .resolve_company_for_community(config.community_id)
        .await
        .map_err(|_| "company_selection")?;
    if scope.company_id() != config.company_id {
        return Err("company_selection");
    }
    for target in &config.targets {
        match one(&control, &scope, &config, target, action).await {
            Ok(value) => emit(value)?,
            Err(code) => {
                emit(
                    json!({"status":"unresolved","employee_id":target.original.employee_id,
                    "operation_id":target.diagnostic.operation_id,"code":code}),
                )?;
                return Err(code);
            }
        }
    }
    emit(json!({"status":"complete","action_complete":true}))
}

#[tokio::main]
async fn main() {
    let result = match ortak_server::shutdown::Shutdown::install() {
        Ok(mut shutdown) => {
            shutdown
                .until(tokio::time::timeout(Duration::from_secs(300), run()))
                .await
        }
        Err(_) => {
            eprintln!("employee-target-operator: shutdown_registration");
            std::process::exit(1);
        }
    };
    if let Ok(ortak_server::shutdown::Outcome::Completed(Ok(Ok(())))) = result {
        return;
    }
    // Any interrupted attempt keeps its caller-frozen diagnostic intent. Errors
    // intentionally omit database/HTTP/config objects and all credential values.
    eprintln!("employee-target-operator: unresolved; retain config and inspect readback or recover its diagnostic");
    std::process::exit(1);
}
