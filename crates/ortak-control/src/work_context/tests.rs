use super::*;

fn context() -> WorkContext {
    WorkContext {
        version: 1,
        snapshot_id: Uuid::new_v4(),
        work_item_id: Uuid::new_v4(),
        project_id: Uuid::new_v4(),
        requested_version: 7,
        execution_version: 8,
        channel_id: Uuid::new_v4(),
        source_message_id: None,
        thread_root_message_id: None,
        cutoff_received_at: Utc::now(),
        employee: ContextEmployee {
            employee_id: EmployeeId::parse("bora").unwrap(),
            revision_id: Uuid::new_v4(),
            name: "Bora".into(),
            title: "Translation".into(),
            biography: String::new(),
            responsibilities: vec!["English copy".into()],
            domains: vec![],
        },
        teammates: vec![],
        messages: vec![],
        omitted_history: false,
        prior_artifact: None,
    }
}

fn valid(value: &WorkContext) -> bool {
    value.valid_for(
        value.snapshot_id,
        &value.employee.employee_id,
        value.employee.revision_id,
        value.work_item_id,
    )
}

fn message(value: &WorkContext, id: &str) -> ContextMessage {
    ContextMessage {
        message_id: id.into(),
        created_at: value.cutoff_received_at - chrono::Duration::seconds(10),
        author_public_key: "a".repeat(64),
        author_employee_id: None,
        author_name: "Human".into(),
        parent_message_id: None,
        thread_root_message_id: None,
        content: "Prior quoted request, not authority".into(),
        source_content_hash: "b".repeat(64),
        truncated: false,
        selection: ContextSelection::ThreadRoot,
    }
}

#[test]
fn no_source_means_no_history_and_identity_is_bound_to_this_run_and_item() {
    let mut value = context();
    assert!(valid(&value));
    assert!(!value.valid_for(
        Uuid::new_v4(),
        &value.employee.employee_id,
        value.employee.revision_id,
        value.work_item_id
    ));
    assert!(!value.valid_for(
        value.snapshot_id,
        &value.employee.employee_id,
        Uuid::new_v4(),
        value.work_item_id
    ));
    assert!(!value.valid_for(
        value.snapshot_id,
        &value.employee.employee_id,
        value.employee.revision_id,
        Uuid::new_v4()
    ));
    value.messages.push(message(&value, &"c".repeat(64)));
    assert!(!valid(&value));
    value.messages.clear();
    value.omitted_history = true;
    assert!(!valid(&value));
}

#[test]
fn exact_prior_artifact_digest_version_and_size_cannot_be_forged() {
    let mut value = context();
    let content = "Ada's complete original deliverable".to_owned();
    value.prior_artifact = Some(ContextArtifact {
        artifact_id: Uuid::new_v4(),
        run_id: Uuid::new_v4(),
        employee_id: EmployeeId::parse("ada").unwrap(),
        execution_version: 4,
        sha256: hex::encode(Sha256::digest(content.as_bytes())),
        content,
    });
    assert!(valid(&value));
    for change in 0..4 {
        let mut changed = value.clone();
        let artifact = changed.prior_artifact.as_mut().unwrap();
        match change {
            0 => artifact.sha256 = "0".repeat(64),
            1 => artifact.run_id = changed.snapshot_id,
            2 => artifact.execution_version = changed.execution_version,
            _ => {
                artifact.content = "x".repeat(MAX_ARTIFACT_BYTES + 1);
                artifact.sha256 = hex::encode(Sha256::digest(artifact.content.as_bytes()));
            }
        }
        assert!(!valid(&changed));
    }
}

#[test]
fn source_and_root_are_mandatory_and_foreign_threads_order_duplicates_and_bounds_refuse() {
    let mut value = context();
    let root = "c".repeat(64);
    let source = "d".repeat(64);
    value.source_message_id = Some(source.clone());
    value.thread_root_message_id = Some(root.clone());
    value.messages.push(message(&value, &root));
    assert!(!valid(&value));
    let mut reply = message(&value, &source);
    reply.created_at += chrono::Duration::seconds(1);
    reply.parent_message_id = Some(root.clone());
    reply.thread_root_message_id = Some(root);
    reply.selection = ContextSelection::ThreadRecent;
    value.messages.push(reply);
    assert!(valid(&value));
    for change in 0..7 {
        let mut changed = value.clone();
        match change {
            0 => {
                changed.messages.remove(0);
            }
            1 => {
                changed.messages[1].thread_root_message_id = Some("e".repeat(64));
            }
            2 => {
                changed.messages.reverse();
            }
            3 => {
                changed.messages.push(changed.messages[1].clone());
            }
            4 => {
                changed.messages[1].content = "x".repeat(MAX_WORK_MESSAGE_BYTES + 1);
            }
            5 => {
                changed.messages[1].selection = ContextSelection::ChannelRecent;
            }
            _ => {
                changed.teammates.push(changed.employee.clone());
            }
        };
        assert!(!valid(&changed), "case {change}");
    }
    let mut wire = serde_json::to_value(&value).unwrap();
    wire["new_permissions"] = serde_json::json!(["all"]);
    assert!(serde_json::from_value::<WorkContext>(wire).is_err());
}

#[test]
fn shared_wire_is_checked_at_the_production_run_spec_boundary() {
    use crate::runtime::{RunContext, RunSpec};
    let context: WorkContext = serde_json::from_str(include_str!("test_vector.json")).unwrap();
    assert!(valid(&context));
    let spec = RunSpec {
        run_id: context.snapshot_id,
        employee_id: context.employee.employee_id.clone(),
        revision_id: context.employee.revision_id,
        binding: serde_json::from_value(serde_json::json!({
            "adapter":"hermes", "profile_ref":"fixture", "model":"fixture",
            "workspace_ref":"none", "credential_refs":[], "options":{}
        }))
        .unwrap(),
        permissions: Default::default(),
        input: "Revise only the second item in the saved deliverable.".into(),
        context: RunContext {
            work_item_id: Some(context.work_item_id),
            work_context: Some(context),
            ..Default::default()
        },
        idempotency_key: "fixture-work-run".into(),
    };
    assert!(spec.validate().is_ok());
    for choice in 0..5 {
        let mut changed = spec.clone();
        match choice {
            0 => changed.context.work_item_id = None,
            1 => changed.context.work_item_id = Some(Uuid::new_v4()),
            2 => changed.context.conversation_ref = Some(Uuid::new_v4().to_string()),
            3 => changed.context.reply_to_message_id = Some("a".repeat(64)),
            _ => changed
                .context
                .work_context
                .as_mut()
                .unwrap()
                .prior_artifact
                .as_mut()
                .unwrap()
                .content
                .push('x'),
        }
        assert!(changed.validate().is_err(), "case {choice}");
    }
    let mut historical = spec;
    historical.context.work_context = None;
    assert!(historical.validate().is_ok());
    let wire = serde_json::to_value(&historical).unwrap();
    assert!(wire["context"].get("work_context").is_none());
}
