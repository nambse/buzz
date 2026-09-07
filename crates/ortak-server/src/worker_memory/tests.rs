//! Private fragment compatibility only; this module never starts the worker.
//!
//! Central runner supplies ORTAK_PRIVATE_MEMORY_TEST_DATABASE_URL,
//! ORTAK_PRIVATE_MEMORY_TEST_COMPANY_ID, ORTAK_PRIVATE_MEMORY_TEST_UID and the
//! single selected ORTAK_HONCHO_PRIVATE_TOKEN without logging credential values.

use super::*;
use ortak_control::{
    fakes::InMemoryProvisioningRepository, ports::CompanyDirectory, PgControlPlane,
};
use serde_json::json;

fn example_config() -> serde_json::Value {
    json!({
        "deployment_id": "12c8b01b-d017-434e-888c-9c0653a080ea",
        "origin": "http://127.0.0.1:8009",
        "endpoint_ref": "service://ortak-private-20260905/honcho",
        "token_ref": "secret://ortak-private-20260905/honcho-admin",
        "token_env": "ORTAK_HONCHO_PRIVATE_TOKEN",
        "validate_memory_io": true,
        "employees": [{
            "employee_id": "ada-private",
            "binding": {
                "adapter": "honcho",
                "endpoint_ref": "service://ortak-private-20260905/honcho",
                "workspace": "fixture-only",
                "user_peer": "operator-private",
                "employee_peer": "ada-private",
                "options": {}
            },
            "creation_key": "fixture-original-create-key",
            "validation_run_id": "dc9236e0-2624-44b3-811a-3103887f9487",
            "validation_recorded_at": "2026-09-05T08:00:00.123456Z"
        }]
    })
}

#[test]
fn employee_destinations_are_default_off_exact_and_owned_before_credentials() {
    let scope = InMemoryProvisioningRepository::new().scope();
    let old: MemoryConfig = serde_json::from_value(example_config()).unwrap();
    assert!(old.employees[0].reviewed_employee_destinations.is_empty());
    for failure in [
        "receipt",
        "duplicate_channel",
        "duplicate_target",
        "nil",
        "bound",
    ] {
        let mut value = prepared_config(scope.company_id());
        value["require_creation_receipts"] = json!(false);
        value["employees"][0]["reviewed_employee_destinations"] = json!([
            {"target_id":Uuid::from_u128(100),"destination_channel_id":Uuid::from_u128(200)}]);
        match failure {
            "receipt"=>value["employees"][0]["creation_receipt"]=serde_json::Value::Null,
            "nil"=>value["employees"][0]["reviewed_employee_destinations"][0]["target_id"]=json!(Uuid::nil()),
            "bound"=>value["employees"][0]["reviewed_employee_destinations"]=json!((0..17).map(|i|
                json!({"target_id":Uuid::from_u128(100+i),"destination_channel_id":Uuid::from_u128(200+i)})).collect::<Vec<_>>()),
            _=>value["employees"][0]["reviewed_employee_destinations"].as_array_mut().unwrap().push(
                json!({"target_id":Uuid::from_u128(if failure=="duplicate_target" {100} else {101}),
                    "destination_channel_id":Uuid::from_u128(if failure=="duplicate_channel" {200} else {201})})),
        }
        let config: MemoryConfig = serde_json::from_value(value).unwrap();
        assert!(config.validate(&scope).is_err(), "{failure}");
    }
}

fn prepared_config(company: Uuid) -> serde_json::Value {
    let mut config = example_config();
    config["origin"] = json!("http://127.0.0.1:1");
    config["token_env"] = json!("ORTAK_MEMORY_RECIPE_TEST_TOKEN");
    config["require_creation_receipts"] = json!(true);
    let entry = config["employees"][0].clone();
    config["employees"][0]["creation_receipt"] = json!({
        "company_id": company,
        "deployment_id": config["deployment_id"],
        "employee_id": entry["employee_id"],
        "binding": entry["binding"],
        "creation_key": entry["creation_key"],
        "request_hash": "0".repeat(64),
        "native_ids": {"workspace": "fixture-native", "peers": {
            "operator-private": "fixture-user-native", "ada-private": "fixture-employee-native"
        }},
        "resources": {
            "workspace": {"resource_ref": "workspace:fixture-only", "ownership": "created"},
            "user_peer": {"resource_ref": "peer:fixture-only/operator-private", "ownership": "created"},
            "employee_peer": {"resource_ref": "peer:fixture-only/ada-private", "ownership": "created"}
        }
    });
    config
}

#[test]
fn config_requires_explicit_bounded_validation_before_credential_resolution() {
    let scope = InMemoryProvisioningRepository::new().scope();
    for change in ["authorization", "empty", "unbounded"] {
        let mut value = example_config();
        match change {
            "authorization" => value["validate_memory_io"] = json!(false),
            "empty" => value["employees"] = json!([]),
            _ => value["employees"] = json!(vec![value["employees"][0].clone(); 65]),
        }
        let config: MemoryConfig = serde_json::from_value(value).unwrap();
        assert!(matches!(
            WorkerMemory::new(&scope, config),
            Err("explicit bounded memory validation configuration required")
        ));
    }
    let mut unknown = example_config();
    unknown["implicit_create"] = json!(true);
    assert!(serde_json::from_value::<MemoryConfig>(unknown).is_err());
}

#[test]
fn reviewed_projects_are_default_off_bounded_and_require_the_full_original_receipt() {
    let scope = InMemoryProvisioningRepository::new().scope();
    let legacy: MemoryConfig = serde_json::from_value(example_config()).unwrap();
    assert!(legacy.employees[0].reviewed_projects.is_empty());
    for change in ["missing_receipt", "nil", "unbounded"] {
        let mut value = prepared_config(scope.company_id());
        // Bind the project-specific full-receipt guard independently of the
        // optional whole-recipe strict mode used by the activation runner.
        value["require_creation_receipts"] = json!(false);
        value["token_env"] = json!("invalid token variable");
        value["employees"][0]["reviewed_projects"] = json!([Uuid::new_v4()]);
        match change {
            "missing_receipt" => {
                value["employees"][0]["creation_receipt"] = serde_json::Value::Null
            }
            "nil" => value["employees"][0]["reviewed_projects"] = json!([Uuid::nil()]),
            _ => {
                value["employees"][0]["reviewed_projects"] =
                    json!((0..17).map(|_| Uuid::new_v4()).collect::<Vec<_>>())
            }
        }
        let config: MemoryConfig = serde_json::from_value(value).unwrap();
        assert!(matches!(
            WorkerMemory::new(&scope, config),
            Err("memory recipe identity, original receipt or diagnostic differs")
        ));
    }
}

#[test]
fn reviewed_runtime_requires_separate_subset_opt_in_before_credentials() {
    let scope = InMemoryProvisioningRepository::new().scope();
    let legacy: MemoryConfig = serde_json::from_value(example_config()).unwrap();
    assert!(legacy.employees[0].reviewed_runtime_projects.is_empty());
    let mut value = prepared_config(scope.company_id());
    value["token_env"] = json!("invalid token variable");
    value["employees"][0]["reviewed_projects"] = json!([Uuid::new_v4()]);
    value["employees"][0]["reviewed_runtime_projects"] = json!([Uuid::new_v4()]);
    assert!(matches!(
        WorkerMemory::new(&scope, serde_json::from_value(value).unwrap()),
        Err("memory recipe identity, original receipt or diagnostic differs")
    ));
}

#[test]
fn conversation_selection_is_explicit_unambiguous_and_checked_before_credentials() {
    let scope = InMemoryProvisioningRepository::new().scope();
    let legacy: MemoryConfig = serde_json::from_value(example_config()).unwrap();
    assert!(legacy.employees[0].reviewed_conversations.is_empty());
    let project = Uuid::new_v4();
    let channel = Uuid::new_v4();
    for fault in [
        "nil_project",
        "nil_channel",
        "unselected_project",
        "duplicate",
        "ambiguous_channel",
        "rebound_project",
        "unbounded",
    ] {
        let mut value = prepared_config(scope.company_id());
        value["token_env"] = json!("invalid token variable");
        value["employees"][0]["reviewed_projects"] = json!([project]);
        let mut selected = vec![json!({"project_id": project, "channel_id": channel})];
        match fault {
            "nil_project" => selected[0]["project_id"] = json!(Uuid::nil()),
            "nil_channel" => selected[0]["channel_id"] = json!(Uuid::nil()),
            "unselected_project" => selected[0]["project_id"] = json!(Uuid::new_v4()),
            "duplicate" => selected.push(selected[0].clone()),
            "ambiguous_channel" => {
                let other = Uuid::new_v4();
                value["employees"][0]["reviewed_projects"] = json!([project, other]);
                selected.push(json!({"project_id":other,"channel_id":channel}));
            }
            "rebound_project" => {
                selected.push(json!({"project_id":project,"channel_id":Uuid::new_v4()}))
            }
            _ => selected = vec![selected[0].clone(); 17],
        }
        value["employees"][0]["reviewed_conversations"] = json!(selected);
        assert!(
            matches!(
                WorkerMemory::new(&scope, serde_json::from_value(value).unwrap()),
                Err("memory recipe identity, original receipt or diagnostic differs")
            ),
            "{fault}"
        );
    }
    let mut valid = prepared_config(scope.company_id());
    valid["employees"][0]["reviewed_projects"] = json!([project]);
    valid["employees"][0]["reviewed_conversations"] =
        json!([{"project_id":project,"channel_id":channel}]);
    let parsed: MemoryConfig = serde_json::from_value(valid.clone()).unwrap();
    assert!(parsed.validate(&scope).is_ok());
    assert!(
        parsed.employees[0].reviewed_runtime_projects.is_empty(),
        "conversation opt-in must not opt into project Work memory"
    );
    valid["employees"][0]["reviewed_conversations"][0]["thread_root"] = json!("caller-controlled");
    assert!(serde_json::from_value::<MemoryConfig>(valid).is_err());
}

#[test]
fn shared_receipt_selection_mismatch_is_rejected_before_credential_resolution() {
    let scope = InMemoryProvisioningRepository::new().scope();
    for change in [
        "missing",
        "company",
        "deployment",
        "employee",
        "binding",
        "key",
        "diagnostic",
        "endpoint",
    ] {
        let mut config = prepared_config(scope.company_id());
        // If selection checking is removed or moved after credential lookup,
        // this invalid variable name produces a different error.
        config["token_env"] = json!("invalid token variable");
        let receipt = &mut config["employees"][0]["creation_receipt"];
        match change {
            "missing" => *receipt = serde_json::Value::Null,
            "company" => receipt["company_id"] = json!(Uuid::nil()),
            "deployment" => receipt["deployment_id"] = json!(Uuid::nil()),
            "employee" => receipt["employee_id"] = json!("other-employee"),
            "binding" => receipt["binding"]["user_peer"] = json!("other-user"),
            "key" => receipt["creation_key"] = json!("other-bootstrap-create-key"),
            "diagnostic" => config["employees"][0]["validation_run_id"] = json!(Uuid::nil()),
            "endpoint" => config["endpoint_ref"] = json!("service://other"),
            _ => unreachable!(),
        }
        assert!(
            matches!(
                WorkerMemory::new(&scope, serde_json::from_value(config).unwrap()),
                Err("memory recipe identity, original receipt or diagnostic differs")
            ),
            "{change}"
        );
    }
}

#[test]
fn duplicate_worker_bindings_and_original_keys_are_rejected_before_credentials() {
    let scope = InMemoryProvisioningRepository::new().scope();
    for duplicate in ["employee", "workspace", "key"] {
        let mut config = example_config();
        config["token_env"] = json!("invalid token variable");
        let mut second = config["employees"][0].clone();
        second["employee_id"] = json!("second-employee");
        second["binding"]["workspace"] = json!("second-workspace");
        second["creation_key"] = json!("second-original-create-key");
        match duplicate {
            "employee" => second["employee_id"] = config["employees"][0]["employee_id"].clone(),
            "workspace" => {
                second["binding"]["workspace"] =
                    config["employees"][0]["binding"]["workspace"].clone()
            }
            "key" => second["creation_key"] = config["employees"][0]["creation_key"].clone(),
            _ => unreachable!(),
        }
        config["employees"].as_array_mut().unwrap().push(second);
        assert!(
            matches!(
                WorkerMemory::new(&scope, serde_json::from_value(config).unwrap()),
                Err("memory recipe identity, original receipt or diagnostic differs")
            ),
            "{duplicate}"
        );
    }
}

#[test]
fn prepared_and_legacy_worker_construction_preserve_distinct_acquisition_modes() {
    const CHILD: &str = "ORTAK_MEMORY_RECIPE_TEST_CHILD";
    if std::env::var_os(CHILD).is_none() {
        let mut child = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "worker_memory::tests::prepared_and_legacy_worker_construction_preserve_distinct_acquisition_modes"])
            .env_clear()
            .env(CHILD, "1")
            .env("ORTAK_MEMORY_RECIPE_TEST_TOKEN", "explicit-synthetic-test-token")
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        let status = loop {
            if let Some(status) = child.try_wait().unwrap() {
                break status;
            }
            if Instant::now() >= deadline {
                child.kill().unwrap();
                child.wait().unwrap();
                panic!("child constructor check deadline exceeded");
            }
            std::thread::sleep(Duration::from_millis(10));
        };
        let mut output = String::new();
        std::io::Read::read_to_string(
            &mut std::io::Read::take(child.stdout.take().unwrap(), 4096),
            &mut output,
        )
        .unwrap();
        assert!(
            status.success() && output.contains("1 passed"),
            "bounded child constructor check failed"
        );
        return;
    }
    let scope = InMemoryProvisioningRepository::new().scope();
    let prepared = prepared_config(scope.company_id());
    let mut legacy = prepared.clone();
    legacy["require_creation_receipts"] = json!(false);
    legacy["employees"][0]
        .as_object_mut()
        .unwrap()
        .remove("creation_receipt");
    for (value, expected) in [
        (prepared, ProvisioningMode::Adopt),
        (legacy, ProvisioningMode::Create),
    ] {
        let config: MemoryConfig = serde_json::from_value(value).unwrap();
        let original = config.employees[0].creation_receipt.clone();
        let run = config.employees[0].validation_run_id;
        let recorded_at = config.employees[0].validation_recorded_at;
        let memory = WorkerMemory::new(&scope, config).unwrap();
        assert!(
            !memory.ready(),
            "construction or a saved receipt cannot grant a witness"
        );
        let values = memory.validations.lock().unwrap();
        assert_eq!(values.len(), 1);
        assert_eq!(values[0].resource.mode, expected);
        assert_eq!(values[0].creation_receipt, original);
        assert_eq!(values[0].roundtrip.run_id, run);
        assert_eq!(values[0].roundtrip.recorded_at, recorded_at);
    }
}

#[tokio::test]
async fn disabled_memory_cannot_become_ready_or_mutate_resources() {
    let memory = WorkerMemory::disabled();
    assert!(!memory.ready());
    assert_eq!(memory.refresh_one().await, None);
    let config: MemoryConfig = serde_json::from_value(example_config()).unwrap();
    assert_mutation_unsupported(&memory, &config.employees[0]).await;
    assert!(!memory.ready());
}

async fn assert_mutation_unsupported(memory: &WorkerMemory, entry: &EmployeeConfig) {
    // These are the production trait methods, including after successful
    // validation. A healthy worker is never a provisioning/deletion authority.
    let request = MemoryResourceRequest {
        employee_id: entry.employee_id.clone(),
        binding: entry.binding.clone(),
        mode: ProvisioningMode::Create,
        idempotency_key: entry.creation_key.clone(),
    };
    assert!(matches!(
        memory.ensure_resources(&request).await,
        Err(MemoryError::Unsupported {
            capability: MemoryCapability::ResourceCreate
        })
    ));
    assert!(matches!(
        memory
            .delete_created_resource(&entry.binding.workspace, "read-only-worker-test")
            .await,
        Err(MemoryError::Unsupported {
            capability: MemoryCapability::ResourceDelete
        })
    ));
}

// No extra dependency or unsafe syscall is needed for the selected macOS/Linux
// test hosts. Flags are the OS ABI O_NOFOLLOW | O_NONBLOCK (Darwin sys/fcntl.h;
// libc's Linux x86_64/aarch64 definitions). Unsupported hosts omit this fixture.
#[cfg(any(
    target_os = "macos",
    all(
        target_os = "linux",
        any(target_arch = "x86_64", target_arch = "aarch64")
    )
))]
mod private_live {
    use super::*;
    use std::{
        fs::{self, OpenOptions},
        io::Read,
        os::unix::fs::{MetadataExt, OpenOptionsExt},
        path::Path,
    };

    const ROOT: &str = "/private/tmp/ortak-private-20260905";
    const DIRECTORY: &str = "/private/tmp/ortak-private-20260905/memory";
    const FRAGMENT: &str = "/private/tmp/ortak-private-20260905/memory/worker-memory.json";
    const PREPARED_FRAGMENT: &str =
        "/private/tmp/ortak-private-20260905/memory/worker-memory-prepared.json";
    const MAX_BYTES: u64 = 16_384;
    #[cfg(target_os = "macos")]
    const OPEN_FLAGS: i32 = 0x100 | 0x4;
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    const OPEN_FLAGS: i32 = 0x20000 | 0x800;
    #[cfg(all(target_os = "linux", target_arch = "aarch64"))]
    const OPEN_FLAGS: i32 = 0x8000 | 0x800;

    fn read_fragment(uid: u32, prepared: bool) -> Result<Vec<u8>, &'static str> {
        for directory in [ROOT, DIRECTORY] {
            let path = Path::new(directory);
            let metadata =
                fs::symlink_metadata(path).map_err(|_| "private directory unavailable")?;
            if !metadata.is_dir() || metadata.uid() != uid || metadata.mode() & 0o077 != 0 {
                return Err("private directory ownership or mode differs");
            }
            if fs::canonicalize(path).map_err(|_| "private directory unavailable")? != path {
                return Err("private directory contains symlink");
            }
        }
        let file = OpenOptions::new()
            .read(true)
            .custom_flags(OPEN_FLAGS)
            .open(if prepared {
                PREPARED_FRAGMENT
            } else {
                FRAGMENT
            })
            .map_err(|_| "private memory fragment unavailable")?;
        let metadata = file
            .metadata()
            .map_err(|_| "fragment metadata unavailable")?;
        if !metadata.is_file()
            || metadata.uid() != uid
            || metadata.mode() & 0o077 != 0
            || metadata.len() == 0
            || metadata.len() > MAX_BYTES
        {
            return Err("private fragment ownership, mode or size differs");
        }
        let mut bytes = Vec::new();
        file.take(MAX_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| "private fragment read failed")?;
        if bytes.len() as u64 > MAX_BYTES {
            return Err("private fragment exceeds bound");
        }
        Ok(bytes)
    }

    fn selected_fragment(bytes: &[u8], company: Uuid) -> Result<MemoryConfig, &'static str> {
        let config: MemoryConfig =
            serde_json::from_slice(bytes).map_err(|_| "invalid private fragment")?;
        if config.deployment_id.is_nil()
            || config.origin != "http://127.0.0.1:8009"
            || config.endpoint_ref != "service://ortak-private-20260905/honcho"
            || config.token_ref
                != CredentialRef::parse("secret://ortak-private-20260905/honcho-admin")
                    .map_err(|_| "invalid fixture credential reference")?
            || config.token_env != "ORTAK_HONCHO_PRIVATE_TOKEN"
            || !config.validate_memory_io
            || config.employees.len() != 1
        {
            return Err("fragment does not select this private deployment");
        }
        let employee = &config.employees[0];
        if employee.employee_id.as_str() != "ada-private"
            || employee.binding.adapter != "honcho"
            || employee.binding.endpoint_ref != config.endpoint_ref
            || employee.binding.workspace != format!("ortak_ada_{}", company.simple())
            || employee.binding.user_peer != "operator-private"
            || employee.binding.employee_peer != "ada-private"
            || !employee.binding.options.is_empty()
            || employee.creation_key
                != format!(
                    "ortak-memory:{company}:ada-private:{}",
                    config.deployment_id
                )
            || employee.validation_run_id.is_nil()
        {
            return Err("fragment identity or original bootstrap key differs");
        }
        Ok(config)
    }

    async fn check_actual_fragment(prepared: bool) -> Result<(), &'static str> {
        let uid: u32 = std::env::var("ORTAK_PRIVATE_MEMORY_TEST_UID")
            .map_err(|_| "explicit expected private owner UID required")?
            .parse()
            .map_err(|_| "invalid expected private owner UID")?;
        let company_text = std::env::var("ORTAK_PRIVATE_MEMORY_TEST_COMPANY_ID")
            .map_err(|_| "explicit expected private company required")?;
        let company: Uuid = company_text
            .parse()
            .map_err(|_| "invalid expected private company")?;
        if company.is_nil() || company.to_string() != company_text {
            return Err("canonical non-nil private company required");
        }
        let bytes = read_fragment(uid, prepared)?;
        let config = selected_fragment(&bytes, company)?;
        if prepared
            && (!config.require_creation_receipts || config.employees[0].creation_receipt.is_none())
        {
            return Err("prepared fragment requires the original complete creation receipt");
        }
        let database = std::env::var("ORTAK_PRIVATE_MEMORY_TEST_DATABASE_URL")
            .map_err(|_| "explicit private test database required")?;
        let parsed = url::Url::parse(&database).map_err(|_| "invalid private test database")?;
        if parsed.scheme() != "postgres"
            || parsed.host_str() != Some("127.0.0.1")
            || parsed.port() != Some(55433)
            || parsed.path() != "/ortak"
            || parsed.username() != "ortak"
            || parsed.password().is_none_or(str::is_empty)
            || parsed.query().is_some()
            || parsed.fragment().is_some()
        {
            return Err("database does not select the fresh private stack");
        }
        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(1)
            .acquire_timeout(Duration::from_secs(3))
            .after_connect(|connection, _| {
                Box::pin(async move {
                    sqlx::query("SET default_transaction_read_only = on")
                        .execute(connection)
                        .await?;
                    Ok(())
                })
            })
            .connect(&database)
            .await
            .map_err(|_| "private read-only database connection failed")?;
        let scope = PgControlPlane::new(pool.clone())
            .resolve_company_by_slug("ortak-private-20260905")
            .await
            .map_err(|_| "private company scope lookup failed")?;
        if scope.company_id() != company {
            return Err("resolved company does not match expected private fixture");
        }
        // No migration, lifecycle mutation, routing, worker main or model call.
        let memory = WorkerMemory::new(&scope, config)?;
        assert!(
            !memory.ready(),
            "saved fragment must not grant a ready witness"
        );
        assert_eq!(
            memory.refresh_one().await,
            Some(true),
            "actual diagnostic validation must succeed"
        );
        assert!(memory.ready());
        let original = selected_fragment(&bytes, company)?;
        assert_mutation_unsupported(&memory, &original.employees[0]).await;
        assert_eq!(
            memory.refresh_one().await,
            None,
            "successful refresh must honor its interval"
        );
        assert!(memory.ready());

        // A second composition must recover with the same original create key
        // and diagnostic run/time. Its in-memory witness starts empty again.
        let restarted = WorkerMemory::new(&scope, selected_fragment(&bytes, company)?)?;
        assert!(!restarted.ready());
        assert_eq!(restarted.refresh_one().await, Some(true));
        assert!(restarted.ready());
        assert_mutation_unsupported(&restarted, &original.employees[0]).await;
        assert!(
            read_fragment(uid, prepared)? == bytes,
            "worker composition changed bootstrap fragment"
        );
        pool.close().await;
        Ok(())
    }

    #[tokio::test]
    #[ignore = "requires exact fresh private fragment, read-only selected DB credentials and selected native Honcho token; replays diagnostic I/O only"]
    async fn live_private_bootstrap_fragment_recovers_worker_memory_readiness() {
        let result =
            tokio::time::timeout(Duration::from_secs(25), check_actual_fragment(false)).await;
        assert!(
            matches!(result, Ok(Ok(()))),
            "private WorkerMemory compatibility gate failed: {result:?}"
        );
    }

    #[tokio::test]
    #[ignore = "requires exported prepared private fragment, read-only selected DB credentials and selected native Honcho token; replays original diagnostic I/O only"]
    async fn live_private_prepared_fragment_recovers_adopted_worker_memory_readiness() {
        let result =
            tokio::time::timeout(Duration::from_secs(25), check_actual_fragment(true)).await;
        assert!(
            matches!(result, Ok(Ok(()))),
            "prepared WorkerMemory compatibility gate failed: {result:?}"
        );
    }
}
