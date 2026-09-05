//! Signed manual Work routes against the disposable PostgreSQL fixture.
use super::*;
use ortak_domain::{AttachmentRef, NewWorkItem, WorkActor, WorkPriority};
use ortak_work::{AttachRecord, WorkService};

#[path = "work/boundaries.rs"]
mod boundaries;
#[path = "work/replays.rs"]
mod replays;

#[test]
fn omitted_project_creation_grant_stays_disabled() {
    let grant: HumanGrant = serde_json::from_value(json!({
        "public_key": Keys::generate().public_key().to_hex(), "role": "operator",
        "channel_ids": [Uuid::new_v4()], "employee_ids": ["cem"]
    }))
    .unwrap();
    assert!(!grant.can_create_projects);
}

fn work_app(f: &Fixture, create: bool, reader_role: Role, channels: Vec<Uuid>) -> Router {
    let mut cfg = config(f.community, &f.operator, f.channel);
    cfg.humans[0].can_create_projects = create;
    cfg.humans[0].channel_ids = channels;
    let mut reader = cfg.humans[0].clone();
    reader.public_key = f.reader.public_key().to_hex();
    reader.role = reader_role;
    cfg.humans.push(reader);
    product_router(f.control.clone(), cfg, Arc::new(Replay::default())).unwrap()
}

async fn post(app: &Router, actor: &Keys, path: &str, body: &Value) -> (StatusCode, Value) {
    // Every retry creates a fresh NIP-98 event, preserving only operation_id.
    response(app, signed(actor, "POST", path, &body.to_string(), true)).await
}

async fn get(app: &Router, actor: &Keys, path: &str) -> (StatusCode, Value) {
    response(app, signed(actor, "GET", path, "", false)).await
}

fn project_body(channel: Uuid) -> Value {
    json!({"operation_id":Uuid::new_v4(),"channel_id":channel,
        "project":{"slug":format!("work-{}",Uuid::new_v4().simple()),
        "name":"Manual project","description":"Private fixture project"}})
}

async fn project(f: &Fixture, app: &Router, channel: Uuid) -> Uuid {
    let (status, body) = post(app, &f.operator, "/api/v1/projects", &project_body(channel)).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    id(&body["project"])
}

fn item_body(title: &str) -> Value {
    json!({"operation_id":Uuid::new_v4(),"title":title,"description":"Manual work",
        "criteria":["Acceptance checked"],"approvals":[{"gate":"human_review","required":true}]})
}

fn id(item: &Value) -> Uuid {
    Uuid::parse_str(item["id"].as_str().unwrap()).unwrap()
}

fn version(item: &Value) -> i64 {
    item["version"].as_i64().unwrap()
}

async fn item(f: &Fixture, app: &Router, project: Uuid) -> Value {
    let (status, body) = post(
        app,
        &f.operator,
        &format!("/api/v1/projects/{project}/work-items"),
        &item_body("Manual task"),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    body["work_item"].clone()
}

async fn transition(f: &Fixture, app: &Router, item: Value, target: &str) -> Value {
    let (status, body) = post(
        app,
        &f.operator,
        &format!("/api/v1/work-items/{}/transitions", id(&item)),
        &json!({"operation_id":Uuid::new_v4(),"expected_version":version(&item),"target":target}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["work_item"]["state"], target);
    body["work_item"].clone()
}

async fn grant(f: &Fixture, project: Uuid, actor: &Keys, role: &str) {
    sqlx::query(
        "INSERT INTO project_access_grants(company_id,project_id,actor_pubkey,role,granted_by)
        VALUES($1,$2,$3,$4,$5) ON CONFLICT(company_id,project_id,actor_pubkey)
        DO UPDATE SET role=EXCLUDED.role,revoked_at=NULL",
    )
    .bind(f.company)
    .bind(project)
    .bind(actor.public_key().to_hex())
    .bind(role)
    .bind(f.operator.public_key().to_hex())
    .execute(&f.pool)
    .await
    .unwrap();
}

async fn employee_channel_binding(f: &Fixture) {
    let key = Keys::generate();
    let revision = Uuid::new_v4();
    let manifest = json!({"name":"Cem","office":{"public_key":key.public_key().to_hex(),
        "signer_ref":"credential://fixture/work-signer"}});
    sqlx::query("INSERT INTO employee_revisions(company_id,id,employee_id,revision_number,manifest,manifest_fingerprint,provisioning_mode)
        VALUES($1,$2,'cem',2,$3,$4,'create')")
        .bind(f.company).bind(revision).bind(&manifest).bind(Sha256::digest(manifest.to_string().as_bytes()).to_vec())
        .execute(&f.pool).await.unwrap();
    sqlx::query("UPDATE employees SET active_revision_id=$2 WHERE company_id=$1 AND id='cem'")
        .bind(f.company)
        .bind(revision)
        .execute(&f.pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO employee_office_bindings(company_id,employee_id,revision_id,provisioning_mode,
        public_key,signer_ref,verified_at) VALUES($1,'cem',$2,'create',$3,'credential://fixture/work-signer',now())")
        .bind(f.company).bind(revision).bind(key.public_key().to_bytes().as_slice())
        .execute(&f.pool).await.unwrap();
    sqlx::query(
        "INSERT INTO channel_members(community_id,channel_id,pubkey,role) VALUES($1,$2,$3,'bot')",
    )
    .bind(f.community)
    .bind(f.channel)
    .bind(key.public_key().to_bytes().as_slice())
    .execute(&f.pool)
    .await
    .unwrap();
}

async fn runtime_counts(f: &Fixture) -> (i64, i64, i64) {
    sqlx::query_as(
        "SELECT (SELECT count(*) FROM runs WHERE company_id=$1),
        (SELECT count(*) FROM outbox WHERE company_id=$1),
        (SELECT count(*) FROM routing_decisions WHERE company_id=$1)",
    )
    .bind(f.company)
    .fetch_one(&f.pool)
    .await
    .unwrap()
}

#[tokio::test]
#[ignore = "requires explicit disposable port55432 Postgres"]
async fn manual_owner_workflow_replays_once_and_never_dispatches_a_runtime() {
    let f = Fixture::new().await;
    let app = work_app(&f, true, Role::Reader, vec![f.channel]);
    let before = runtime_counts(&f).await;
    let create = project_body(f.channel);
    let (status, first) = post(&app, &f.operator, "/api/v1/projects", &create).await;
    assert_eq!(status, StatusCode::CREATED, "{first}");
    assert_eq!(first["project"]["role"], "owner");
    let project = id(&first["project"]);
    let (status, replay) = post(&app, &f.operator, "/api/v1/projects", &create).await;
    assert_eq!(status, StatusCode::OK, "{replay}");
    assert_eq!(replay["created"], false);
    assert_eq!(replay["project"], first["project"]);
    let mut changed = create.clone();
    changed["project"]["name"] = json!("Conflicting operation");
    assert_eq!(
        post(&app, &f.operator, "/api/v1/projects", &changed)
            .await
            .0,
        StatusCode::CONFLICT
    );

    let path = format!("/api/v1/projects/{project}/work-items");
    let create = item_body("Ship a manual fixture");
    let (status, first) = post(&app, &f.operator, &path, &create).await;
    assert_eq!(status, StatusCode::CREATED, "{first}");
    let (status, replay) = post(&app, &f.operator, &path, &create).await;
    assert_eq!(status, StatusCode::OK, "{replay}");
    assert_eq!(replay["created"], false);
    assert_eq!(replay["work_item"], first["work_item"]);
    let mut changed = create.clone();
    changed["title"] = json!("Changed replay");
    assert_eq!(
        post(&app, &f.operator, &path, &changed).await.0,
        StatusCode::CONFLICT
    );
    let current = first["work_item"].clone();
    let work = id(&current);
    let assign_path = format!("/api/v1/work-items/{work}/assignments");
    let assignment = json!({"operation_id":Uuid::new_v4(),"expected_version":1,"employee_id":"cem","role":"owner"});
    assert_eq!(
        post(&app, &f.operator, &assign_path, &assignment).await.0,
        StatusCode::CONFLICT,
        "active employee alone cannot substitute for verified Office channel binding"
    );
    employee_channel_binding(&f).await;
    let (status, assigned) = post(&app, &f.operator, &assign_path, &assignment).await;
    assert_eq!(status, StatusCode::OK, "{assigned}");
    let (status, replay) = post(&app, &f.operator, &assign_path, &assignment).await;
    assert_eq!(status, StatusCode::OK, "{replay}");
    assert_eq!(replay, assigned);
    let mut current = assigned["work_item"].clone();
    for state in ["ready", "in_progress", "review"] {
        current = transition(&f, &app, current, state).await;
    }
    let complete_path = format!("/api/v1/work-items/{work}/transitions");
    assert_eq!(post(&app, &f.operator, &complete_path,
        &json!({"operation_id":Uuid::new_v4(),"expected_version":version(&current),"target":"completed"})).await.0,
        StatusCode::CONFLICT, "pending acceptance and review block completion");
    let criterion = current["criteria"][0]["id"].as_str().unwrap();
    let (status, satisfied) = post(
        &app,
        &f.operator,
        &format!("/api/v1/work-items/{work}/criteria/{criterion}/satisfy"),
        &json!({"operation_id":Uuid::new_v4(),"expected_version":version(&current)}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{satisfied}");
    current = satisfied["work_item"].clone();
    let approval = current["approvals"][0]["id"].as_str().unwrap();
    let (status, approved) = post(&app, &f.operator,
        &format!("/api/v1/work-items/{work}/approvals/{approval}/resolve"),
        &json!({"operation_id":Uuid::new_v4(),"expected_version":version(&current),"decision":"approve"})).await;
    assert_eq!(status, StatusCode::OK, "{approved}");
    current = transition(&f, &app, approved["work_item"].clone(), "completed").await;
    assert_eq!(version(&current), 8);
    assert_eq!(current["execution_available"], false);
    assert_eq!(
        current["created_by"],
        json!({"type":"human","public_key":f.operator.public_key().to_hex()})
    );
    let history = current["history"].as_array().unwrap();
    assert_eq!(history.len(), 8);
    for (sequence, entry) in history.iter().enumerate() {
        assert_eq!(entry["sequence"], sequence as i64);
        assert_eq!(entry["version"], sequence as i64 + 1);
        assert_eq!(
            entry["actor"]["public_key"],
            f.operator.public_key().to_hex()
        );
        assert!(entry.get("payload").is_none());
    }
    let counts: (i64, i64, i64, i64) = sqlx::query_as(
        "SELECT
        (SELECT count(*) FROM projects WHERE company_id=$1),
        (SELECT count(*) FROM work_items WHERE company_id=$1),
        (SELECT count(*) FROM work_item_history WHERE company_id=$1),
        (SELECT count(*) FROM work_api_operations WHERE company_id=$1)",
    )
    .bind(f.company)
    .fetch_one(&f.pool)
    .await
    .unwrap();
    assert_eq!(
        counts,
        (1, 1, 8, 9),
        "conflicts and fresh-auth replays must leave no duplicate resources/history/receipts"
    );
    assert_eq!(runtime_counts(&f).await, before);
}

#[tokio::test]
#[ignore = "requires explicit disposable port55432 Postgres"]
async fn explicit_project_roles_and_creation_permission_cannot_be_bypassed_by_global_role() {
    let f = Fixture::new().await;
    let create = project_body(f.channel);
    let (status, capabilities) = get(&f.app, &f.operator, "/api/v1/projects").await;
    assert_eq!(status, StatusCode::OK, "{capabilities}");
    assert_eq!(capabilities["can_create_projects"], false);
    assert_eq!(capabilities["create_channels"], json!([]));
    assert_eq!(
        post(&f.app, &f.operator, "/api/v1/projects", &create)
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    let app = work_app(&f, true, Role::Reader, vec![f.channel]);
    let (status, capabilities) = get(&app, &f.reader, "/api/v1/projects").await;
    assert_eq!(status, StatusCode::OK, "{capabilities}");
    assert_eq!(capabilities["can_create_projects"], false);
    assert_eq!(capabilities["create_channels"], json!([]));
    assert_eq!(
        post(&app, &f.reader, "/api/v1/projects", &create).await.0,
        StatusCode::FORBIDDEN
    );
    let project = project(&f, &app, f.channel).await;
    let current = item(&f, &app, project).await;
    let create_path = format!("/api/v1/projects/{project}/work-items");
    let detail = format!("/api/v1/projects/{project}");
    let operator_app = work_app(&f, true, Role::Operator, vec![f.channel]);
    assert_eq!(
        get(&operator_app, &f.reader, &detail).await.0,
        StatusCode::NOT_FOUND,
        "global Operator has no implicit project grant"
    );
    grant(&f, project, &f.reader, "owner").await;
    let (status, detail) = get(&app, &f.reader, &detail).await;
    assert_eq!(status, StatusCode::OK, "{detail}");
    assert_eq!(detail["project"]["role"], "owner");
    assert_eq!(detail["project"]["can_contribute"], false);
    assert_eq!(detail["project"]["can_review"], false);
    assert_eq!(
        post(
            &app,
            &f.reader,
            &create_path,
            &item_body("Reader must not write")
        )
        .await
        .0,
        StatusCode::FORBIDDEN,
        "global Reader cannot mutate even with an owner project grant"
    );
    grant(&f, project, &f.reader, "viewer").await;
    assert_eq!(
        post(
            &operator_app,
            &f.reader,
            &create_path,
            &item_body("Viewer cannot contribute")
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    grant(&f, project, &f.reader, "contributor").await;
    assert_eq!(
        post(
            &operator_app,
            &f.reader,
            &create_path,
            &item_body("Contributor task")
        )
        .await
        .0,
        StatusCode::CREATED
    );
    let criterion = current["criteria"][0]["id"].as_str().unwrap();
    let satisfy = format!(
        "/api/v1/work-items/{}/criteria/{criterion}/satisfy",
        id(&current)
    );
    let command = json!({"operation_id":Uuid::new_v4(),"expected_version":version(&current)});
    assert_eq!(
        post(&operator_app, &f.reader, &satisfy, &command).await.0,
        StatusCode::FORBIDDEN
    );
    grant(&f, project, &f.reader, "reviewer").await;
    assert_eq!(
        post(
            &operator_app,
            &f.reader,
            &create_path,
            &item_body("Reviewer cannot contribute")
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    let (status, reviewed) = post(&operator_app, &f.reader, &satisfy, &command).await;
    assert_eq!(status, StatusCode::OK, "{reviewed}");
    assert_eq!(
        reviewed["work_item"]["criteria"][0]["satisfied_by"]["public_key"],
        f.reader.public_key().to_hex()
    );
    assert_eq!(runtime_counts(&f).await, (0, 0, 0));
}
