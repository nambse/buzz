//! Actual identity rotation and clock expiry at the authenticated facade seam.
use super::*;

async fn office_revision(
    f: &ApiFixture,
    number: i64,
    key: MessageId,
    member: bool,
    finite: bool,
) -> Uuid {
    let revision = Uuid::new_v4();
    let signer = format!("credential://fixture/work-key-{number}");
    let manifest = serde_json::json!({"office":{"public_key":key.to_hex(),"signer_ref":signer}});
    sqlx::query("INSERT INTO employee_revisions(company_id,id,employee_id,revision_number,manifest,manifest_fingerprint,provisioning_mode)
 VALUES($1,$2,'cem',$3,$4,$5,'create')")
        .bind(f.company.scope.company_id()).bind(revision).bind(number).bind(&manifest)
        .bind(Sha256::digest(manifest.to_string().as_bytes()).to_vec()).execute(&f.company.pool).await.unwrap();
    sqlx::query("INSERT INTO employee_office_bindings(company_id,employee_id,revision_id,provisioning_mode,public_key,signer_ref,verified_at,valid_until)
 VALUES($1,'cem',$2,'create',$3,$4,now(),CASE WHEN $5 THEN clock_timestamp()+interval '10 minutes' ELSE NULL END)")
        .bind(f.company.scope.company_id()).bind(revision).bind(key.as_bytes().as_slice()).bind(signer).bind(finite).execute(&f.company.pool).await.unwrap();
    if member {
        sqlx::query("INSERT INTO channel_members(community_id,channel_id,pubkey,role) VALUES($1,$2,$3,'bot')")
            .bind(f.company.community_id).bind(f.channel).bind(key.as_bytes().as_slice()).execute(&f.company.pool).await.unwrap();
    }
    sqlx::query("UPDATE employees SET active_revision_id=$2 WHERE company_id=$1 AND id='cem'")
        .bind(f.company.scope.company_id())
        .bind(revision)
        .execute(&f.company.pool)
        .await
        .unwrap();
    revision
}

#[tokio::test]
#[ignore = "requires disposable Postgres"]
async fn assignment_requires_the_active_manifest_identity_and_accepts_live_finite_binding() {
    let f = ApiFixture::new().await;
    let project = f.project().await;
    let old_key = message_id();
    office_revision(&f, 2, old_key, true, false).await;
    let item = f
        .api
        .create_work_item(Uuid::new_v4(), f.item_input(project))
        .await
        .unwrap()
        .item;
    let action = WorkMutation::Assign {
        employee_id: employee("cem"),
        role: AssignmentRole::Owner,
    };
    let assigned = f
        .api
        .mutate(Uuid::new_v4(), item.item.id, 1, action.clone())
        .await
        .unwrap();
    assert_eq!(assigned.item.assignments.len(), 1);
    // Old verified binding and channel membership remain. The new active
    // identity must independently belong to this channel.
    let new_key = message_id();
    office_revision(&f, 3, new_key, false, true).await;
    let item = f
        .api
        .create_work_item(Uuid::new_v4(), f.item_input(project))
        .await
        .unwrap()
        .item;
    let op = Uuid::new_v4();
    assert!(matches!(
        f.api
            .mutate(op, item.item.id, item.item.version, action.clone())
            .await,
        Err(WorkError::EmployeeNotAssignable { .. })
    ));
    sqlx::query(
        "INSERT INTO channel_members(community_id,channel_id,pubkey,role) VALUES($1,$2,$3,'bot')",
    )
    .bind(f.company.community_id)
    .bind(f.channel)
    .bind(new_key.as_bytes().as_slice())
    .execute(&f.company.pool)
    .await
    .unwrap();
    let assigned = f
        .api
        .mutate(op, item.item.id, item.item.version, action)
        .await
        .unwrap();
    assert_eq!(
        assigned.item.version, 2,
        "a refused operation must leave no success receipt"
    );
}

#[tokio::test]
#[ignore = "requires disposable Postgres"]
async fn elapsed_office_witness_rolls_back_a_mutation_after_project_wait() {
    let f = ApiFixture::new().await;
    let project = f.project().await;
    let key = message_id();
    office_revision(&f, 2, key, true, true).await;
    let item = f
        .api
        .create_work_item(Uuid::new_v4(), f.item_input(project))
        .await
        .unwrap()
        .item;
    sqlx::query("UPDATE employee_office_bindings SET valid_until=clock_timestamp()+interval '250 milliseconds'
 WHERE company_id=$1 AND public_key=$2")
        .bind(f.company.scope.company_id()).bind(key.as_bytes().as_slice()).execute(&f.company.pool).await.unwrap();
    let mut held = f.company.pool.begin().await.unwrap();
    let pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *held)
        .await
        .unwrap();
    sqlx::query("SELECT id FROM projects WHERE company_id=$1 AND id=$2 FOR UPDATE")
        .bind(f.company.scope.company_id())
        .bind(project)
        .execute(&mut *held)
        .await
        .unwrap();
    let api = f.api.clone();
    let id = item.item.id;
    let op = Uuid::new_v4();
    let task = tokio::spawn(async move {
        api.mutate(
            op,
            id,
            1,
            WorkMutation::Transition {
                target: WorkState::Ready,
                reason: None,
            },
        )
        .await
    });
    f.wait_blocked(pid, 1).await;
    // Wait on the database clock, bounded below the facade's 500ms lock timeout.
    sqlx::query(
        "SELECT pg_sleep(GREATEST(0,EXTRACT(EPOCH FROM valid_until-clock_timestamp()))+0.02)
 FROM employee_office_bindings WHERE company_id=$1 AND public_key=$2",
    )
    .bind(f.company.scope.company_id())
    .bind(key.as_bytes().as_slice())
    .execute(&f.company.pool)
    .await
    .unwrap();
    held.commit().await.unwrap();
    assert!(matches!(
        task.await.unwrap(),
        Err(WorkError::OperationTimedOut)
    ));
    let persisted = f
        .company
        .service
        .work_item(&f.company.scope, id)
        .await
        .unwrap();
    assert_eq!(persisted.item.version, 1);
    assert_eq!(persisted.item.state, WorkState::Proposed);
    let receipts: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM work_api_operations WHERE company_id=$1 AND operation_id=$2",
    )
    .bind(f.company.scope.company_id())
    .bind(op)
    .fetch_one(&f.company.pool)
    .await
    .unwrap();
    assert_eq!(receipts, 0);
}
