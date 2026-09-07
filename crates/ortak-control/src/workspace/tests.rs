use super::*;

fn grant() -> WorkspaceGrant {
    let mut grant = WorkspaceGrant {
        format: WORKSPACE_FORMAT.into(),
        company_id: Uuid::from_u128(1),
        project_id: Uuid::from_u128(2),
        employee_id: EmployeeId::parse("ada").unwrap(),
        workspace_ref: "input:brief".into(),
        revision: Uuid::from_u128(3),
        manifest_hash: String::new(),
        files: vec![WorkspaceFile {
            file_id: Uuid::from_u128(4),
            name: "brief.txt".into(),
            media_type: "text/plain".into(),
            bytes: 5,
            sha256: digest(b"hello"),
        }],
    };
    grant.manifest_hash = grant.compute_hash().unwrap();
    grant
}
#[test]
fn selected_workspace_bounds_and_hash_are_enforced() {
    let original = grant();
    original.validate().unwrap();
    for name in ["/absolute", "../secret", "a//b", "a/./b", "a\\b", "a\0b"] {
        let mut bad = original.clone();
        bad.files[0].name = name.into();
        bad.manifest_hash = bad.compute_hash().unwrap();
        assert!(bad.validate().is_err(), "{name:?}");
    }
    let mut changed = original.clone();
    changed.files[0].name = "other.txt".into();
    assert!(changed.validate().is_err());
    let mut duplicate = original.clone();
    duplicate.files.push(duplicate.files[0].clone());
    duplicate.manifest_hash = duplicate.compute_hash().unwrap();
    assert!(duplicate.validate().is_err());
}
#[test]
fn tool_result_is_exact_private_selected_content() {
    let grant = grant();
    let id = grant.files[0].file_id;
    let request = WorkspaceToolRequest {
        call_id: "call_1".into(),
        file_id: id,
        arguments_hash: WorkspaceToolRequest::hash_arguments(id),
        ordinal: 1,
    };
    let mut result = WorkspaceResult::Completed {
        content: "hello".into(),
        sha256: grant.files[0].sha256.clone(),
        bytes: 5,
        name: "brief.txt".into(),
    };
    result.validate(&grant, &request).unwrap();
    assert!(!format!("{result:?}").contains("hello"));
    if let WorkspaceResult::Completed { content, .. } = &mut result {
        *content = "world".into();
    }
    assert!(result.validate(&grant, &request).is_err());
    let mut wrong = request.clone();
    wrong.arguments_hash = "0".repeat(64);
    assert!(wrong.validate(&grant).is_err());
}
#[test]
fn workspace_policy_never_inherits_network_or_approval_authority() {
    let mut policy = PermissionPolicy {
        allowed_tools: vec![ToolCapability::Files],
        allowed_workspaces: vec!["input:brief".into()],
        ..Default::default()
    };
    assert!(workspace_read_policy(&policy, "input:brief"));
    assert!(!empty_policy(&policy));
    policy.allowed_networks.push("public-web".into());
    assert!(!workspace_read_policy(&policy, "input:brief"));
    assert!(empty_policy(&PermissionPolicy::default()));
}
