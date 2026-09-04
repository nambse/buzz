//! Production-seam Postgres tests for the Work and Projects foundation.
//!
//! Run with a disposable local database that can receive the embedded
//! migrations:
//! `ORTAK_TEST_DATABASE_URL=postgres://... cargo test -p ortak-work -- --ignored`

use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use ortak_control::inbox::InboxEvent;
use ortak_control::ports::{CompanyDirectory, InboxRepository, RoutingRepository};
use ortak_control::routing::{
    CandidateRevision, RosterScope, RoutingCommitOutcome, RoutingProposal,
};
use ortak_control::{CompanyScope, MessageId, PgControlPlane};
use ortak_domain::{
    ApprovalDecision, ApprovalGateSpec, ApprovalStatus, AssignmentRole, AssignmentStatus,
    AttachmentRef, CriterionStatus, DomainError, EmployeeId, MessageOrigin, NewProject,
    NewWorkItem, ProjectSlug, ProjectStatus, RecipientAction, RecipientDecision, RoutingDecision,
    RoutingMode, RoutingPolicy, RoutingReason, WorkActor, WorkEvent, WorkPriority, WorkState,
};
use ortak_work::{
    AddDependency, ArchiveProject, AssignEmployee, AttachRecord, ResolveApproval, SatisfyCriterion,
    TransitionWorkItem, WorkError, WorkItemAggregate, WorkListQuery, WorkService,
};
use sha2::{Digest, Sha256};
use sqlx::{PgPool, Row};
use uuid::Uuid;

const DEFAULT_DATABASE_URL: &str = "postgres://ortak:ortak@127.0.0.1:55432/ortak"; // sadscan:disable np.postgres.1 -- local disposable test database

fn database_url() -> String {
    std::env::var("ORTAK_TEST_DATABASE_URL").unwrap_or_else(|_| DEFAULT_DATABASE_URL.to_owned())
}

async fn setup_pool() -> PgPool {
    let pool = PgPool::connect(&database_url())
        .await
        .expect("connect to test database");
    buzz_db::migration::run_migrations(&pool)
        .await
        .expect("apply migrations");
    pool
}

fn message_id() -> MessageId {
    let mut bytes = [0u8; 32];
    bytes[..16].copy_from_slice(Uuid::new_v4().as_bytes());
    bytes[16..].copy_from_slice(Uuid::new_v4().as_bytes());
    MessageId::from_bytes(bytes)
}

fn employee(value: &str) -> EmployeeId {
    EmployeeId::parse(value).expect("valid employee id")
}

fn human() -> WorkActor {
    WorkActor::Human("sefa".to_owned())
}

/// One disposable company with a community binding and three disposable
/// employee fixtures: `cem` (active), `zeynep` (active), `ada` (draft).
/// The ids are ordinary test fixtures, not the real adopted profiles.
struct Company {
    pool: PgPool,
    control: PgControlPlane,
    service: WorkService<PgControlPlane>,
    community_id: Uuid,
    scope: CompanyScope,
    cem_revision: Uuid,
}

impl Company {
    async fn new(pool: &PgPool) -> Self {
        let control = PgControlPlane::new(pool.clone());
        let community_id = Uuid::new_v4();
        sqlx::query("INSERT INTO communities (id, host) VALUES ($1, $2)")
            .bind(community_id)
            .bind(format!("ortak-work-{}.example", community_id.simple()))
            .execute(pool)
            .await
            .expect("insert community");
        let company_id: Uuid = sqlx::query(
            "INSERT INTO companies (slug, display_name, routing_policy)
             VALUES ($1, 'Ortak work test', $2) RETURNING id",
        )
        .bind(format!("co-{}", Uuid::new_v4().simple()))
        .bind(serde_json::to_value(RoutingPolicy::default()).expect("policy json"))
        .fetch_one(pool)
        .await
        .expect("insert company")
        .try_get("id")
        .expect("company id");
        sqlx::query(
            "INSERT INTO office_company_bindings (community_id, company_id) VALUES ($1, $2)",
        )
        .bind(community_id)
        .bind(company_id)
        .execute(pool)
        .await
        .expect("insert binding");

        let mut cem_revision = None;
        for (id, active) in [("cem", true), ("zeynep", true), ("ada", false)] {
            sqlx::query("INSERT INTO employees (company_id, id, status) VALUES ($1, $2, 'draft')")
                .bind(company_id)
                .bind(id)
                .execute(pool)
                .await
                .expect("insert employee");
            if active {
                let manifest = serde_json::json!({ "fixture": id, "routing": { "enabled": true } });
                let revision_id: Uuid = sqlx::query(
                    "INSERT INTO employee_revisions
                         (company_id, employee_id, revision_number, manifest,
                          manifest_fingerprint, provisioning_mode)
                     VALUES ($1, $2, 1, $3, $4, 'adopt') RETURNING id",
                )
                .bind(company_id)
                .bind(id)
                .bind(&manifest)
                .bind(Sha256::digest(manifest.to_string().as_bytes()).to_vec())
                .fetch_one(pool)
                .await
                .expect("insert revision")
                .try_get("id")
                .expect("revision id");
                sqlx::query(
                    "UPDATE employees SET status = 'active', active_revision_id = $3
                      WHERE company_id = $1 AND id = $2",
                )
                .bind(company_id)
                .bind(id)
                .bind(revision_id)
                .execute(pool)
                .await
                .expect("activate employee");
                if id == "cem" {
                    cem_revision = Some(revision_id);
                }
            }
        }

        let scope = control
            .resolve_company_for_community(community_id)
            .await
            .expect("resolve scope");
        assert_eq!(scope.company_id(), company_id);
        Self {
            pool: pool.clone(),
            service: WorkService::new(control.clone()),
            control,
            community_id,
            scope,
            cem_revision: cem_revision.expect("cem revision"),
        }
    }

    async fn project(&self, slug: &str) -> Uuid {
        let creation = self
            .service
            .create_project(
                &self.scope,
                NewProject {
                    slug: ProjectSlug::parse(slug).expect("slug"),
                    name: format!("Project {slug}"),
                    description: String::new(),
                },
                human(),
            )
            .await
            .expect("create project");
        assert!(creation.created);
        creation.project.project.id
    }

    async fn item(&self, project_id: Uuid, title: &str) -> WorkItemAggregate {
        self.service
            .create_work_item(
                &self.scope,
                NewWorkItem {
                    project_id,
                    title: title.to_owned(),
                    description: String::new(),
                    priority: WorkPriority::Normal,
                    criteria: Vec::new(),
                    approvals: Vec::new(),
                    source_message_id: None,
                },
                human(),
            )
            .await
            .expect("create work item")
            .item
    }

    /// Stores an inbox row for a message and routes it to Cem through the
    /// production routing commit, leaving the inbox row `decided` with a
    /// decision that woke Cem.
    async fn decided_message(&self) -> (MessageId, Uuid) {
        self.routed_message(RecipientAction::Wake).await
    }

    /// Stores an inbox row for a message and commits a silent decision
    /// (Cem dropped, nobody woken) through the production routing commit,
    /// leaving the inbox row `decided` with a decision that dispatched
    /// nothing.
    async fn silently_decided_message(&self) -> (MessageId, Uuid) {
        self.routed_message(RecipientAction::Drop).await
    }

    async fn routed_message(&self, action: RecipientAction) -> (MessageId, Uuid) {
        let id = message_id();
        let (mode, reason) = match action {
            RecipientAction::Wake => (
                RoutingMode::Deterministic,
                RoutingReason::StructuredDispatch,
            ),
            RecipientAction::Drop => (RoutingMode::Silent, RoutingReason::NonRoutableMessage),
        };
        self.control
            .insert_accepted_event(
                self.community_id,
                &InboxEvent {
                    event_id: id,
                    event_created_at: Utc::now(),
                    event_kind: 1,
                    author_pubkey: [7; 32],
                    channel_id: Some(Uuid::new_v4()),
                },
            )
            .await
            .expect("insert inbox row");
        let claim = self
            .control
            .claim_message(&self.scope, id, "router", Duration::from_secs(60), 5)
            .await
            .expect("claim")
            .expect("claimable");
        let policy = RoutingPolicy::default();
        let proposal = RoutingProposal {
            company_id: self.scope.company_id(),
            message_id: id,
            root_message_id: id,
            claim_generation: claim.claim_generation,
            origin: MessageOrigin::Human("sefa".to_owned()),
            input_hash: [3; 32],
            candidates: vec![CandidateRevision {
                employee_id: employee("cem"),
                revision_id: self.cem_revision,
            }],
            roster_scope: RosterScope::Targets,
            decision: RoutingDecision {
                message_id: id.to_hex(),
                mode,
                summary_reason: reason,
                policy_version: policy.version.clone(),
                policy_fingerprint: policy.fingerprint(),
                recipients: vec![RecipientDecision {
                    employee_id: employee("cem"),
                    action,
                    reason,
                    score: None,
                    evidence: Vec::new(),
                }],
            },
            scorer: None,
        };
        match self
            .control
            .commit_routing(&self.scope, &proposal)
            .await
            .expect("commit routing")
        {
            RoutingCommitOutcome::Committed(decision) => (id, decision.decision_id),
            other => panic!("expected a committed decision, got {other:?}"),
        }
    }

    /// A pending (never decided) inbox row.
    async fn pending_message(&self) -> MessageId {
        let id = message_id();
        self.control
            .insert_accepted_event(
                self.community_id,
                &InboxEvent {
                    event_id: id,
                    event_created_at: Utc::now(),
                    event_kind: 1,
                    author_pubkey: [7; 32],
                    channel_id: None,
                },
            )
            .await
            .expect("insert inbox row");
        id
    }

    async fn transition(
        &self,
        item: &WorkItemAggregate,
        target: WorkState,
    ) -> ortak_work::Result<WorkItemAggregate> {
        self.service
            .transition_work_item(
                &self.scope,
                TransitionWorkItem {
                    work_item_id: item.item.id,
                    expected_version: item.item.version,
                    target,
                    reason: None,
                    actor: human(),
                },
            )
            .await
    }

    async fn history_rows(&self, work_item_id: Uuid) -> Vec<(i64, i64, String)> {
        sqlx::query(
            "SELECT sequence, version, event_type FROM work_item_history
              WHERE company_id = $1 AND work_item_id = $2 ORDER BY sequence",
        )
        .bind(self.scope.company_id())
        .bind(work_item_id)
        .fetch_all(&self.pool)
        .await
        .expect("history rows")
        .iter()
        .map(|row| {
            (
                row.try_get("sequence").expect("sequence"),
                row.try_get("version").expect("version"),
                row.try_get("event_type").expect("event_type"),
            )
        })
        .collect()
    }
}

fn assert_dense_history(aggregate: &WorkItemAggregate) {
    assert_eq!(
        aggregate.history.len() as i64,
        aggregate.item.version,
        "one history row per version"
    );
    for (index, record) in aggregate.history.iter().enumerate() {
        assert_eq!(record.sequence, index as i64);
        assert_eq!(record.version, record.sequence + 1);
    }
    assert!(!aggregate.history_truncated);
}

#[tokio::test]
#[ignore = "requires a disposable PostgreSQL database"]
async fn promotion_is_idempotent_and_derives_its_source_from_the_decided_inbox() {
    let pool = setup_pool().await;
    let company = Company::new(&pool).await;
    let project_id = company.project("fitness-app").await;

    // Project creation is idempotent by slug and detects a different name.
    let replay = company
        .service
        .create_project(
            &company.scope,
            NewProject {
                slug: ProjectSlug::parse("fitness-app").expect("slug"),
                name: "Project fitness-app".to_owned(),
                description: String::new(),
            },
            human(),
        )
        .await
        .expect("replay project creation");
    assert!(!replay.created);
    assert_eq!(replay.project.project.id, project_id);
    let conflict = company
        .service
        .create_project(
            &company.scope,
            NewProject {
                slug: ProjectSlug::parse("fitness-app").expect("slug"),
                name: "Something else".to_owned(),
                description: String::new(),
            },
            human(),
        )
        .await;
    assert!(matches!(conflict, Err(WorkError::ProjectConflict { .. })));

    let (message, decision_id) = company.decided_message().await;
    let promote = |title: &str| {
        company.service.promote_message(
            &company.scope,
            project_id,
            message,
            title.to_owned(),
            "Promote the onboarding thread".to_owned(),
            WorkPriority::High,
            vec!["Onboarding flow shipped".to_owned()],
            vec![ApprovalGateSpec {
                gate: "human_review".to_owned(),
                required: true,
            }],
            WorkActor::Employee(employee("cem")),
        )
    };
    let first = promote("Ship onboarding").await.expect("promote");
    assert!(first.created);
    let item = &first.item.item;
    assert_eq!(item.state, WorkState::Proposed);
    assert_eq!(item.version, 1);
    assert_eq!(
        item.source_message_id.as_deref(),
        Some(message.to_hex().as_str())
    );
    assert_eq!(first.item.source_routing_decision_id, Some(decision_id));
    assert_eq!(item.criteria.len(), 1);
    assert_eq!(item.approvals.len(), 1);
    assert_eq!(
        item.attachments
            .iter()
            .map(|attachment| attachment.reference.clone())
            .collect::<Vec<_>>(),
        vec![
            AttachmentRef::OfficeMessage {
                message_id: message.to_hex()
            },
            AttachmentRef::RoutingDecision { decision_id },
        ],
        "source message and its decision are attached from authoritative rows"
    );
    assert_eq!(first.item.created_by, WorkActor::Employee(employee("cem")));
    assert_dense_history(&first.item);
    assert!(matches!(
        first.item.history[0].event,
        WorkEvent::Created { .. }
    ));

    // A replay with the same definition returns the same item and writes
    // nothing: same version, same single history row.
    let second = promote("Ship onboarding").await.expect("replay promotion");
    assert!(!second.created);
    assert_eq!(second.item.item.id, item.id);
    assert_eq!(second.item.item.title, "Ship onboarding");
    assert_eq!(second.item.item.version, 1);
    assert_eq!(company.history_rows(item.id).await.len(), 1);

    // A replay that changes the immutable creation definition is not a
    // replay: it is refused with a bounded conflict naming the existing
    // item, and still writes nothing.
    let retitled = promote("Different title").await;
    assert!(matches!(
        retitled,
        Err(WorkError::PromotionConflict { work_item_id, ref message_id })
            if work_item_id == item.id && *message_id == message.to_hex()
    ));
    let other_project = company.project("fitness-app-v2").await;
    let moved = company
        .service
        .promote_message(
            &company.scope,
            other_project,
            message,
            "Ship onboarding".to_owned(),
            "Promote the onboarding thread".to_owned(),
            WorkPriority::High,
            vec!["Onboarding flow shipped".to_owned()],
            vec![ApprovalGateSpec {
                gate: "human_review".to_owned(),
                required: true,
            }],
            WorkActor::Employee(employee("cem")),
        )
        .await;
    assert!(matches!(
        moved,
        Err(WorkError::PromotionConflict { work_item_id, .. }) if work_item_id == item.id
    ));
    let regated = company
        .service
        .promote_message(
            &company.scope,
            project_id,
            message,
            "Ship onboarding".to_owned(),
            "Promote the onboarding thread".to_owned(),
            WorkPriority::High,
            vec!["Onboarding flow shipped".to_owned()],
            vec![ApprovalGateSpec {
                gate: "human_review".to_owned(),
                required: false,
            }],
            WorkActor::Employee(employee("cem")),
        )
        .await;
    assert!(matches!(
        regated,
        Err(WorkError::PromotionConflict { work_item_id, .. }) if work_item_id == item.id
    ));
    assert_eq!(company.history_rows(item.id).await.len(), 1);
    let current = company
        .service
        .work_item(&company.scope, item.id)
        .await
        .expect("read promoted item");
    assert_eq!(current.item.version, 1);
    assert_eq!(current.item.project_id, project_id);

    // A pending (undecided) message and an unknown message both fail closed.
    let pending = company.pending_message().await;
    let refused = company
        .service
        .promote_message(
            &company.scope,
            project_id,
            pending,
            "Too early".to_owned(),
            String::new(),
            WorkPriority::Normal,
            Vec::new(),
            Vec::new(),
            human(),
        )
        .await;
    assert!(matches!(
        refused,
        Err(WorkError::SourceMessageNotDecided { .. })
    ));
    let unknown = company
        .service
        .promote_message(
            &company.scope,
            project_id,
            message_id(),
            "Unknown".to_owned(),
            String::new(),
            WorkPriority::Normal,
            Vec::new(),
            Vec::new(),
            human(),
        )
        .await;
    assert!(matches!(
        unknown,
        Err(WorkError::SourceMessageNotDecided { .. })
    ));

    // A decided message of another company is not visible here.
    let other = Company::new(&pool).await;
    let (foreign, _) = other.decided_message().await;
    let cross = company
        .service
        .promote_message(
            &company.scope,
            project_id,
            foreign,
            "Foreign".to_owned(),
            String::new(),
            WorkPriority::Normal,
            Vec::new(),
            Vec::new(),
            human(),
        )
        .await;
    assert!(matches!(
        cross,
        Err(WorkError::SourceMessageNotDecided { .. })
    ));

    // The project list sees the promoted item in keyset order.
    let older = company.item(project_id, "Older manual item").await;
    let page = company
        .service
        .list_project_work(
            &company.scope,
            project_id,
            &WorkListQuery {
                limit: Some(1),
                ..WorkListQuery::default()
            },
        )
        .await
        .expect("list page 1");
    assert_eq!(page.items.len(), 1);
    assert_eq!(page.items[0].id, older.item.id, "newest first");
    let cursor = page.next_cursor.expect("second page exists");
    let page = company
        .service
        .list_project_work(
            &company.scope,
            project_id,
            &WorkListQuery {
                limit: Some(1),
                cursor: Some(cursor),
                ..WorkListQuery::default()
            },
        )
        .await
        .expect("list page 2");
    assert_eq!(page.items.len(), 1);
    assert_eq!(page.items[0].id, item.id);
    assert!(page.next_cursor.is_none());
    let filtered = company
        .service
        .list_project_work(
            &company.scope,
            project_id,
            &WorkListQuery {
                states: vec![WorkState::Completed],
                ..WorkListQuery::default()
            },
        )
        .await
        .expect("filtered list");
    assert!(filtered.items.is_empty());
}

#[tokio::test]
#[ignore = "requires a disposable PostgreSQL database"]
async fn assignment_requires_an_active_employee_and_scope_isolates_tenants() {
    let pool = setup_pool().await;
    let company = Company::new(&pool).await;
    let other = Company::new(&pool).await;
    let project_id = company.project("mobile").await;
    let item = company.item(project_id, "Assignable").await;

    let assign = |employee_id: &str, expected_version: i64| {
        company.service.assign_employee(
            &company.scope,
            AssignEmployee {
                work_item_id: item.item.id,
                expected_version,
                employee_id: employee(employee_id),
                role: AssignmentRole::Owner,
                actor: human(),
            },
        )
    };

    // Draft and unknown employees are refused; nothing is written.
    let draft = assign("ada", 1).await;
    assert!(matches!(
        draft,
        Err(WorkError::EmployeeNotAssignable { ref employee_id }) if employee_id.as_str() == "ada"
    ));
    let unknown = assign("nobody", 1).await;
    assert!(matches!(
        unknown,
        Err(WorkError::EmployeeNotAssignable { .. })
    ));
    assert_eq!(company.history_rows(item.item.id).await.len(), 1);

    // An active employee is assigned once; a duplicate active assignment
    // is refused by the domain.
    let assigned = assign("cem", 1).await.expect("assign cem");
    assert_eq!(assigned.item.version, 2);
    assert_eq!(assigned.item.assignments.len(), 1);
    assert_eq!(assigned.item.assignments[0].employee_id, employee("cem"));
    assert_eq!(
        assigned.item.assignments[0].status,
        AssignmentStatus::Active
    );
    assert!(matches!(
        assigned.history.last().map(|record| &record.event),
        Some(WorkEvent::Assigned { .. })
    ));
    let duplicate = assign("cem", 2).await;
    assert!(matches!(
        duplicate,
        Err(WorkError::Domain(DomainError::DuplicateAssignment))
    ));

    // An employee actor must be active in this company.
    let draft_actor = company
        .service
        .assign_employee(
            &company.scope,
            AssignEmployee {
                work_item_id: item.item.id,
                expected_version: 2,
                employee_id: employee("zeynep"),
                role: AssignmentRole::Reviewer,
                actor: WorkActor::Employee(employee("ada")),
            },
        )
        .await;
    assert!(matches!(draft_actor, Err(WorkError::ActorNotFound { .. })));

    // Another company's scope cannot see, mutate, list, or depend on it.
    let foreign_read = other.service.work_item(&other.scope, item.item.id).await;
    assert!(matches!(
        foreign_read,
        Err(WorkError::WorkItemNotFound { work_item_id }) if work_item_id == item.item.id
    ));
    let foreign_assign = other
        .service
        .assign_employee(
            &other.scope,
            AssignEmployee {
                work_item_id: item.item.id,
                expected_version: 2,
                employee_id: employee("cem"),
                role: AssignmentRole::Contributor,
                actor: human(),
            },
        )
        .await;
    assert!(matches!(
        foreign_assign,
        Err(WorkError::WorkItemNotFound { .. })
    ));
    let foreign_list = other
        .service
        .list_project_work(&other.scope, project_id, &WorkListQuery::default())
        .await;
    assert!(matches!(
        foreign_list,
        Err(WorkError::ProjectNotFound { .. })
    ));
    let foreign_project = other.project("mobile").await;
    let foreign_item = other.item(foreign_project, "Theirs").await;
    let cross_dependency = company
        .service
        .add_dependency(
            &company.scope,
            AddDependency {
                work_item_id: item.item.id,
                expected_version: 2,
                depends_on: foreign_item.item.id,
                actor: human(),
            },
        )
        .await;
    assert!(matches!(
        cross_dependency,
        Err(WorkError::WorkItemNotFound { work_item_id }) if work_item_id == foreign_item.item.id
    ));
    let cross_attachment = company
        .service
        .attach_record(
            &company.scope,
            AttachRecord {
                work_item_id: item.item.id,
                expected_version: 2,
                reference: AttachmentRef::RoutingDecision {
                    decision_id: other.decided_message().await.1,
                },
                label: None,
                actor: human(),
            },
        )
        .await;
    assert!(matches!(
        cross_attachment,
        Err(WorkError::AttachmentTargetNotFound {
            kind: "routing_decision"
        })
    ));

    // Nothing above changed the item.
    let current = company
        .service
        .work_item(&company.scope, item.item.id)
        .await
        .expect("read item");
    assert_eq!(current.item.version, 2);
    assert_dense_history(&current);

    // Archiving the project freezes its work but keeps it readable.
    let archived = company
        .service
        .archive_project(
            &company.scope,
            ArchiveProject {
                project_id,
                expected_version: 1,
                reason: Some("wrapped up".to_owned()),
                actor: human(),
            },
        )
        .await
        .expect("archive project");
    assert_eq!(archived.project.status, ProjectStatus::Archived);
    assert_eq!(archived.project.version, 2);
    let frozen = company.transition(&current, WorkState::Ready).await;
    assert!(matches!(frozen, Err(WorkError::ProjectArchived { .. })));
    let still_there = company
        .service
        .work_item(&company.scope, item.item.id)
        .await
        .expect("read archived work");
    assert_eq!(still_there.item.version, 2);
    let new_work = company
        .service
        .create_work_item(
            &company.scope,
            NewWorkItem {
                project_id,
                title: "Late".to_owned(),
                description: String::new(),
                priority: WorkPriority::Normal,
                criteria: Vec::new(),
                approvals: Vec::new(),
                source_message_id: None,
            },
            human(),
        )
        .await;
    assert!(matches!(new_work, Err(WorkError::ProjectArchived { .. })));
}

#[tokio::test]
#[ignore = "requires a disposable PostgreSQL database"]
async fn dependency_cycles_cross_project_edges_and_stale_versions_are_rejected() {
    let pool = setup_pool().await;
    let company = Company::new(&pool).await;
    let project_id = company.project("platform").await;
    let a = company.item(project_id, "A").await;
    let b = company.item(project_id, "B").await;
    let c = company.item(project_id, "C").await;
    let other_project = company.project("elsewhere").await;
    let elsewhere = company.item(other_project, "Elsewhere").await;

    let depend = |item: &WorkItemAggregate, on: Uuid, expected_version: i64| {
        company.service.add_dependency(
            &company.scope,
            AddDependency {
                work_item_id: item.item.id,
                expected_version,
                depends_on: on,
                actor: human(),
            },
        )
    };

    // Self-dependency is refused before any transaction (and by the check).
    let self_edge = depend(&a, a.item.id, 1).await;
    assert!(matches!(
        self_edge,
        Err(WorkError::Domain(DomainError::SelfDependency))
    ));
    let structural = sqlx::query(
        "INSERT INTO work_dependencies
             (company_id, project_id, work_item_id, depends_on_work_item_id,
              created_by_type, created_by_id)
         VALUES ($1, $2, $3, $3, 'human', 'test')",
    )
    .bind(company.scope.company_id())
    .bind(project_id)
    .bind(a.item.id)
    .execute(&pool)
    .await;
    assert!(
        structural.is_err(),
        "the check constraint refuses a self edge"
    );

    // A -> B -> C, then C -> A closes a cycle.
    let a = depend(&a, b.item.id, 1).await.expect("a depends on b");
    assert_eq!(a.item.dependencies.len(), 1);
    let b = depend(&b, c.item.id, 1).await.expect("b depends on c");
    let cycle = depend(&c, a.item.id, 1).await;
    assert!(matches!(
        cycle,
        Err(WorkError::Domain(DomainError::DependencyCycle))
    ));
    assert_eq!(company.history_rows(c.item.id).await.len(), 1);
    let shortcut = depend(&a, c.item.id, 2)
        .await
        .expect("a -> c is not a cycle");
    assert_eq!(shortcut.item.dependencies.len(), 2);

    // Cross-project edges are refused even inside the company.
    let cross = depend(&b, elsewhere.item.id, 2).await;
    assert!(matches!(
        cross,
        Err(WorkError::CrossProjectDependency { depends_on }) if depends_on == elsewhere.item.id
    ));

    // Unfinished dependencies block the start of work; a completed one
    // does not.
    let b = company
        .transition(&b, WorkState::Ready)
        .await
        .expect("b ready");
    let blocked = company.transition(&b, WorkState::InProgress).await;
    assert!(matches!(
        blocked,
        Err(WorkError::Domain(DomainError::DependenciesUnresolved {
            count: 1
        }))
    ));
    let c = company
        .service
        .work_item(&company.scope, c.item.id)
        .await
        .expect("read c");
    let mut c = company
        .transition(&c, WorkState::Ready)
        .await
        .expect("c ready");
    for target in [
        WorkState::InProgress,
        WorkState::Review,
        WorkState::Completed,
    ] {
        c = company.transition(&c, target).await.expect("c advances");
    }
    assert_eq!(c.item.state, WorkState::Completed);
    let started = company
        .transition(&b, WorkState::InProgress)
        .await
        .expect("b starts");
    assert_eq!(started.item.state, WorkState::InProgress);

    // Optimistic concurrency: two writers holding the same version; exactly
    // one wins and the loser sees the refreshed version.
    let stale_version = started.item.version;
    let first = company.transition(&started, WorkState::Blocked).await;
    let second = company.transition(&started, WorkState::Review).await;
    assert!(first.is_ok());
    assert!(matches!(
        second,
        Err(WorkError::VersionConflict { expected, actual, .. })
            if expected == stale_version && actual == stale_version + 1
    ));
    let (winner, loser) = tokio::join!(
        company.service.transition_work_item(
            &company.scope,
            TransitionWorkItem {
                work_item_id: b.item.id,
                expected_version: stale_version + 1,
                target: WorkState::Ready,
                reason: None,
                actor: human(),
            },
        ),
        company.service.transition_work_item(
            &company.scope,
            TransitionWorkItem {
                work_item_id: b.item.id,
                expected_version: stale_version + 1,
                target: WorkState::Cancelled,
                reason: Some("racing".to_owned()),
                actor: human(),
            },
        ),
    );
    assert_eq!(
        winner.is_ok() as u8 + loser.is_ok() as u8,
        1,
        "exactly one concurrent writer commits"
    );
    let current = company
        .service
        .work_item(&company.scope, b.item.id)
        .await
        .expect("read b");
    assert_eq!(current.item.version, stale_version + 2);
    assert_dense_history(&current);

    // The row guard refuses a version step other than +1 even for direct SQL.
    let skip = sqlx::query(
        "UPDATE work_items SET version = version + 2 WHERE company_id = $1 AND id = $2",
    )
    .bind(company.scope.company_id())
    .bind(a.item.id)
    .execute(&pool)
    .await;
    assert!(skip.is_err(), "work_items_guard refuses a version jump");
}

#[tokio::test]
#[ignore = "requires a disposable PostgreSQL database"]
async fn completion_requires_gates_and_history_is_atomic_and_immutable() {
    let pool = setup_pool().await;
    let company = Company::new(&pool).await;
    let project_id = company.project("release").await;
    let created = company
        .service
        .create_work_item(
            &company.scope,
            NewWorkItem {
                project_id,
                title: "Release 1.0".to_owned(),
                description: "Ship it".to_owned(),
                priority: WorkPriority::Urgent,
                criteria: vec!["Tests green".to_owned(), "Changelog written".to_owned()],
                approvals: vec![
                    ApprovalGateSpec {
                        gate: "human_review".to_owned(),
                        required: true,
                    },
                    ApprovalGateSpec {
                        gate: "security".to_owned(),
                        required: false,
                    },
                ],
                source_message_id: None,
            },
            human(),
        )
        .await
        .expect("create")
        .item;

    let mut item = created;
    for target in [WorkState::Ready, WorkState::InProgress, WorkState::Review] {
        item = company.transition(&item, target).await.expect("advance");
    }
    assert_eq!(item.item.version, 4);
    let cem = WorkActor::Employee(employee("cem"));

    // Completion is refused while a criterion or a required approval is
    // pending: no state change, no version bump, no history row.
    let refused = company.transition(&item, WorkState::Completed).await;
    let Err(WorkError::Domain(DomainError::CompletionBlocked { blockers })) = refused else {
        panic!("expected completion to be blocked, got {refused:?}");
    };
    assert_eq!(blockers.len(), 3);
    let unchanged = company
        .service
        .work_item(&company.scope, item.item.id)
        .await
        .expect("read");
    assert_eq!(unchanged.item.state, WorkState::Review);
    assert_eq!(unchanged.item.version, 4);
    assert_eq!(company.history_rows(item.item.id).await.len(), 4);

    // Satisfy both criteria (an unknown criterion is refused) and approve
    // the required gate; the optional gate never blocks.
    let unknown = company
        .service
        .satisfy_criterion(
            &company.scope,
            SatisfyCriterion {
                work_item_id: item.item.id,
                expected_version: 4,
                criterion_id: Uuid::new_v4(),
                actor: cem.clone(),
            },
        )
        .await;
    assert!(matches!(
        unknown,
        Err(WorkError::Domain(DomainError::UnknownCriterion))
    ));
    let criteria: Vec<Uuid> = item
        .item
        .criteria
        .iter()
        .map(|criterion| criterion.id)
        .collect();
    for criterion_id in criteria {
        item = company
            .service
            .satisfy_criterion(
                &company.scope,
                SatisfyCriterion {
                    work_item_id: item.item.id,
                    expected_version: item.item.version,
                    criterion_id,
                    actor: cem.clone(),
                },
            )
            .await
            .expect("satisfy");
    }
    assert!(item
        .item
        .criteria
        .iter()
        .all(|criterion| criterion.satisfied_by == Some(cem.clone())));
    let still_blocked = company.transition(&item, WorkState::Completed).await;
    assert!(matches!(
        still_blocked,
        Err(WorkError::Domain(DomainError::CompletionBlocked { ref blockers })) if blockers.len() == 1
    ));
    let required = item
        .item
        .approvals
        .iter()
        .find(|gate| gate.gate == "human_review")
        .expect("required gate")
        .id;
    item = company
        .service
        .resolve_approval(
            &company.scope,
            ResolveApproval {
                work_item_id: item.item.id,
                expected_version: item.item.version,
                approval_id: required,
                decision: ApprovalDecision::Approve,
                reason: Some("looks good".to_owned()),
                actor: human(),
            },
        )
        .await
        .expect("approve");
    let again = company
        .service
        .resolve_approval(
            &company.scope,
            ResolveApproval {
                work_item_id: item.item.id,
                expected_version: item.item.version,
                approval_id: required,
                decision: ApprovalDecision::Reject,
                reason: None,
                actor: human(),
            },
        )
        .await;
    assert!(matches!(
        again,
        Err(WorkError::Domain(DomainError::ApprovalAlreadyResolved))
    ));
    assert_eq!(
        item.item
            .approvals
            .iter()
            .find(|gate| gate.gate == "security")
            .map(|gate| gate.status),
        Some(ApprovalStatus::Pending)
    );

    // Now completion succeeds with exactly one more history row, and the
    // terminal item refuses every further command.
    let completed = company
        .transition(&item, WorkState::Completed)
        .await
        .expect("complete");
    assert_eq!(completed.item.state, WorkState::Completed);
    assert!(completed.completed_at.is_some());
    assert_eq!(completed.item.version, 8);
    assert_dense_history(&completed);
    let rows = company.history_rows(completed.item.id).await;
    assert_eq!(
        rows.iter()
            .map(|(_, _, event)| event.as_str())
            .collect::<Vec<_>>(),
        vec![
            "work.created",
            "work.state_changed",
            "work.state_changed",
            "work.state_changed",
            "work.criterion_satisfied",
            "work.criterion_satisfied",
            "work.approval_resolved",
            "work.state_changed",
        ]
    );
    let reopen = company.transition(&completed, WorkState::InProgress).await;
    assert!(matches!(
        reopen,
        Err(WorkError::Domain(DomainError::WorkItemTerminal {
            state: WorkState::Completed
        }))
    ));
    let late_assign = company
        .service
        .assign_employee(
            &company.scope,
            AssignEmployee {
                work_item_id: completed.item.id,
                expected_version: 8,
                employee_id: employee("zeynep"),
                role: AssignmentRole::Reviewer,
                actor: human(),
            },
        )
        .await;
    assert!(matches!(
        late_assign,
        Err(WorkError::Domain(DomainError::WorkItemTerminal { .. }))
    ));

    // History cannot be rewritten or erased, and a cancelled item keeps
    // everything that happened before.
    let rewrite = sqlx::query(
        "UPDATE work_item_history SET event_type = 'work.forged'
          WHERE company_id = $1 AND work_item_id = $2 AND sequence = 0",
    )
    .bind(company.scope.company_id())
    .bind(completed.item.id)
    .execute(&pool)
    .await;
    assert!(rewrite.is_err(), "history rows are immutable");
    let erase =
        sqlx::query("DELETE FROM work_item_history WHERE company_id = $1 AND work_item_id = $2")
            .bind(company.scope.company_id())
            .bind(completed.item.id)
            .execute(&pool)
            .await;
    assert!(erase.is_err(), "history rows cannot be deleted");
    let gap = sqlx::query(
        "INSERT INTO work_item_history
             (company_id, work_item_id, sequence, version, event_type, actor_type, actor_id, payload)
         VALUES ($1, $2, 10, 11, 'work.state_changed', 'human', 'x', '{}'::jsonb)",
    )
    .bind(company.scope.company_id())
    .bind(completed.item.id)
    .execute(&pool)
    .await;
    assert!(gap.is_err(), "history sequences are dense");

    let doomed = company.item(project_id, "Doomed").await;
    let doomed = company
        .service
        .assign_employee(
            &company.scope,
            AssignEmployee {
                work_item_id: doomed.item.id,
                expected_version: 1,
                employee_id: employee("zeynep"),
                role: AssignmentRole::Owner,
                actor: human(),
            },
        )
        .await
        .expect("assign");
    let cancelled = company
        .service
        .transition_work_item(
            &company.scope,
            TransitionWorkItem {
                work_item_id: doomed.item.id,
                expected_version: doomed.item.version,
                target: WorkState::Cancelled,
                reason: Some("descoped".to_owned()),
                actor: human(),
            },
        )
        .await
        .expect("cancel");
    assert_eq!(cancelled.item.state, WorkState::Cancelled);
    assert!(cancelled.cancelled_at.is_some());
    assert_eq!(
        cancelled.item.assignments.len(),
        1,
        "assignments survive cancellation"
    );
    assert_eq!(cancelled.history.len(), 3);
    assert!(matches!(
        cancelled.history[2].event,
        WorkEvent::StateChanged {
            from: WorkState::Ready | WorkState::Proposed,
            to: WorkState::Cancelled,
            ..
        }
    ));
    let delete_item = sqlx::query("DELETE FROM work_items WHERE company_id = $1 AND id = $2")
        .bind(company.scope.company_id())
        .bind(cancelled.item.id)
        .execute(&pool)
        .await;
    assert!(delete_item.is_err(), "work items are never deleted");
}

#[tokio::test]
#[ignore = "requires a disposable PostgreSQL database"]
async fn promotion_replay_survives_project_archive_and_silent_decisions_carry_no_provenance() {
    let pool = setup_pool().await;
    let company = Company::new(&pool).await;
    let project_id = company.project("archived-later").await;
    let (message, decision_id) = company.decided_message().await;
    let promote = |project_id: Uuid, description: &str| {
        company.service.promote_message(
            &company.scope,
            project_id,
            message,
            "Promoted before the archive".to_owned(),
            description.to_owned(),
            WorkPriority::Normal,
            Vec::new(),
            Vec::new(),
            human(),
        )
    };
    let first = promote(project_id, "original").await.expect("promote");
    assert!(first.created);
    assert_eq!(first.item.source_routing_decision_id, Some(decision_id));
    let item_id = first.item.item.id;

    let project = company
        .service
        .project(&company.scope, project_id)
        .await
        .expect("read project");
    let archived = company
        .service
        .archive_project(
            &company.scope,
            ArchiveProject {
                project_id,
                expected_version: project.project.version,
                reason: None,
                actor: human(),
            },
        )
        .await
        .expect("archive project");
    assert_eq!(archived.project.status, ProjectStatus::Archived);

    // The message was promoted before the archive, so a replay with the
    // same definition still answers with the existing item instead of
    // refusing the archived project, and writes nothing.
    let replay = promote(project_id, "original")
        .await
        .expect("replay after the project was archived");
    assert!(!replay.created);
    assert_eq!(replay.item.item.id, item_id);
    assert_eq!(replay.item.item.version, 1);
    assert_eq!(company.history_rows(item_id).await.len(), 1);

    // A different definition is a conflict, not an archive refusal, and a
    // live project named for an already-promoted message is a conflict too.
    let rewritten = promote(project_id, "rewritten").await;
    assert!(matches!(
        rewritten,
        Err(WorkError::PromotionConflict { work_item_id, .. }) if work_item_id == item_id
    ));
    let live_project = company.project("still-live").await;
    let moved = promote(live_project, "original").await;
    assert!(matches!(
        moved,
        Err(WorkError::PromotionConflict { work_item_id, .. }) if work_item_id == item_id
    ));

    // The archive fence still holds for a message that was never promoted.
    let (fresh, _) = company.decided_message().await;
    let refused = company
        .service
        .promote_message(
            &company.scope,
            project_id,
            fresh,
            "Too late".to_owned(),
            String::new(),
            WorkPriority::Normal,
            Vec::new(),
            Vec::new(),
            human(),
        )
        .await;
    assert!(matches!(
        refused,
        Err(WorkError::ProjectArchived { project_id: refused_project }) if refused_project == project_id
    ));
    let promoted_rows: i64 = sqlx::query(
        "SELECT count(*) FROM work_items WHERE company_id = $1 AND source_message_id = $2",
    )
    .bind(company.scope.company_id())
    .bind(message.as_bytes().as_slice())
    .fetch_one(&pool)
    .await
    .expect("count promoted rows")
    .try_get(0)
    .expect("count");
    assert_eq!(promoted_rows, 1, "a replay never creates a second item");

    // A silent decision leaves the inbox row decided, so the message can be
    // promoted, but it dispatched nothing and is not dispatch provenance:
    // the decision id is absent and only the message is attached.
    let (silent, silent_decision) = company.silently_decided_message().await;
    let wake_count: i32 =
        sqlx::query("SELECT wake_count FROM routing_decisions WHERE company_id = $1 AND id = $2")
            .bind(company.scope.company_id())
            .bind(silent_decision)
            .fetch_one(&pool)
            .await
            .expect("read the silent decision")
            .try_get("wake_count")
            .expect("wake_count");
    assert_eq!(wake_count, 0, "the fixture decision must wake nobody");
    let promote_silent = || {
        company.service.promote_message(
            &company.scope,
            live_project,
            silent,
            "Promoted from a silent decision".to_owned(),
            String::new(),
            WorkPriority::Normal,
            Vec::new(),
            Vec::new(),
            human(),
        )
    };
    let quiet = promote_silent()
        .await
        .expect("promote a silently decided message");
    assert!(quiet.created);
    assert_eq!(
        quiet.item.item.source_message_id.as_deref(),
        Some(silent.to_hex().as_str())
    );
    assert_eq!(quiet.item.source_routing_decision_id, None);
    assert_eq!(
        quiet
            .item
            .item
            .attachments
            .iter()
            .map(|attachment| attachment.reference.clone())
            .collect::<Vec<_>>(),
        vec![AttachmentRef::OfficeMessage {
            message_id: silent.to_hex()
        }],
        "no routing decision attachment for a decision that woke nobody"
    );
    let stored_decision: Option<Uuid> = sqlx::query(
        "SELECT source_routing_decision_id FROM work_items WHERE company_id = $1 AND id = $2",
    )
    .bind(company.scope.company_id())
    .bind(quiet.item.item.id)
    .fetch_one(&pool)
    .await
    .expect("read the promoted row")
    .try_get("source_routing_decision_id")
    .expect("source_routing_decision_id");
    assert_eq!(stored_decision, None);
    let quiet_replay = promote_silent().await.expect("replay the silent promotion");
    assert!(!quiet_replay.created);
    assert_eq!(quiet_replay.item.item.id, quiet.item.item.id);
    assert_eq!(quiet_replay.item.source_routing_decision_id, None);
}

/// Rounds of racing mutations in the lock-order regression.
const LOCK_ORDER_ROUNDS: usize = 24;
/// Attempts one mutation may spend losing the version race before the
/// regression fails; a deadlock never reaches this bound because Postgres
/// reports it as an error, which fails the lane immediately.
const LOCK_ORDER_MAX_ATTEMPTS: usize = 64;

enum RacingMutation {
    /// Dependency-graph mutation: locks the project row `FOR UPDATE`.
    Depend(Uuid),
    /// Ordinary item mutation: locks the project row `FOR SHARE`.
    Satisfy(Uuid),
}

/// Commits one mutation through the production service, re-reading the
/// current version after every lost version race. Any other failure,
/// including a Postgres deadlock surfacing as a database error, fails the
/// test on the spot.
async fn commit_racing_mutation(
    company: &Company,
    work_item_id: Uuid,
    mutation: RacingMutation,
) -> WorkItemAggregate {
    for _ in 0..LOCK_ORDER_MAX_ATTEMPTS {
        let expected_version = company
            .service
            .work_item(&company.scope, work_item_id)
            .await
            .expect("read the current version")
            .item
            .version;
        let result = match &mutation {
            RacingMutation::Depend(depends_on) => {
                company
                    .service
                    .add_dependency(
                        &company.scope,
                        AddDependency {
                            work_item_id,
                            expected_version,
                            depends_on: *depends_on,
                            actor: human(),
                        },
                    )
                    .await
            }
            RacingMutation::Satisfy(criterion_id) => {
                company
                    .service
                    .satisfy_criterion(
                        &company.scope,
                        SatisfyCriterion {
                            work_item_id,
                            expected_version,
                            criterion_id: *criterion_id,
                            actor: human(),
                        },
                    )
                    .await
            }
        };
        match result {
            Ok(aggregate) => return aggregate,
            Err(WorkError::VersionConflict { .. }) => continue,
            Err(other) => panic!(
                "racing mutation of {work_item_id} failed: {other} \
                 (a lock-order deadlock surfaces here as a database error)"
            ),
        }
    }
    panic!(
        "racing mutation of {work_item_id} lost the version race {LOCK_ORDER_MAX_ATTEMPTS} times"
    );
}

/// Regression for the project → item lock order. Dependency additions on
/// one item (project row `FOR UPDATE`, then the item row) race ordinary
/// mutations of that same item and of a sibling item in the same project
/// (project row `FOR SHARE`, then the item row) on separate connections.
/// Before the order was fixed, the ordinary path locked the item row and
/// then waited for the project row that the dependency path already held
/// while it waited for the item row, which Postgres aborted as a deadlock.
/// Every lane must commit inside the bound and leave dense, consistent
/// aggregates.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires a disposable PostgreSQL database"]
async fn concurrent_dependency_and_item_mutations_do_not_deadlock() {
    let pool = setup_pool().await;
    let company = Arc::new(Company::new(&pool).await);
    let project_id = company.project("lock-order").await;
    let criteria: Vec<String> = (0..LOCK_ORDER_ROUNDS)
        .map(|round| format!("criterion {round}"))
        .collect();
    let create = |title: &str| {
        company.service.create_work_item(
            &company.scope,
            NewWorkItem {
                project_id,
                title: title.to_owned(),
                description: String::new(),
                priority: WorkPriority::Normal,
                criteria: criteria.clone(),
                approvals: Vec::new(),
                source_message_id: None,
            },
            human(),
        )
    };
    let hub = create("Hub").await.expect("create hub").item.item;
    let sibling = create("Sibling").await.expect("create sibling").item.item;
    let hub_criteria: Vec<Uuid> = hub.criteria.iter().map(|criterion| criterion.id).collect();
    let sibling_criteria: Vec<Uuid> = sibling
        .criteria
        .iter()
        .map(|criterion| criterion.id)
        .collect();
    let mut targets = Vec::with_capacity(LOCK_ORDER_ROUNDS);
    for round in 0..LOCK_ORDER_ROUNDS {
        targets.push(
            company
                .item(project_id, &format!("Target {round}"))
                .await
                .item
                .id,
        );
    }

    let graph_lane = {
        let company = Arc::clone(&company);
        let hub_id = hub.id;
        tokio::spawn(async move {
            for target in targets {
                commit_racing_mutation(&company, hub_id, RacingMutation::Depend(target)).await;
            }
        })
    };
    let hub_lane = {
        let company = Arc::clone(&company);
        let hub_id = hub.id;
        tokio::spawn(async move {
            for criterion in hub_criteria {
                commit_racing_mutation(&company, hub_id, RacingMutation::Satisfy(criterion)).await;
            }
        })
    };
    let sibling_lane = {
        let company = Arc::clone(&company);
        let sibling_id = sibling.id;
        tokio::spawn(async move {
            for criterion in sibling_criteria {
                commit_racing_mutation(&company, sibling_id, RacingMutation::Satisfy(criterion))
                    .await;
            }
        })
    };
    let lanes = tokio::time::timeout(Duration::from_secs(120), async {
        tokio::try_join!(graph_lane, hub_lane, sibling_lane)
    })
    .await
    .expect("every racing lane commits inside the bound; a hang is a lock-order regression");
    lanes.expect("no racing lane panicked");

    let hub = company
        .service
        .work_item(&company.scope, hub.id)
        .await
        .expect("read hub");
    assert_eq!(
        hub.item.version,
        1 + 2 * LOCK_ORDER_ROUNDS as i64,
        "one version per committed dependency and criterion"
    );
    assert_eq!(hub.item.dependencies.len(), LOCK_ORDER_ROUNDS);
    assert!(hub
        .item
        .criteria
        .iter()
        .all(|criterion| criterion.status == CriterionStatus::Satisfied));
    assert_dense_history(&hub);
    let edges: i64 = sqlx::query(
        "SELECT count(*) FROM work_dependencies
          WHERE company_id = $1 AND project_id = $2 AND work_item_id = $3",
    )
    .bind(company.scope.company_id())
    .bind(project_id)
    .bind(hub.item.id)
    .fetch_one(&pool)
    .await
    .expect("count edges")
    .try_get(0)
    .expect("count");
    assert_eq!(edges, LOCK_ORDER_ROUNDS as i64);

    let sibling = company
        .service
        .work_item(&company.scope, sibling.id)
        .await
        .expect("read sibling");
    assert_eq!(sibling.item.version, 1 + LOCK_ORDER_ROUNDS as i64);
    assert!(sibling
        .item
        .criteria
        .iter()
        .all(|criterion| criterion.status == CriterionStatus::Satisfied));
    assert_dense_history(&sibling);
}
