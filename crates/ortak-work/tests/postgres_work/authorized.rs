//! Production facade tests: atomic receipts, ACL contention, source and identity gates.
use super::*;
use ortak_work::{ApiWorkPrincipal, AuthorizedWork, WorkMutation};
use std::collections::BTreeSet;

struct ApiFixture {
    company: Company,
    channel: Uuid,
    hidden: Uuid,
    key: MessageId,
    api: AuthorizedWork,
}
impl ApiFixture {
    async fn new() -> Self {
        let pool = setup_pool().await;
        let company = Company::new(&pool).await;
        let key = message_id();
        let channel = Uuid::new_v4();
        let hidden = Uuid::new_v4();
        for id in [channel, hidden] {
            sqlx::query("INSERT INTO channels(community_id,id,name,created_by,visibility) VALUES($1,$2,$3,$4,'private')")
                .bind(company.community_id).bind(id).bind(format!("work-api-{}",id.simple())).bind(key.as_bytes().as_slice()).execute(&pool).await.unwrap();
            sqlx::query(
                "INSERT INTO channel_members(community_id,channel_id,pubkey) VALUES($1,$2,$3)",
            )
            .bind(company.community_id)
            .bind(id)
            .bind(key.as_bytes().as_slice())
            .execute(&pool)
            .await
            .unwrap();
        }
        let principal = ApiWorkPrincipal::new(
            company.community_id,
            key.to_hex(),
            *message_id().as_bytes(),
            true,
            true,
            BTreeSet::from([channel, hidden]),
            BTreeSet::from([employee("cem")]),
        )
        .unwrap();
        let api = AuthorizedWork::new(company.control.clone(), company.scope.clone(), principal);
        Self {
            company,
            channel,
            hidden,
            key,
            api,
        }
    }
    fn project_input(&self) -> NewProject {
        NewProject {
            slug: ProjectSlug::parse(format!("p-{}", Uuid::new_v4().simple())).unwrap(),
            name: "Scoped project".into(),
            description: String::new(),
        }
    }
    async fn project(&self) -> Uuid {
        self.api
            .create_project(Uuid::new_v4(), self.channel, self.project_input())
            .await
            .unwrap()
            .project
            .record
            .project
            .id
    }
    fn item_input(&self, project: Uuid) -> NewWorkItem {
        NewWorkItem {
            project_id: project,
            title: "Manual task".into(),
            description: String::new(),
            priority: WorkPriority::Normal,
            criteria: vec![],
            approvals: vec![],
            source_message_id: None,
        }
    }
    async fn source(&self, channel: Uuid) -> MessageId {
        let id = message_id();
        let created = Utc::now();
        sqlx::query("INSERT INTO events(community_id,id,pubkey,created_at,kind,tags,content,sig,channel_id) VALUES($1,$2,$3,$4,9,$5,'Scoped source',$6,$7)")
            .bind(self.company.community_id).bind(id.as_bytes().as_slice()).bind(self.key.as_bytes().as_slice()).bind(created)
            .bind(serde_json::json!([["h",channel.to_string()]])).bind([0_u8;64].as_slice()).bind(channel).execute(&self.company.pool).await.unwrap();
        self.company
            .control
            .insert_accepted_event(
                self.company.community_id,
                &InboxEvent {
                    event_id: id,
                    event_created_at: created,
                    event_kind: 9,
                    author_pubkey: *self.key.as_bytes(),
                    channel_id: Some(channel),
                },
            )
            .await
            .unwrap();
        sqlx::query("UPDATE office_inbox SET state='decided',finalized_at=now() WHERE company_id=$1 AND event_id=$2")
            .bind(self.company.scope.company_id()).bind(id.as_bytes().as_slice()).execute(&self.company.pool).await.unwrap();
        id
    }
    async fn wait_blocked(&self, pid: i32, count: i64) {
        for _ in 0..100 {
            let found: i64 = sqlx::query_scalar(
                "SELECT count(*) FROM pg_stat_activity WHERE $1=ANY(pg_blocking_pids(pid))",
            )
            .bind(pid)
            .fetch_one(&self.company.pool)
            .await
            .unwrap();
            if found >= count {
                return;
            }
            tokio::time::sleep(Duration::from_millis(3)).await;
        }
        panic!("production operations did not reach the held project fence");
    }
}

#[tokio::test]
#[ignore = "requires disposable Postgres"]
async fn concurrent_operation_replay_commits_one_item_history_and_receipt() {
    let f = ApiFixture::new().await;
    let project = f.project().await;
    let op = Uuid::new_v4();
    let input = f.item_input(project);
    let (a, b) = tokio::join!(
        f.api.create_work_item(op, input.clone()),
        f.api.create_work_item(op, input.clone())
    );
    let a = a.unwrap();
    let b = b.unwrap();
    assert_ne!(a.created, b.created);
    assert_eq!(a.item.item.id, b.item.item.id);
    let counts: (i64, i64, i64) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM work_items WHERE company_id=$1 AND project_id=$2),
 (SELECT count(*) FROM work_item_history WHERE company_id=$1 AND work_item_id=$3),
 (SELECT count(*) FROM work_api_operations WHERE company_id=$1 AND operation_id=$4)",
    )
    .bind(f.company.scope.company_id())
    .bind(project)
    .bind(a.item.item.id)
    .bind(op)
    .fetch_one(&f.company.pool)
    .await
    .unwrap();
    assert_eq!(counts, (1, 1, 1));
    let mut changed = input;
    changed.description = "Different immutable operation".into();
    assert!(matches!(
        f.api.create_work_item(op, changed).await,
        Err(WorkError::OperationConflict)
    ));
}

#[tokio::test]
#[ignore = "requires disposable Postgres"]
async fn receipt_failure_rolls_back_project_binding_and_owner_together() {
    let f = ApiFixture::new().await;
    let op = Uuid::new_v4();
    let name = format!("fixture_receipt_{}", op.simple());
    // Inject one real database storage failure at the final receipt seam. The
    // generated trigger affects this unique operation only, and is always removed.
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!("CREATE FUNCTION {name}() RETURNS TRIGGER LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'fixture receipt failure' USING ERRCODE='serialization_failure'; END $$;
 CREATE TRIGGER {name} BEFORE INSERT ON work_api_operations FOR EACH ROW WHEN (NEW.operation_id='{op}'::uuid) EXECUTE FUNCTION {name}();")))
        .execute(&f.company.pool).await.unwrap();
    let input = f.project_input();
    let slug = input.slug.to_string();
    let result = f.api.create_project(op, f.channel, input).await;
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
        "DROP TRIGGER {name} ON work_api_operations; DROP FUNCTION {name}();"
    )))
    .execute(&f.company.pool)
    .await
    .unwrap();
    assert!(result.is_err());
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM projects WHERE company_id=$1 AND slug=$2")
            .bind(f.company.scope.company_id())
            .bind(slug)
            .fetch_one(&f.company.pool)
            .await
            .unwrap();
    assert_eq!(
        count, 0,
        "receipt failure must roll back the preceding project and both FK-owned access rows"
    );
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM project_access_grants WHERE company_id=$1")
            .bind(f.company.scope.company_id())
            .fetch_one(&f.company.pool)
            .await
            .unwrap();
    assert_eq!(count, 0);
}

#[tokio::test]
#[ignore = "requires disposable Postgres"]
async fn acl_existing_and_absent_writes_obey_the_project_fence() {
    let f = ApiFixture::new().await;
    let project = f.project().await;
    let mut held = f.company.pool.begin().await.unwrap();
    sqlx::query("SELECT id FROM projects WHERE company_id=$1 AND id=$2 FOR SHARE")
        .bind(f.company.scope.company_id())
        .bind(project)
        .execute(&mut *held)
        .await
        .unwrap();
    let error=sqlx::query("UPDATE project_access_grants SET revoked_at=now() WHERE company_id=$1 AND project_id=$2 AND actor_pubkey=$3")
        .bind(f.company.scope.company_id()).bind(project).bind(f.key.to_hex()).execute(&f.company.pool).await.unwrap_err();
    assert_eq!(
        error.as_database_error().unwrap().code().as_deref(),
        Some("55P03")
    );
    let error=sqlx::query("INSERT INTO project_access_grants(company_id,project_id,actor_pubkey,role,granted_by) VALUES($1,$2,$3,'viewer',$4)")
        .bind(f.company.scope.company_id()).bind(project).bind(message_id().to_hex()).bind(f.key.to_hex()).execute(&f.company.pool).await.unwrap_err();
    assert_eq!(
        error.as_database_error().unwrap().code().as_deref(),
        Some("55P03")
    );
    held.rollback().await.unwrap();
    assert_eq!(
        f.api.project(project).await.unwrap().role,
        ortak_work::ProjectRole::Owner
    );
}

#[tokio::test]
#[ignore = "requires disposable Postgres"]
async fn waiting_project_list_and_mutation_recheck_committed_grant_revocation() {
    let f = ApiFixture::new().await;
    let project = f.project().await;
    let item = f
        .api
        .create_work_item(Uuid::new_v4(), f.item_input(project))
        .await
        .unwrap()
        .item;
    let mut held = f.company.pool.begin().await.unwrap();
    let pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *held)
        .await
        .unwrap();
    sqlx::query("UPDATE project_access_grants SET revoked_at=now() WHERE company_id=$1 AND project_id=$2 AND actor_pubkey=$3")
        .bind(f.company.scope.company_id()).bind(project).bind(f.key.to_hex()).execute(&mut *held).await.unwrap();
    let api = f.api.clone();
    let listed = tokio::spawn(async move { api.list_projects(None, 25).await });
    let api = f.api.clone();
    let id = item.item.id;
    let op = Uuid::new_v4();
    let mutated = tokio::spawn(async move {
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
    f.wait_blocked(pid, 2).await;
    held.commit().await.unwrap();
    assert!(matches!(
        listed.await.unwrap(),
        Err(WorkError::ProjectNotFound { .. })
    ));
    assert!(matches!(
        mutated.await.unwrap(),
        Err(WorkError::WorkItemNotFound { .. })
    ));
    let counts: (i64, i64) = sqlx::query_as(
        "SELECT (SELECT version FROM work_items WHERE company_id=$1 AND id=$2),
 (SELECT count(*) FROM work_api_operations WHERE company_id=$1 AND operation_id=$3)",
    )
    .bind(f.company.scope.company_id())
    .bind(id)
    .bind(op)
    .fetch_one(&f.company.pool)
    .await
    .unwrap();
    assert_eq!(counts, (1, 0));
}

#[tokio::test]
#[ignore = "requires disposable Postgres"]
async fn canonical_source_is_checked_before_page_limit_and_on_replay() {
    let f = ApiFixture::new().await;
    let project = f.project().await;
    let visible = f
        .api
        .create_work_item(Uuid::new_v4(), f.item_input(project))
        .await
        .unwrap()
        .item;
    let foreign = f.source(f.hidden).await;
    let mut input = f.item_input(project);
    input.source_message_id = Some(foreign.to_hex());
    assert!(matches!(
        f.api.create_work_item(Uuid::new_v4(), input.clone()).await,
        Err(WorkError::SourceMessageNotDecided { .. })
    ));
    // The retained internal company repository can contain source references from
    // a different channel. Such newer rows must not consume an API page's LIMIT.
    let hidden = f
        .company
        .service
        .create_work_item(&f.company.scope, input, human())
        .await
        .unwrap()
        .item;
    let page = f
        .api
        .list_project_work(
            project,
            &WorkListQuery {
                limit: Some(1),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(page.items.len(), 1);
    assert_eq!(page.items[0].id, visible.item.id);
    assert!(page.next_cursor.is_none());
    assert!(matches!(
        f.api.work_item(hidden.item.id).await,
        Err(WorkError::WorkItemNotFound { .. })
    ));
    let source = f.source(f.channel).await;
    let mut input = f.item_input(project);
    input.source_message_id = Some(source.to_hex());
    let op = Uuid::new_v4();
    let item = f
        .api
        .create_work_item(op, input.clone())
        .await
        .unwrap()
        .item;
    sqlx::query("UPDATE events SET deleted_at=now() WHERE community_id=$1 AND id=$2")
        .bind(f.company.community_id)
        .bind(source.as_bytes().as_slice())
        .execute(&f.company.pool)
        .await
        .unwrap();
    assert!(matches!(
        f.api.create_work_item(op, input).await,
        Err(WorkError::SourceMessageNotDecided { .. })
    ));
    assert!(matches!(
        f.api.work_item(item.item.id).await,
        Err(WorkError::WorkItemNotFound { .. })
    ));
    assert!(matches!(
        f.api
            .mutate(
                Uuid::new_v4(),
                item.item.id,
                1,
                WorkMutation::Transition {
                    target: WorkState::Ready,
                    reason: None
                }
            )
            .await,
        Err(WorkError::WorkItemNotFound { .. })
    ));
}

#[path = "authorized/identity.rs"]
mod identity;

#[path = "authorized/receipts.rs"]
mod receipts;

#[path = "authorized/queue.rs"]
mod queue;
