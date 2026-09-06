use super::*;

fn fixture() -> (WorkItem, NewWorkItem, EditWorkDefinition) {
    let input = NewWorkItem {
        project_id: Uuid::new_v4(),
        title: "Plan".into(),
        description: "Original description".into(),
        priority: WorkPriority::Normal,
        criteria: vec!["Original criterion".into()],
        approvals: vec![ApprovalGateSpec {
            gate: "review".into(),
            required: true,
        }],
        source_message_id: None,
    };
    let (item, _) = WorkItem::create(
        NewWorkItemIds {
            id: Uuid::new_v4(),
            criterion_ids: vec![Uuid::new_v4()],
            approval_ids: vec![Uuid::new_v4()],
        },
        &input,
    )
    .unwrap();
    let edit = EditWorkDefinition {
        title: Some("Revised".into()),
        description: Some("Revised description".into()),
        criteria: vec![CriterionEdit {
            id: item.criteria[0].id,
            text: Some("Revised criterion".into()),
        }],
        additional_criteria: vec!["Another criterion".into()],
    };
    (item, input, edit)
}
#[test]
fn definition_edit_advances_once_retains_ids_and_records_original_creation_fingerprint() {
    let (mut item, input, edit) = fixture();
    let original = item.clone();
    let added = Uuid::new_v4();
    let event = item.edit_definition(&edit, &[added]).unwrap();
    assert_eq!(item.version, 2);
    assert_eq!(Some(&item.title), edit.title.as_ref());
    assert_eq!(Some(&item.description), edit.description.as_ref());
    assert_eq!(item.criteria[0].id, original.criteria[0].id);
    assert_eq!(item.criteria[0].position, 0);
    assert_eq!(Some(&item.criteria[0].text), edit.criteria[0].text.as_ref());
    assert_eq!(item.criteria[1].id, added);
    assert_eq!(item.criteria[1].position, 1);
    assert_eq!(item.approvals, original.approvals);
    assert_eq!(
        event,
        WorkEvent::DefinitionEdited {
            previous_definition_hash: input.definition_fingerprint(),
            title_changed: true,
            description_changed: true,
            edited_criterion_ids: vec![original.criteria[0].id],
            added_criterion_ids: vec![added],
        }
    );
}
#[test]
fn definition_refusals_are_atomic_for_ids_text_review_evidence_and_terminal_state() {
    let (original, _, edit) = fixture();
    let mut cases = Vec::new();
    for state in [
        WorkState::Review,
        WorkState::Completed,
        WorkState::Cancelled,
    ] {
        let mut item = original.clone();
        item.state = state;
        cases.push((item, edit.clone()));
    }
    let mut accepted = original.clone();
    accepted
        .satisfy_criterion(accepted.criteria[0].id, WorkActor::System)
        .unwrap();
    cases.push((accepted, edit.clone()));
    let mut approved = original.clone();
    approved
        .resolve_approval(
            approved.approvals[0].id,
            ApprovalDecision::Approve,
            None,
            WorkActor::System,
        )
        .unwrap();
    cases.push((approved, edit.clone()));
    for bad in [
        EditWorkDefinition {
            criteria: vec![],
            ..edit.clone()
        },
        EditWorkDefinition {
            criteria: vec![CriterionEdit {
                id: Uuid::new_v4(),
                text: Some("Other".into()),
            }],
            ..edit.clone()
        },
        EditWorkDefinition {
            title: Some("é".repeat(101)),
            ..edit.clone()
        },
        EditWorkDefinition {
            description: Some("-----BEGIN PRIVATE KEY-----".into()),
            ..edit.clone()
        },
    ] {
        cases.push((original.clone(), bad));
    }
    for (mut item, bad) in cases {
        let before = item.clone();
        assert!(item.edit_definition(&bad, &[Uuid::new_v4()]).is_err());
        assert_eq!(item, before);
    }
}
#[test]
fn fingerprint_ignores_approval_order_but_not_definition_bytes() {
    let (_, mut input, _) = fixture();
    input.approvals.push(ApprovalGateSpec {
        gate: "other".into(),
        required: false,
    });
    let original = input.definition_fingerprint();
    input.approvals.reverse();
    assert_eq!(input.definition_fingerprint(), original);
    input.description.push('!');
    assert_ne!(input.definition_fingerprint(), original);
}

#[test]
fn null_preserves_canonical_text_that_the_safe_ui_projection_may_redact() {
    let (mut item, _, mut edit) = fixture();
    item.description = "Example: password=demo".into();
    item.criteria[0].text = "Document api_key=example".into();
    edit.description = None;
    edit.criteria[0].text = None;
    edit.additional_criteria.clear();
    item.edit_definition(&edit, &[]).unwrap();
    assert_eq!(item.title, "Revised");
    assert_eq!(item.description, "Example: password=demo");
    assert_eq!(item.criteria[0].text, "Document api_key=example");
}

#[test]
fn unchanged_edits_and_reused_or_missing_child_ids_cannot_advance_version() {
    let (original, _, mut edit) = fixture();
    edit.title = None;
    edit.description = None;
    edit.criteria[0].text = None;
    edit.additional_criteria.clear();
    let mut item = original.clone();
    assert!(item.edit_definition(&edit, &[]).is_err());
    assert_eq!(item, original);
    edit.additional_criteria.push("Appended".into());
    for ids in [vec![], vec![Uuid::nil()], vec![original.criteria[0].id]] {
        assert!(item.edit_definition(&edit, &ids).is_err());
        assert_eq!(item, original);
    }
}
