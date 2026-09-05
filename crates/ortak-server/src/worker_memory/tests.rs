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
    const MAX_BYTES: u64 = 16_384;
    #[cfg(target_os = "macos")]
    const OPEN_FLAGS: i32 = 0x100 | 0x4;
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    const OPEN_FLAGS: i32 = 0x20000 | 0x800;
    #[cfg(all(target_os = "linux", target_arch = "aarch64"))]
    const OPEN_FLAGS: i32 = 0x8000 | 0x800;

    fn read_fragment(uid: u32) -> Result<Vec<u8>, &'static str> {
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
            .open(FRAGMENT)
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

    async fn check_actual_fragment() -> Result<(), &'static str> {
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
        let bytes = read_fragment(uid)?;
        let config = selected_fragment(&bytes, company)?;
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
            read_fragment(uid)? == bytes,
            "worker composition changed bootstrap fragment"
        );
        pool.close().await;
        Ok(())
    }

    #[tokio::test]
    #[ignore = "requires exact fresh private fragment, read-only selected DB credentials and selected native Honcho token; replays diagnostic I/O only"]
    async fn live_private_bootstrap_fragment_recovers_worker_memory_readiness() {
        let result = tokio::time::timeout(Duration::from_secs(25), check_actual_fragment()).await;
        assert!(
            matches!(result, Ok(Ok(()))),
            "private WorkerMemory compatibility gate failed: {result:?}"
        );
    }
}
