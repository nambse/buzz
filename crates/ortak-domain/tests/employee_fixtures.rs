use ortak_domain::{
    DomainError, EmployeeCatalog, EmployeeManifest, EmployeeStatus, ProvisioningMode,
};

const CEM_RAW: &str = include_str!("../../../config/employees/cem.yaml");
const ZEYNEP_RAW: &str = include_str!("../../../config/employees/zeynep.yaml");

fn manifests() -> [EmployeeManifest; 2] {
    [
        serde_yaml::from_str(CEM_RAW).expect("Cem fixture must deserialize"),
        serde_yaml::from_str(ZEYNEP_RAW).expect("Zeynep fixture must deserialize"),
    ]
}

#[test]
fn adopted_employee_fixtures_are_valid_and_catalog_compatible() {
    let manifests = manifests();
    for manifest in &manifests {
        assert_eq!(manifest.provisioning, ProvisioningMode::Adopt);
        assert_eq!(manifest.employee.status, EmployeeStatus::Draft);
        manifest.validate().expect("fixture must validate");
    }

    let catalog = EmployeeCatalog::new(manifests.map(|manifest| manifest.employee))
        .expect("fixture aliases and identifiers must be company-unique");
    assert_eq!(catalog.employees().count(), 2);
}

#[test]
fn fixtures_contain_references_but_no_private_material() {
    let raw = [CEM_RAW, ZEYNEP_RAW].join("\n").to_lowercase();
    let typed = serde_json::to_string(&manifests())
        .expect("employee fixtures must serialize for inspection")
        .to_lowercase();

    for forbidden in [
        "private_key",
        "secret_key",
        "auth.json",
        "nsec1",
        "bearer ",
        "api_key",
        "access_token",
        "refresh_token",
    ] {
        assert!(
            !raw.contains(forbidden),
            "raw fixture unexpectedly contains forbidden marker: {forbidden}"
        );
        assert!(!typed.contains(forbidden));
    }

    assert!(raw.contains("credential://"));
    assert!(raw.contains("/opt/data/profiles/cem"));
    assert!(raw.contains("/opt/data/profiles/zeynep"));
}

#[test]
fn unknown_manifest_fields_fail_closed() {
    let with_secret = format!("{CEM_RAW}\n  private_key: should-never-be-accepted\n");
    let result = serde_yaml::from_str::<EmployeeManifest>(&with_secret);
    assert!(result.is_err());
}

#[test]
fn unsupported_schema_and_invalid_catalog_entries_fail_closed() {
    let mut unsupported = manifests()
        .into_iter()
        .next()
        .expect("fixture array must contain Cem");
    unsupported.schema_version = "ortak.employee/v99".to_owned();
    assert!(matches!(
        unsupported.validate(),
        Err(DomainError::UnsupportedManifestSchema(_))
    ));

    let mut invalid_employee = unsupported.employee;
    invalid_employee.routing.semantic_min_score = Some(1.5);
    assert!(EmployeeCatalog::new([invalid_employee]).is_err());
}

#[test]
fn permission_typos_and_secret_like_options_fail_closed_without_echoing_values() {
    let permission_typo =
        CEM_RAW.replace("destructive_file_operation", "destructve_file_operation");
    assert!(serde_yaml::from_str::<EmployeeManifest>(&permission_typo).is_err());

    let secret_option = CEM_RAW.replace(
        "reasoning_effort: medium",
        "api_key: should-never-appear-in-an-error",
    );
    let manifest: EmployeeManifest =
        serde_yaml::from_str(&secret_option).expect("option shape should deserialize");
    let error = manifest
        .validate()
        .expect_err("secret-like adapter options must fail validation")
        .to_string();
    assert!(!error.contains("should-never-appear-in-an-error"));

    let literal_credential = CEM_RAW.replace(
        "credential://ortak-runtime/cem/codex-oauth",
        "sk-live-should-never-appear-in-an-error",
    );
    let error = serde_yaml::from_str::<EmployeeManifest>(&literal_credential)
        .expect_err("literal credentials must fail deserialization")
        .to_string();
    assert!(!error.contains("sk-live-should-never-appear-in-an-error"));
}

#[test]
fn semantic_candidate_metadata_is_bounded_before_catalog_entry() {
    let mut oversized = manifests()[0].clone();
    oversized.employee.biography = "x".repeat(4_097);
    assert!(oversized.validate().is_err());

    let mut invalid_domain = manifests()[0].clone();
    invalid_domain.employee.domains = vec!["free form model prose".to_owned()];
    assert!(invalid_domain.validate().is_err());

    let mut duplicate_capability = manifests()[0].clone();
    duplicate_capability
        .employee
        .permissions
        .allowed_tools
        .push(duplicate_capability.employee.permissions.allowed_tools[0]);
    assert!(duplicate_capability.validate().is_err());
}
