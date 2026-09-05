//! Real repository reads across mixed audiences and held mutation fences.
use super::*;

async fn assigned(
    f: &ApiFixture,
    project: Uuid,
    role: AssignmentRole,
    source: Option<MessageId>,
) -> Uuid {
    let mut input = f.item_input(project);
    input.source_message_id = source.map(|v| v.to_hex());
    let item = f
        .company
        .service
        .create_work_item(&f.company.scope, input, human())
        .await
        .unwrap()
        .item;
    f.company
        .service
        .assign_employee(
            &f.company.scope,
            AssignEmployee {
                work_item_id: item.item.id,
                expected_version: item.item.version,
                employee_id: employee("cem"),
                role,
                actor: human(),
            },
        )
        .await
        .unwrap()
        .item
        .id
}

fn audience(
    f: &ApiFixture,
    key: MessageId,
    channels: BTreeSet<Uuid>,
    employees: BTreeSet<EmployeeId>,
) -> AuthorizedWork {
    AuthorizedWork::new(
        f.company.control.clone(),
        f.company.scope.clone(),
        ApiWorkPrincipal::new(
            f.company.community_id,
            key.to_hex(),
            *message_id().as_bytes(),
            true,
            false,
            channels,
            employees,
        )
        .unwrap(),
    )
}

#[tokio::test]
#[ignore = "requires disposable Postgres"]
async fn queue_filters_before_limit_paginates_all_roles_and_keeps_inactive_assignments() {
    let f = ApiFixture::new().await;
    let visible = f.project().await;
    let mut expected = Vec::new();
    for role in [
        AssignmentRole::Owner,
        AssignmentRole::Contributor,
        AssignmentRole::Reviewer,
    ] {
        expected.push((assigned(&f, visible, role, None).await, role));
    }
    // Newer ineligible rows must not consume page positions or the lookahead.
    let ungranted = f.project().await;
    assigned(&f, ungranted, AssignmentRole::Owner, None).await;
    sqlx::query(
        "UPDATE project_access_grants SET revoked_at=now() WHERE company_id=$1 AND project_id=$2",
    )
    .bind(f.company.scope.company_id())
    .bind(ungranted)
    .execute(&f.company.pool)
    .await
    .unwrap();
    let outside = f.company.project("internal-only").await;
    assigned(&f, outside, AssignmentRole::Owner, None).await;
    let source = f.source(f.hidden).await;
    assigned(&f, visible, AssignmentRole::Owner, Some(source)).await;
    let released = assigned(&f, visible, AssignmentRole::Owner, None).await;
    sqlx::query("UPDATE work_assignments SET status='released',released_at=now() WHERE company_id=$1 AND work_item_id=$2")
        .bind(f.company.scope.company_id()).bind(released).execute(&f.company.pool).await.unwrap();
    let terminal = assigned(&f, visible, AssignmentRole::Owner, None).await;
    let item = f
        .company
        .service
        .work_item(&f.company.scope, terminal)
        .await
        .unwrap();
    f.company
        .transition(&item, WorkState::Cancelled)
        .await
        .unwrap();
    let archived = f.project().await;
    assigned(&f, archived, AssignmentRole::Owner, None).await;
    f.company
        .service
        .archive_project(
            &f.company.scope,
            ArchiveProject {
                project_id: archived,
                expected_version: 1,
                actor: human(),
                reason: None,
            },
        )
        .await
        .unwrap();
    sqlx::query("UPDATE employees SET status='paused' WHERE company_id=$1 AND id='cem'")
        .bind(f.company.scope.company_id())
        .execute(&f.company.pool)
        .await
        .unwrap();
    expected.reverse();
    let mut cursor = None;
    for (index, (id, role)) in expected.iter().enumerate() {
        let page = f
            .api
            .employee_queue(&employee("cem"), cursor.as_deref(), 1)
            .await
            .unwrap();
        assert_eq!(page.items.len(), 1);
        assert_eq!(page.items[0].work.id, *id);
        assert_eq!(page.items[0].assignment_role, *role);
        assert_eq!(page.next_cursor.is_some(), index < expected.len() - 1);
        cursor = page.next_cursor;
    }
    assert!(matches!(
        f.api.employee_queue(&employee("zeynep"), None, 25).await,
        Err(WorkError::EmployeeNotFound { .. })
    ));
}

#[tokio::test]
#[ignore = "requires disposable Postgres"]
async fn queue_cursor_binds_employee_principal_and_both_configured_audiences() {
    let f = ApiFixture::new().await;
    let project = f.project().await;
    for _ in 0..2 {
        assigned(&f, project, AssignmentRole::Owner, None).await;
    }
    let channels = BTreeSet::from([f.channel, f.hidden]);
    let employees = BTreeSet::from([employee("cem"), employee("zeynep")]);
    let api = audience(&f, f.key, channels.clone(), employees.clone());
    let first = api.employee_queue(&employee("cem"), None, 1).await.unwrap();
    let cursor = first.next_cursor.unwrap();
    for (other, target) in [
        (
            audience(&f, message_id(), channels.clone(), employees.clone()),
            employee("cem"),
        ),
        (
            audience(&f, f.key, BTreeSet::from([f.channel]), employees.clone()),
            employee("cem"),
        ),
        (
            audience(
                &f,
                f.key,
                channels.clone(),
                BTreeSet::from([employee("cem")]),
            ),
            employee("cem"),
        ),
        (audience(&f, f.key, channels, employees), employee("zeynep")),
    ] {
        assert!(matches!(
            other.employee_queue(&target, Some(&cursor), 1).await,
            Err(WorkError::InvalidQuery(_))
        ));
    }
    let second = api
        .employee_queue(&employee("cem"), Some(&cursor), 1)
        .await
        .unwrap();
    assert_ne!(first.items[0].work.id, second.items[0].work.id);
    assert!(second.next_cursor.is_none());
}

#[tokio::test]
#[ignore = "requires disposable Postgres"]
async fn queue_rechecks_project_grant_after_waiting_for_revocation() {
    let f = ApiFixture::new().await;
    let project = f.project().await;
    assigned(&f, project, AssignmentRole::Owner, None).await;
    let mut held = f.company.pool.begin().await.unwrap();
    let pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *held)
        .await
        .unwrap();
    sqlx::query(
        "UPDATE project_access_grants SET revoked_at=now() WHERE company_id=$1 AND project_id=$2",
    )
    .bind(f.company.scope.company_id())
    .bind(project)
    .execute(&mut *held)
    .await
    .unwrap();
    let api = f.api.clone();
    let pending = tokio::spawn(async move { api.employee_queue(&employee("cem"), None, 25).await });
    f.wait_blocked(pid, 1).await;
    held.commit().await.unwrap();
    assert!(matches!(
        pending.await.unwrap(),
        Err(WorkError::ProjectNotFound { .. })
    ));
    assert!(f
        .api
        .employee_queue(&employee("cem"), None, 25)
        .await
        .unwrap()
        .items
        .is_empty());
}

#[tokio::test]
#[ignore = "requires disposable Postgres"]
async fn queue_rechecks_assignment_release_after_waiting_for_its_row() {
    let f = ApiFixture::new().await;
    let project = f.project().await;
    let item = assigned(&f, project, AssignmentRole::Reviewer, None).await;
    let mut held = f.company.pool.begin().await.unwrap();
    let pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *held)
        .await
        .unwrap();
    sqlx::query("UPDATE work_assignments SET status='released',released_at=now() WHERE company_id=$1 AND work_item_id=$2")
        .bind(f.company.scope.company_id()).bind(item).execute(&mut *held).await.unwrap();
    let api = f.api.clone();
    let pending = tokio::spawn(async move { api.employee_queue(&employee("cem"), None, 25).await });
    f.wait_blocked(pid, 1).await;
    held.commit().await.unwrap();
    assert!(matches!(
        pending.await.unwrap(),
        Err(WorkError::OperationConflict)
    ));
    assert!(f
        .api
        .employee_queue(&employee("cem"), None, 25)
        .await
        .unwrap()
        .items
        .is_empty());
}

#[tokio::test]
#[ignore = "requires disposable Postgres"]
async fn queue_page_ceiling_and_existing_employee_check_are_enforced() {
    let f = ApiFixture::new().await;
    let project = f.project().await;
    for _ in 0..26 {
        assigned(&f, project, AssignmentRole::Owner, None).await;
    }
    let page = f
        .api
        .employee_queue(&employee("cem"), None, u32::MAX)
        .await
        .unwrap();
    assert_eq!(page.items.len(), 25);
    let next = f
        .api
        .employee_queue(&employee("cem"), page.next_cursor.as_deref(), 25)
        .await
        .unwrap();
    assert_eq!(next.items.len(), 1);
    assert!(next.next_cursor.is_none());
    assert!(!page
        .items
        .iter()
        .any(|entry| entry.work.id == next.items[0].work.id));
    assert_eq!(
        f.api
            .employee_queue(&employee("cem"), None, 0)
            .await
            .unwrap()
            .items
            .len(),
        1
    );
    let absent = employee("not-provisioned");
    let api = audience(
        &f,
        f.key,
        BTreeSet::from([f.channel]),
        BTreeSet::from([absent.clone()]),
    );
    assert!(matches!(
        api.employee_queue(&absent, None, 25).await,
        Err(WorkError::EmployeeNotFound { .. })
    ));
}
