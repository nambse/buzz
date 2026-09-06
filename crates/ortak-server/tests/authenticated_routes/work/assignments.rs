//! Signed release/reassignment bind the persisted assignment, receipts and authority fences.
use super::execution::fixture;
use super::*;

fn app(f: &Fixture) -> Router {
    let mut cfg = config(f.community, &f.operator, f.channel);
    cfg.humans[0].can_create_projects = true;
    cfg.humans[0]
        .employee_ids
        .push(EmployeeId::parse("ada").unwrap());
    let mut reader = cfg.humans[0].clone();
    reader.public_key = f.reader.public_key().to_hex();
    cfg.humans.push(reader);
    product_router(f.control.clone(), cfg, Arc::new(Replay::default())).unwrap()
}

async fn replacement(f: &Fixture) -> Vec<u8> {
    let key = Keys::generate().public_key().to_bytes().to_vec();
    let revision = Uuid::new_v4();
    let mut tx = f.pool.begin().await.unwrap();
    sqlx::query("INSERT INTO employees(company_id,id) VALUES($1,'ada')")
        .bind(f.company)
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("INSERT INTO employee_revisions(company_id,id,employee_id,revision_number,manifest,manifest_fingerprint,provisioning_mode)
        VALUES($1,$2,'ada',1,$3,$4,'adopt')")
        .bind(f.company).bind(revision).bind(json!({"name":"Ada","office":{"public_key":hex::encode(&key),"signer_ref":"credential://fixture/ada"}}))
        .bind(vec![0x49u8;32]).execute(&mut *tx).await.unwrap();
    sqlx::query("INSERT INTO employee_office_bindings(company_id,employee_id,revision_id,provisioning_mode,public_key,signer_ref,verified_at)
        VALUES($1,'ada',$2,'adopt',$3,'credential://fixture/ada',clock_timestamp())")
        .bind(f.company).bind(revision).bind(&key).execute(&mut *tx).await.unwrap();
    sqlx::query("UPDATE employees SET status='active',active_revision_id=$2 WHERE company_id=$1 AND id='ada'")
        .bind(f.company).bind(revision).execute(&mut *tx).await.unwrap();
    sqlx::query(
        "INSERT INTO channel_members(community_id,channel_id,pubkey,role) VALUES($1,$2,$3,'bot')",
    )
    .bind(f.community)
    .bind(f.channel)
    .bind(&key)
    .execute(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();
    key
}

async fn current(f: &Fixture, app: &Router, work: Uuid) -> Value {
    let result = get(app, &f.operator, &format!("/api/v1/work-items/{work}")).await;
    assert_eq!(result.0, StatusCode::OK);
    result.1["work_item"].clone()
}

#[tokio::test]
#[ignore = "requires explicit disposable port55432 Postgres"]
async fn assignment_release_keeps_inactive_employee_recovery_and_current_permission_replay() {
    let f = Fixture::new().await;
    let employee = fixture::employee(&f).await;
    let app = app(&f);
    let (project, before) = fixture::ready(&f, &app).await;
    let path = format!("/api/v1/work-items/{}/assignments/cem/release", id(&before));
    let body = json!({"operation_id":Uuid::new_v4(),"expected_version":version(&before),"reason":"Employee unavailable"});
    let mut invalid = body.clone();
    invalid["actor"] = json!("injected");
    assert_eq!(
        post(&app, &f.operator, &path, &invalid).await.0,
        StatusCode::BAD_REQUEST
    );
    grant(&f, project, &f.reader, "viewer").await;
    assert_eq!(
        post(&app, &f.reader, &path, &body).await.0,
        StatusCode::FORBIDDEN
    );
    sqlx::query("UPDATE employees SET status='paused' WHERE company_id=$1 AND id='cem'")
        .bind(f.company)
        .execute(&f.pool)
        .await
        .unwrap();
    sqlx::query("UPDATE channel_members SET removed_at=clock_timestamp() WHERE community_id=$1 AND pubkey=$2")
        .bind(f.community).bind(hex::decode(employee.office.public_key).unwrap()).execute(&f.pool).await.unwrap();
    let saved = post(&app, &f.operator, &path, &body).await;
    assert_eq!(saved.0, StatusCode::OK);
    let after = &saved.1["work_item"];
    assert_eq!(version(after), version(&before) + 1);
    assert_eq!(after["assignments"][0]["status"], "released");
    assert_eq!(after["criteria"], before["criteria"]);
    assert_eq!(after["approvals"], before["approvals"]);
    assert_eq!(after["state"], before["state"]);
    assert_eq!(post(&app, &f.operator, &path, &body).await.1, saved.1);
    let queue = get(&app, &f.operator, "/api/v1/employees/cem/work-items").await;
    assert_eq!(queue.0, StatusCode::OK);
    assert_eq!(queue.1["work_items"], json!([]));
    let mut duplicate = body.clone();
    duplicate["operation_id"] = json!(Uuid::new_v4());
    duplicate["expected_version"] = json!(version(after));
    assert_eq!(
        post(&app, &f.operator, &path, &duplicate).await.0,
        StatusCode::CONFLICT
    );
    let evidence: (i64,i64,bool) = sqlx::query_as("SELECT
        (SELECT count(*) FROM work_item_history WHERE company_id=$1 AND work_item_id=$2 AND event_type='work.assignment_released'),
        (SELECT count(*) FROM work_api_operations WHERE company_id=$1 AND operation_id=$3),
        (SELECT released_at IS NOT NULL FROM work_assignments WHERE company_id=$1 AND work_item_id=$2 AND employee_id='cem')")
        .bind(f.company).bind(id(&before)).bind(Uuid::parse_str(body["operation_id"].as_str().unwrap()).unwrap()).fetch_one(&f.pool).await.unwrap();
    assert_eq!(evidence, (1, 1, true));
    sqlx::query("UPDATE project_access_grants SET revoked_at=clock_timestamp() WHERE company_id=$1 AND project_id=$2 AND actor_pubkey=$3")
        .bind(f.company).bind(project).bind(f.operator.public_key().to_hex()).execute(&f.pool).await.unwrap();
    assert_eq!(
        post(&app, &f.operator, &path, &body).await.0,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
#[ignore = "requires explicit disposable port55432 Postgres"]
async fn assignment_reassign_is_atomic_under_storage_failure_replay_and_target_revocation() {
    let f = Fixture::new().await;
    fixture::employee(&f).await;
    let target_key = replacement(&f).await;
    let app = app(&f);
    let (_, before) = fixture::ready(&f, &app).await;
    let work = id(&before);
    let path = format!("/api/v1/work-items/{work}/assignments/cem/reassign");
    let body = json!({"operation_id":Uuid::new_v4(),"expected_version":version(&before),"replacement_employee_id":"ada","role":"owner","reason":"Hand over work"});
    let name = format!("assignment_failure_{}", Uuid::new_v4().simple());
    let operation = body["operation_id"].as_str().unwrap();
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!("CREATE FUNCTION {name}() RETURNS TRIGGER LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'fixture receipt failure'; END $$;
        CREATE TRIGGER {name} BEFORE INSERT ON work_api_operations FOR EACH ROW WHEN(NEW.operation_id='{operation}'::uuid) EXECUTE FUNCTION {name}();"))).execute(&f.pool).await.unwrap();
    let failed = post(&app, &f.operator, &path, &body).await;
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
        "DROP TRIGGER {name} ON work_api_operations; DROP FUNCTION {name}();"
    )))
    .execute(&f.pool)
    .await
    .unwrap();
    assert_eq!(failed.0, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(current(&f, &app, work).await, before);
    let (a, b) = tokio::join!(
        post(&app, &f.operator, &path, &body),
        post(&app, &f.operator, &path, &body)
    );
    assert_eq!(a.0, StatusCode::OK);
    assert_eq!(b.0, StatusCode::OK);
    assert_eq!(a.1, b.1);
    let after = &a.1["work_item"];
    assert_eq!(version(after), version(&before) + 1);
    let statuses: Vec<(String,String)> = sqlx::query_as("SELECT employee_id,status FROM work_assignments WHERE company_id=$1 AND work_item_id=$2 ORDER BY employee_id")
        .bind(f.company).bind(work).fetch_all(&f.pool).await.unwrap();
    assert_eq!(
        statuses,
        vec![
            ("ada".into(), "active".into()),
            ("cem".into(), "released".into())
        ]
    );
    let mut changed = body.clone();
    changed["reason"] = json!("Different request");
    assert_eq!(
        post(&app, &f.operator, &path, &changed).await.0,
        StatusCode::CONFLICT
    );
    // Same employee role changes require one new receipt and keep a single row.
    let role_path = format!("/api/v1/work-items/{work}/assignments/ada/reassign");
    let role = json!({"operation_id":Uuid::new_v4(),"expected_version":version(after),"replacement_employee_id":"ada","role":"reviewer","reason":"Role correction"});
    let corrected = post(&app, &f.operator, &role_path, &role).await;
    assert_eq!(corrected.0, StatusCode::OK);
    assert_eq!(version(&corrected.1["work_item"]), version(after) + 1);
    assert_eq!(corrected.1["work_item"]["approvals"], before["approvals"]);
    sqlx::query("UPDATE channel_members SET removed_at=clock_timestamp() WHERE community_id=$1 AND channel_id=$2 AND pubkey=$3")
        .bind(f.community).bind(f.channel).bind(target_key).execute(&f.pool).await.unwrap();
    assert_eq!(
        post(&app, &f.operator, &path, &body).await.0,
        StatusCode::CONFLICT,
        "replay still requires current replacement eligibility"
    );
    let mut next = role.clone();
    next["operation_id"] = json!(Uuid::new_v4());
    next["expected_version"] = json!(version(&corrected.1["work_item"]));
    next["role"] = json!("owner");
    assert_eq!(
        post(&app, &f.operator, &role_path, &next).await.0,
        StatusCode::CONFLICT
    );
    assert_eq!(current(&f, &app, work).await, corrected.1["work_item"]);
}
