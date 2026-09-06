use super::*;

fn ready() -> (WorkItem, EmployeeId) {
    let employee = EmployeeId::parse("executor").unwrap();
    let (mut item, _) = WorkItem::create(
        NewWorkItemIds {
            id: Uuid::new_v4(),
            criterion_ids: vec![Uuid::new_v4()],
            approval_ids: vec![Uuid::new_v4()],
        },
        &NewWorkItem {
            project_id: Uuid::new_v4(),
            title: "Explain the result".into(),
            description: "Include evidence".into(),
            priority: WorkPriority::Normal,
            criteria: vec!["Human checks the evidence".into()],
            approvals: vec![ApprovalGateSpec {
                gate: "human_review".into(),
                required: true,
            }],
            source_message_id: None,
        },
    )
    .unwrap();
    item.assign(employee.clone(), AssignmentRole::Owner)
        .unwrap();
    item.transition(WorkState::Ready, None).unwrap();
    (item, employee)
}

#[test]
fn execution_and_result_each_advance_once_without_accepting_human_review() {
    let (mut item, employee) = ready();
    let criteria = item.criteria.clone();
    let approvals = item.approvals.clone();
    let version = item.version;
    let run = Uuid::new_v4();
    let artifact = Uuid::new_v4();
    let request = item
        .request_execution(run, &employee, Uuid::new_v4())
        .unwrap();
    assert_eq!(item.version, version + 1);
    assert_eq!(
        request.state_change(),
        Some((WorkState::Ready, WorkState::InProgress))
    );
    let result = item
        .execution_result_ready(run, artifact, Uuid::new_v4())
        .unwrap();
    assert_eq!(item.version, version + 2);
    assert_eq!(
        result.state_change(),
        Some((WorkState::InProgress, WorkState::Review))
    );
    assert_eq!(item.criteria, criteria);
    assert_eq!(item.approvals, approvals);
    assert!(item.transition(WorkState::Completed, None).is_err());
    assert_eq!(item.attachments.len(), 2);
}

#[test]
fn nonexecutor_dependency_and_existing_review_refusals_preserve_the_whole_item() {
    let (base, employee) = ready();
    for case in 0..4 {
        let mut item = base.clone();
        match case {
            0 => item.assignments[0].role = AssignmentRole::Reviewer,
            1 => item.assignments[0].status = AssignmentStatus::Released,
            2 => item.criteria[0].status = CriterionStatus::Satisfied,
            _ => item.dependencies.push(WorkDependency {
                depends_on: Uuid::new_v4(),
                depends_on_state: WorkState::Ready,
            }),
        }
        let before = item.clone();
        assert!(item
            .request_execution(Uuid::new_v4(), &employee, Uuid::new_v4())
            .is_err());
        assert_eq!(item, before);
    }
}

#[test]
fn late_or_unrelated_result_cannot_change_work_or_review_evidence() {
    let (mut item, employee) = ready();
    let run = Uuid::new_v4();
    item.request_execution(run, &employee, Uuid::new_v4())
        .unwrap();
    let before = item.clone();
    assert!(item
        .execution_result_ready(Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4())
        .is_err());
    assert_eq!(item, before);
    item.transition(WorkState::Cancelled, None).unwrap();
    let before = item.clone();
    assert!(item
        .execution_result_ready(run, Uuid::new_v4(), Uuid::new_v4())
        .is_err());
    assert_eq!(item, before);
}

#[test]
fn canonical_input_preserves_criteria_and_refuses_oversize_instead_of_truncating() {
    let (mut item, _) = ready();
    let input: serde_json::Value =
        serde_json::from_slice(&item.execution_input().unwrap()).unwrap();
    assert_eq!(
        input["acceptance_criteria"][0]["id"],
        item.criteria[0].id.to_string()
    );
    assert_eq!(
        input["acceptance_criteria"][0]["text"],
        item.criteria[0].text
    );
    item.description = "x".repeat(32 * 1024);
    assert!(item.execution_input().is_err());
}
