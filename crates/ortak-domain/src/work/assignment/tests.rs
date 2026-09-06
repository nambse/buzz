use super::*;

fn item() -> WorkItem {
    WorkItem::create(
        NewWorkItemIds {
            id: Uuid::new_v4(),
            criterion_ids: vec![Uuid::new_v4()],
            approval_ids: vec![],
        },
        &NewWorkItem {
            project_id: Uuid::new_v4(),
            title: "Assignment regression".into(),
            description: "Review preserved".into(),
            priority: WorkPriority::Normal,
            criteria: vec!["Human accepts the result".into()],
            approvals: vec![],
            source_message_id: None,
        },
    )
    .unwrap()
    .0
}

#[test]
fn assignment_replacement_and_role_change_are_atomic_and_preserve_review() {
    let mut item = item();
    let cem = EmployeeId::parse("cem").unwrap();
    let ada = EmployeeId::parse("ada").unwrap();
    item.assign(cem.clone(), AssignmentRole::Owner).unwrap();
    let before = item.clone();
    let event = item
        .reassign(
            &cem,
            ada.clone(),
            AssignmentRole::Contributor,
            "New owner".into(),
        )
        .unwrap();
    assert_eq!(item.version, before.version + 1);
    assert_eq!(item.assignments[0].status, AssignmentStatus::Released);
    assert_eq!(item.assignments[1].status, AssignmentStatus::Active);
    assert_eq!(item.criteria, before.criteria);
    assert_eq!(item.approvals, before.approvals);
    assert_eq!(item.state, before.state);
    assert_eq!(event.event_type(), "work.assignment_reassigned");
    item.reassign(
        &ada,
        ada.clone(),
        AssignmentRole::Owner,
        "Role correction".into(),
    )
    .unwrap();
    assert_eq!(item.version, before.version + 2);
    assert_eq!(item.assignments.len(), 2);
    item.release_assignment(&ada, "Unassign".into()).unwrap();
    item.assign(cem, AssignmentRole::Owner).unwrap();
    assert_eq!(
        item.assignments.len(),
        2,
        "released provenance is reused, never deleted"
    );
}

#[test]
fn assignment_rejections_do_not_partially_release_or_advance_version() {
    let mut item = item();
    let cem = EmployeeId::parse("cem").unwrap();
    let ada = EmployeeId::parse("ada").unwrap();
    item.assign(cem.clone(), AssignmentRole::Owner).unwrap();
    item.assign(ada.clone(), AssignmentRole::Contributor)
        .unwrap();
    let before = item.clone();
    for replacement in [cem.clone(), ada] {
        assert!(item
            .reassign(
                &cem,
                replacement,
                AssignmentRole::Owner,
                "No duplicate".into()
            )
            .is_err());
        assert_eq!(item, before);
    }
    for reason in [" ".into(), "x".repeat(MAX_WORK_REASON_BYTES + 1)] {
        assert!(item.release_assignment(&cem, reason).is_err());
        assert_eq!(item, before);
    }
    item.state = WorkState::Completed;
    let terminal = item.clone();
    assert!(matches!(
        item.release_assignment(&cem, "Frozen".into()),
        Err(DomainError::WorkItemTerminal { .. })
    ));
    assert_eq!(item, terminal);
    item.state = WorkState::Ready;
    item.version = i64::MAX;
    let overflow = item.clone();
    assert!(item.release_assignment(&cem, "Overflow".into()).is_err());
    assert_eq!(item, overflow);
}
