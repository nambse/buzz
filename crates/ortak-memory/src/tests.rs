use super::*;
use chrono::Utc;
use ortak_control::memory::{MemoryBudget, MemoryFact, MemoryProvenance, MemoryScope};
use std::collections::BTreeMap;
use zeroize::Zeroizing;

fn fixture(origin: &str, mode: ProvisioningMode) -> (Uuid, HonchoMemoryConfig) {
    let company = Uuid::from_u128(1);
    let binding = MemoryBinding {
        adapter: "honcho".into(),
        endpoint_ref: "service://honcho-private".into(),
        workspace: "private_employee_one".into(),
        user_peer: "operator".into(),
        employee_peer: "employee".into(),
        options: BTreeMap::new(),
    };
    (
        company,
        HonchoMemoryConfig {
            deployment: HonchoDeploymentSelection {
                deployment_id: Uuid::from_u128(2),
                protocol: PROTOCOL.into(),
                honcho_version: HONCHO_VERSION.into(),
                endpoint_ref: binding.endpoint_ref.clone(),
                origin: origin.into(),
                token_ref: ortak_domain::CredentialRef::parse("secret://memory/private").unwrap(),
            },
            employees: vec![HonchoEmployeeBinding {
                employee_id: EmployeeId::parse("employee-one").unwrap(),
                binding,
                mode,
                allow_company_truth: false,
                allowed_projects: BTreeSet::from([Uuid::from_u128(4)]),
            }],
            request_timeout: Duration::from_secs(3),
            witness_lifetime: Duration::from_secs(60),
        },
    )
}
fn adapter(company: Uuid, config: HonchoMemoryConfig) -> HonchoMemoryAdapter {
    let token = ResolvedHonchoToken::new(
        config.deployment.token_ref.clone(),
        Zeroizing::new("fresh-test-token".into()),
    );
    HonchoMemoryAdapter::for_company(company, config, token).unwrap()
}
fn gate(config: &HonchoMemoryConfig) -> MemoryRoundtripRequest {
    let allowed = &config.employees[0];
    MemoryRoundtripRequest {
        employee_id: allowed.employee_id.clone(),
        binding: allowed.binding.clone(),
        run_id: Uuid::from_u128(8),
        recorded_at: "2026-09-05T00:00:00.123456789Z".parse().unwrap(),
    }
}

#[test]
fn deployment_binding_and_secret_resolution_are_fail_closed() {
    for origin in [
        "http://honcho.example",
        "https://user:secret@example.com",
        "https://example.com/api",
        "http://localhost:0",
        "https://example.com/?token=a",
    ] {
        let (_, config) = fixture(origin, ProvisioningMode::Create);
        assert!(config::validate(&config).is_err());
    }
    let (company, mut config) = fixture("http://127.0.0.1:30001", ProvisioningMode::Create);
    let bad = ResolvedHonchoToken::new(
        ortak_domain::CredentialRef::parse("secret://another/token").unwrap(),
        Zeroizing::new("should-not-appear".into()),
    );
    let error = HonchoMemoryAdapter::for_company(company, config.clone(), bad)
        .err()
        .unwrap();
    assert!(!format!("{error:?}").contains("should-not-appear"));
    config.employees.push(config.employees[0].clone());
    assert!(config::validate(&config).is_err());
}

#[test]
fn deterministic_sessions_and_pydantic_hash_inputs_preserve_scope() {
    let (company, config) = fixture("http://localhost:30001", ProvisioningMode::Create);
    let allowed = &config.employees[0];
    let mut names = BTreeSet::new();
    for scope in [
        MemoryScope::EmployeeExperience,
        MemoryScope::Relationship,
        MemoryScope::RunScratch {
            run_id: Uuid::from_u128(9),
        },
        MemoryScope::ProjectContext {
            project_id: Uuid::from_u128(4),
        },
    ] {
        let a = wire::session(company, allowed, &scope).unwrap();
        assert_eq!(a, wire::session(company, allowed, &scope).unwrap());
        assert!(names.insert(a.clone()));
        assert_ne!(
            a,
            wire::session(Uuid::from_u128(3), allowed, &scope).unwrap()
        );
    }
    assert_eq!(
        wire::fingerprint(&json!({"b":"é","a":[{"z":1,"a":"line\ntext"}]})).unwrap(),
        "b333f2795e7d210b99069335ac02b4f17d83f67c9c431d3665cf23a87f934d73"
    );
    let value = MemoryProvenance {
        employee_id: allowed.employee_id.clone(),
        run_id: None,
        source: "office_message".into(),
        recorded_at: "2026-09-05T00:00:00.123456789Z".parse().unwrap(),
    };
    assert_eq!(
        wire::provenance(&value).unwrap(),
        json!({"employee_id":"employee-one","source":"office_message","recorded_at":"2026-09-05T00:00:00.123456Z"})
    );
    assert!(wire::check_scope(allowed, &MemoryScope::CompanyTruth).is_err());
    assert!(wire::check_scope(
        allowed,
        &MemoryScope::ProjectContext {
            project_id: Uuid::from_u128(99)
        }
    )
    .is_err());
}

#[tokio::test]
async fn unvalidated_binding_and_deletion_never_make_http_requests() {
    let (company, config) = fixture("http://127.0.0.1:1", ProvisioningMode::Create);
    let request = gate(&config);
    let service = adapter(company, config);
    let recall = MemoryRecallRequest {
        employee_id: request.employee_id,
        binding: request.binding,
        scope: MemoryScope::EmployeeExperience,
        query: "x".into(),
        budget: MemoryBudget::default(),
    };
    assert!(matches!(
        service.recall(&recall).await,
        Err(MemoryError::Unsupported {
            capability: MemoryCapability::Recall
        })
    ));
    let mut other = recall.clone();
    other.binding.endpoint_ref = "service://other".into();
    assert!(matches!(
        service.recall(&other).await,
        Err(MemoryError::InvalidRequest { .. })
    ));
    assert!(matches!(
        service.delete_created_resource("workspace:old", "x").await,
        Err(MemoryError::Unsupported {
            capability: MemoryCapability::ResourceDelete
        })
    ));
}

#[path = "tests/http_contract.rs"]
mod http_contract;

#[path = "tests/live_extension.rs"]
mod live_extension;

#[test]
fn superseded_validation_cannot_restore_or_inherit_a_witness() {
    let (company, config) = fixture("http://127.0.0.1:1", ProvisioningMode::Create);
    let allowed = config.employees[0].clone();
    let service = adapter(company, config);
    let old = service.begin_validation(&allowed).unwrap();
    let current = service.begin_validation(&allowed).unwrap();
    assert!(service.publish_validation(&allowed, old).is_err());
    assert!(service
        .check_gate(&allowed, IoGate::Validation(old))
        .is_err());
    assert!(!service.witnessed(&allowed).unwrap());
    service.publish_validation(&allowed, current).unwrap();
    let admitted = service
        .require_witness(&allowed, MemoryCapability::Remember)
        .unwrap();
    // A newer failed refresh leaves no witness and invalidates admitted old work.
    service.begin_validation(&allowed).unwrap();
    assert!(service.check_gate(&allowed, admitted).is_err());
    assert!(!service.witnessed(&allowed).unwrap());
    assert!(service.publish_validation(&allowed, current).is_err());
}
