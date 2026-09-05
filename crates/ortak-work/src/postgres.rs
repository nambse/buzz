//! PostgreSQL implementation of [`WorkRepository`] on the existing
//! [`PgControlPlane`], over the migration 0047 relations plus the 0045 rows
//! it joins for authority (`employees`, `office_inbox`, `routing_decisions`,
//! `runs`).
//!
//! Every mutation follows one shape: open a transaction, verify the actor,
//! lock the item through [`lock_item`], compare the caller's
//! `expected_version`, load the aggregate, run the pure domain command,
//! write the child rows, advance the item row by exactly one version,
//! append exactly one history row, reload the aggregate inside the same
//! transaction, and commit.
//!
//! # Lock order
//!
//! Every path that locks a work item row locks its project row first, in
//! three separate statements, so Postgres can never take the two rows in
//! the opposite order inside one statement:
//!
//! 1. an unlocked, company-scoped read of `work_items.project_id`, which
//!    the `work_items_guard` trigger pins for the life of the row;
//! 2. the project row, `FOR SHARE` for an ordinary item mutation (the
//!    archive fence: an archive takes the same row `FOR UPDATE`) or `FOR
//!    UPDATE` for a dependency-graph mutation (so concurrent graph edits of
//!    one project serialize and their cycle checks see each other);
//! 3. the item row `FOR UPDATE`, then the version compare.
//!
//! Creation locks only the project row (`FOR SHARE`); archive locks only
//! the project row (`FOR UPDATE`); no path locks an item before its
//! project, and no path locks two items. A transaction that holds a project
//! lock therefore never waits for an item lock held by a transaction that
//! is itself waiting for the project, which is the cycle that `FOR UPDATE
//! OF w FOR SHARE OF p` in one statement used to allow.

use std::collections::{BTreeMap, BTreeSet};

use chrono::{DateTime, Utc};
use ortak_control::{CompanyScope, ControlError, MessageId, PgControlPlane};
use ortak_domain::{
    creates_dependency_cycle, AcceptanceCriterion, ApprovalGate, ApprovalStatus, Assignment,
    AssignmentRole, AssignmentStatus, AttachmentRef, CriterionStatus, EmployeeId, NewWorkItem,
    NewWorkItemIds, Project, ProjectEvent, ProjectSlug, ProjectStatus, WorkActor, WorkAttachment,
    WorkDependency, WorkEvent, WorkItem, WorkPriority, WorkState,
};
use sqlx::postgres::PgRow;
use sqlx::{PgConnection, Row};
use uuid::Uuid;

use crate::error::{Result, WorkError};
use crate::model::{
    ProjectRecord, WorkHistoryRecord, WorkItemAggregate, WorkListCursor, WorkListPage,
    WorkListQuery, WorkSummary, MAX_WORK_HISTORY_ROWS,
};
use crate::repository::{
    AddDependency, ArchiveProject, AssignEmployee, AttachRecord, CreateProject, CreateWorkItem,
    ProjectCreation, ResolveApproval, SatisfyCriterion, TransitionWorkItem, WorkItemCreation,
    WorkRepository,
};

// ── SQL ──────────────────────────────────────────────────────────────────────

// The project column list is repeated verbatim in the four project
// statements: sqlx accepts only `'static` statements, so they cannot be
// assembled at runtime.
const PROJECT_SQL: &str = "SELECT id, slug, name, description, status, version,
            created_by_type, created_by_id, created_at, updated_at, archived_at
       FROM projects WHERE company_id = $1 AND id = $2";

const PROJECT_FOR_UPDATE_SQL: &str = "SELECT id, slug, name, description, status, version,
            created_by_type, created_by_id, created_at, updated_at, archived_at
       FROM projects WHERE company_id = $1 AND id = $2 FOR UPDATE";

const PROJECT_FOR_SHARE_SQL: &str = "SELECT id, slug, name, description, status, version,
            created_by_type, created_by_id, created_at, updated_at, archived_at
       FROM projects WHERE company_id = $1 AND id = $2 FOR SHARE";

const PROJECT_BY_SLUG_SQL: &str = "SELECT id, slug, name, description, status, version,
            created_by_type, created_by_id, created_at, updated_at, archived_at
       FROM projects WHERE company_id = $1 AND slug = $2";

const ITEM_SQL: &str = "SELECT w.id, w.project_id, w.title, w.description, w.priority, w.state,
            w.version, w.source_message_id, w.source_routing_decision_id,
            w.created_by_type, w.created_by_id, w.created_at, w.updated_at,
            w.completed_at, w.cancelled_at, p.status AS project_status
       FROM work_items w
       JOIN projects p ON p.company_id = w.company_id AND p.id = w.project_id
      WHERE w.company_id = $1 AND w.id = $2";

// Locks only the item row. Callers hold the project row already (see the
// module docs on lock order), so the joined project status is stable.
const ITEM_FOR_UPDATE_SQL: &str =
    "SELECT w.id, w.project_id, w.title, w.description, w.priority, w.state,
            w.version, w.source_message_id, w.source_routing_decision_id,
            w.created_by_type, w.created_by_id, w.created_at, w.updated_at,
            w.completed_at, w.cancelled_at, p.status AS project_status
       FROM work_items w
       JOIN projects p ON p.company_id = w.company_id AND p.id = w.project_id
      WHERE w.company_id = $1 AND w.id = $2
        FOR UPDATE OF w";

// Unlocked, company-scoped read of the item's immutable owning project.
const ITEM_PROJECT_SQL: &str =
    "SELECT project_id FROM work_items WHERE company_id = $1 AND id = $2";

const PROJECT_STATUS_FOR_SHARE_SQL: &str =
    "SELECT status FROM projects WHERE company_id = $1 AND id = $2 FOR SHARE";

const PROJECT_STATUS_FOR_UPDATE_SQL: &str =
    "SELECT status FROM projects WHERE company_id = $1 AND id = $2 FOR UPDATE";
const CRITERIA_SQL: &str = "SELECT id, position, text, status, satisfied_by_type, satisfied_by_id
       FROM work_acceptance_criteria
      WHERE company_id = $1 AND work_item_id = $2
      ORDER BY position";

const APPROVALS_SQL: &str =
    "SELECT id, gate, required, status, resolved_by_type, resolved_by_id, reason
       FROM work_approvals
      WHERE company_id = $1 AND work_item_id = $2
      ORDER BY gate";

const ASSIGNMENTS_SQL: &str = "SELECT employee_id, role, status
       FROM work_assignments
      WHERE company_id = $1 AND work_item_id = $2
      ORDER BY assigned_at, employee_id";

const DEPENDENCIES_SQL: &str = "SELECT d.depends_on_work_item_id, t.state
       FROM work_dependencies d
       JOIN work_items t
         ON t.company_id = d.company_id AND t.id = d.depends_on_work_item_id
      WHERE d.company_id = $1 AND d.work_item_id = $2
      ORDER BY d.created_at, d.depends_on_work_item_id";

const ATTACHMENTS_SQL: &str = "SELECT id, kind, message_id, routing_decision_id, run_id, label
       FROM work_attachments
      WHERE company_id = $1 AND work_item_id = $2
      ORDER BY attached_at, kind, id";

const HISTORY_SQL: &str = "SELECT sequence, version, actor_type, actor_id, payload, recorded_at
       FROM work_item_history
      WHERE company_id = $1 AND work_item_id = $2
      ORDER BY sequence
      LIMIT $3";

const UPDATE_ITEM_SQL: &str = "UPDATE work_items
        SET state = $3,
            version = $4,
            updated_at = now(),
            completed_at = CASE WHEN $3 = 'completed' THEN coalesce(completed_at, now()) ELSE completed_at END,
            cancelled_at = CASE WHEN $3 = 'cancelled' THEN coalesce(cancelled_at, now()) ELSE cancelled_at END
      WHERE company_id = $1 AND id = $2 AND version = $5";

const INSERT_HISTORY_SQL: &str = "INSERT INTO work_item_history
        (company_id, work_item_id, sequence, version, event_type,
         actor_type, actor_id, from_state, to_state, payload)
     VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)";

// A decision is dispatch provenance only when it actually woke someone for
// this message; a silent or all-drop decision is not.
const WAKING_DECISION_FOR_MESSAGE_SQL: &str = "SELECT d.id
       FROM routing_decisions d
      WHERE d.company_id = $1
        AND d.message_id = $2
        AND EXISTS (SELECT 1 FROM routing_recipients r
                     WHERE r.company_id = d.company_id
                       AND r.routing_decision_id = d.id
                       AND r.action = 'wake')";

const LIST_SQL: &str = "SELECT id, project_id, title, priority, state, version,
            source_message_id, created_at, updated_at
       FROM work_items
      WHERE company_id = $1
        AND project_id = $2
        AND ($3::text[] IS NULL OR state = ANY($3::text[]))
        AND ($4::timestamptz IS NULL OR (created_at, id) < ($4::timestamptz, $5::uuid))
      ORDER BY created_at DESC, id DESC
      LIMIT $6";

// ── Row helpers ──────────────────────────────────────────────────────────────

fn invalid(detail: impl Into<String>) -> WorkError {
    WorkError::InvalidRecord {
        detail: detail.into(),
    }
}

fn parse_state(value: &str) -> Result<WorkState> {
    WorkState::parse(value).ok_or_else(|| invalid(format!("work_items.state holds {value:?}")))
}

fn parse_actor(row: &PgRow, type_column: &str, id_column: &str) -> Result<WorkActor> {
    let actor_type: String = row.try_get(type_column)?;
    let actor_id: Option<String> = row.try_get(id_column)?;
    WorkActor::from_columns(&actor_type, actor_id.as_deref()).map_err(|_| {
        invalid(format!(
            "{type_column}/{id_column} hold an unreadable actor"
        ))
    })
}

fn parse_optional_actor(
    row: &PgRow,
    type_column: &str,
    id_column: &str,
) -> Result<Option<WorkActor>> {
    let actor_type: Option<String> = row.try_get(type_column)?;
    match actor_type {
        None => Ok(None),
        Some(actor_type) => {
            let actor_id: Option<String> = row.try_get(id_column)?;
            WorkActor::from_columns(&actor_type, actor_id.as_deref())
                .map(Some)
                .map_err(|_| {
                    invalid(format!(
                        "{type_column}/{id_column} hold an unreadable actor"
                    ))
                })
        }
    }
}

fn project_record(row: &PgRow) -> Result<ProjectRecord> {
    let slug: String = row.try_get("slug")?;
    let status: String = row.try_get("status")?;
    Ok(ProjectRecord {
        project: Project {
            id: row.try_get("id")?,
            slug: ProjectSlug::parse(slug).map_err(|_| invalid("projects.slug is malformed"))?,
            name: row.try_get("name")?,
            description: row.try_get("description")?,
            status: ProjectStatus::parse(&status)
                .ok_or_else(|| invalid(format!("projects.status holds {status:?}")))?,
            version: row.try_get("version")?,
        },
        created_by: parse_actor(row, "created_by_type", "created_by_id")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
        archived_at: row.try_get("archived_at")?,
    })
}

/// The item row as read, before its children are loaded.
struct ItemRow {
    item: WorkItem,
    source_routing_decision_id: Option<Uuid>,
    created_by: WorkActor,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    completed_at: Option<DateTime<Utc>>,
    cancelled_at: Option<DateTime<Utc>>,
    project_status: ProjectStatus,
}

fn item_row(row: &PgRow) -> Result<ItemRow> {
    let priority: String = row.try_get("priority")?;
    let state: String = row.try_get("state")?;
    let project_status: String = row.try_get("project_status")?;
    let source: Option<Vec<u8>> = row.try_get("source_message_id")?;
    let source_message_id = source
        .map(|bytes| MessageId::try_from_slice(&bytes).map(|id| id.to_hex()))
        .transpose()?;
    Ok(ItemRow {
        item: WorkItem {
            id: row.try_get("id")?,
            project_id: row.try_get("project_id")?,
            title: row.try_get("title")?,
            description: row.try_get("description")?,
            priority: WorkPriority::parse(&priority)
                .ok_or_else(|| invalid(format!("work_items.priority holds {priority:?}")))?,
            state: parse_state(&state)?,
            version: row.try_get("version")?,
            source_message_id,
            criteria: Vec::new(),
            approvals: Vec::new(),
            assignments: Vec::new(),
            dependencies: Vec::new(),
            attachments: Vec::new(),
        },
        source_routing_decision_id: row.try_get("source_routing_decision_id")?,
        created_by: parse_actor(row, "created_by_type", "created_by_id")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
        completed_at: row.try_get("completed_at")?,
        cancelled_at: row.try_get("cancelled_at")?,
        project_status: ProjectStatus::parse(&project_status)
            .ok_or_else(|| invalid(format!("projects.status holds {project_status:?}")))?,
    })
}

fn attachment_from_row(row: &PgRow) -> Result<WorkAttachment> {
    let kind: String = row.try_get("kind")?;
    let reference = match kind.as_str() {
        "office_message" => {
            let bytes: Vec<u8> = row.try_get("message_id")?;
            AttachmentRef::OfficeMessage {
                message_id: MessageId::try_from_slice(&bytes)?.to_hex(),
            }
        }
        "routing_decision" => AttachmentRef::RoutingDecision {
            decision_id: row.try_get("routing_decision_id")?,
        },
        "run" => AttachmentRef::Run {
            run_id: row.try_get("run_id")?,
        },
        other => return Err(invalid(format!("work_attachments.kind holds {other:?}"))),
    };
    Ok(WorkAttachment {
        id: row.try_get("id")?,
        reference,
        label: row.try_get("label")?,
    })
}

fn summary_from_row(row: &PgRow) -> Result<WorkSummary> {
    let priority: String = row.try_get("priority")?;
    let state: String = row.try_get("state")?;
    let source: Option<Vec<u8>> = row.try_get("source_message_id")?;
    Ok(WorkSummary {
        id: row.try_get("id")?,
        project_id: row.try_get("project_id")?,
        title: row.try_get("title")?,
        priority: WorkPriority::parse(&priority)
            .ok_or_else(|| invalid(format!("work_items.priority holds {priority:?}")))?,
        state: parse_state(&state)?,
        version: row.try_get("version")?,
        source_message_id: source
            .map(|bytes| MessageId::try_from_slice(&bytes).map(|id| id.to_hex()))
            .transpose()?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}

// ── Authority checks ─────────────────────────────────────────────────────────

/// An employee actor must be an `active` employee of the company.
async fn verify_actor(
    connection: &mut PgConnection,
    scope: &CompanyScope,
    actor: &WorkActor,
) -> Result<()> {
    actor.validate()?;
    if let WorkActor::Employee(employee_id) = actor {
        if !employee_is_active(connection, scope, employee_id).await? {
            return Err(WorkError::ActorNotFound {
                employee_id: employee_id.clone(),
            });
        }
    }
    Ok(())
}

async fn employee_is_active(
    connection: &mut PgConnection,
    scope: &CompanyScope,
    employee_id: &EmployeeId,
) -> Result<bool> {
    let row = sqlx::query("SELECT status FROM employees WHERE company_id = $1 AND id = $2")
        .bind(scope.company_id())
        .bind(employee_id.as_str())
        .fetch_optional(&mut *connection)
        .await?;
    Ok(row
        .map(|row| row.try_get::<String, _>("status"))
        .transpose()?
        .is_some_and(|status| status == "active"))
}

async fn source_message_is_decided(
    connection: &mut PgConnection,
    scope: &CompanyScope,
    message_id: MessageId,
) -> Result<bool> {
    let row = sqlx::query("SELECT state FROM office_inbox WHERE company_id = $1 AND event_id = $2")
        .bind(scope.company_id())
        .bind(message_id.as_bytes().as_slice())
        .fetch_optional(&mut *connection)
        .await?;
    Ok(row
        .map(|row| row.try_get::<String, _>("state"))
        .transpose()?
        .is_some_and(|state| state == "decided"))
}

/// The decision that dispatched the message, if one woke at least one
/// employee. A silent or all-drop decision dispatched nothing and is not
/// attached as provenance.
async fn waking_decision_for_message(
    connection: &mut PgConnection,
    scope: &CompanyScope,
    message_id: MessageId,
) -> Result<Option<Uuid>> {
    let row = sqlx::query(WAKING_DECISION_FOR_MESSAGE_SQL)
        .bind(scope.company_id())
        .bind(message_id.as_bytes().as_slice())
        .fetch_optional(&mut *connection)
        .await?;
    row.map(|row| row.try_get("id"))
        .transpose()
        .map_err(Into::into)
}

async fn attachment_target_exists(
    connection: &mut PgConnection,
    scope: &CompanyScope,
    reference: &AttachmentRef,
) -> Result<bool> {
    let row = match reference {
        AttachmentRef::OfficeMessage { message_id } => {
            let message_id = MessageId::parse_hex(message_id)?;
            return source_message_is_decided(connection, scope, message_id).await;
        }
        AttachmentRef::RoutingDecision { decision_id } => {
            sqlx::query("SELECT 1 FROM routing_decisions WHERE company_id = $1 AND id = $2")
                .bind(scope.company_id())
                .bind(decision_id)
                .fetch_optional(&mut *connection)
                .await?
        }
        AttachmentRef::Run { run_id } => {
            sqlx::query("SELECT 1 FROM runs WHERE company_id = $1 AND id = $2")
                .bind(scope.company_id())
                .bind(run_id)
                .fetch_optional(&mut *connection)
                .await?
        }
    };
    Ok(row.is_some())
}

// ── Aggregate loading ────────────────────────────────────────────────────────

async fn load_children(
    connection: &mut PgConnection,
    scope: &CompanyScope,
    item: &mut WorkItem,
) -> Result<()> {
    let company_id = scope.company_id();

    for row in sqlx::query(CRITERIA_SQL)
        .bind(company_id)
        .bind(item.id)
        .fetch_all(&mut *connection)
        .await?
    {
        let status: String = row.try_get("status")?;
        let position: i16 = row.try_get("position")?;
        item.criteria.push(AcceptanceCriterion {
            id: row.try_get("id")?,
            position: u16::try_from(position)
                .map_err(|_| invalid("criterion position is negative"))?,
            text: row.try_get("text")?,
            status: CriterionStatus::parse(&status).ok_or_else(|| {
                invalid(format!("work_acceptance_criteria.status holds {status:?}"))
            })?,
            satisfied_by: parse_optional_actor(&row, "satisfied_by_type", "satisfied_by_id")?,
        });
    }

    for row in sqlx::query(APPROVALS_SQL)
        .bind(company_id)
        .bind(item.id)
        .fetch_all(&mut *connection)
        .await?
    {
        let status: String = row.try_get("status")?;
        item.approvals.push(ApprovalGate {
            id: row.try_get("id")?,
            gate: row.try_get("gate")?,
            required: row.try_get("required")?,
            status: ApprovalStatus::parse(&status)
                .ok_or_else(|| invalid(format!("work_approvals.status holds {status:?}")))?,
            resolved_by: parse_optional_actor(&row, "resolved_by_type", "resolved_by_id")?,
            reason: row.try_get("reason")?,
        });
    }

    for row in sqlx::query(ASSIGNMENTS_SQL)
        .bind(company_id)
        .bind(item.id)
        .fetch_all(&mut *connection)
        .await?
    {
        let employee_id: String = row.try_get("employee_id")?;
        let role: String = row.try_get("role")?;
        let status: String = row.try_get("status")?;
        item.assignments.push(Assignment {
            employee_id: EmployeeId::parse(employee_id)
                .map_err(|_| invalid("work_assignments.employee_id is malformed"))?,
            role: AssignmentRole::parse(&role)
                .ok_or_else(|| invalid(format!("work_assignments.role holds {role:?}")))?,
            status: AssignmentStatus::parse(&status)
                .ok_or_else(|| invalid(format!("work_assignments.status holds {status:?}")))?,
        });
    }

    for row in sqlx::query(DEPENDENCIES_SQL)
        .bind(company_id)
        .bind(item.id)
        .fetch_all(&mut *connection)
        .await?
    {
        let state: String = row.try_get("state")?;
        item.dependencies.push(WorkDependency {
            depends_on: row.try_get("depends_on_work_item_id")?,
            depends_on_state: parse_state(&state)?,
        });
    }

    for row in sqlx::query(ATTACHMENTS_SQL)
        .bind(company_id)
        .bind(item.id)
        .fetch_all(&mut *connection)
        .await?
    {
        item.attachments.push(attachment_from_row(&row)?);
    }

    Ok(())
}

async fn load_history(
    connection: &mut PgConnection,
    scope: &CompanyScope,
    work_item_id: Uuid,
) -> Result<(Vec<WorkHistoryRecord>, bool)> {
    let rows = sqlx::query(HISTORY_SQL)
        .bind(scope.company_id())
        .bind(work_item_id)
        .bind(MAX_WORK_HISTORY_ROWS + 1)
        .fetch_all(&mut *connection)
        .await?;
    let truncated = rows.len() as i64 > MAX_WORK_HISTORY_ROWS;
    let mut history = Vec::with_capacity(rows.len());
    for row in rows.iter().take(MAX_WORK_HISTORY_ROWS as usize) {
        let payload: serde_json::Value = row.try_get("payload")?;
        let event: WorkEvent = serde_json::from_value(payload)
            .map_err(|_| invalid("work_item_history.payload is not a typed work event"))?;
        history.push(WorkHistoryRecord {
            sequence: row.try_get("sequence")?,
            version: row.try_get("version")?,
            actor: parse_actor(row, "actor_type", "actor_id")?,
            event,
            recorded_at: row.try_get("recorded_at")?,
        });
    }
    Ok((history, truncated))
}

/// Reads the aggregate, optionally locking the item row `FOR UPDATE`.
///
/// With `for_update`, the caller must already hold the item's project row
/// (see [`lock_item`]); this function never takes a project lock itself.
async fn load_aggregate(
    connection: &mut PgConnection,
    scope: &CompanyScope,
    work_item_id: Uuid,
    for_update: bool,
) -> Result<Option<(WorkItemAggregate, ProjectStatus)>> {
    let sql = if for_update {
        ITEM_FOR_UPDATE_SQL
    } else {
        ITEM_SQL
    };
    let row = sqlx::query(sql)
        .bind(scope.company_id())
        .bind(work_item_id)
        .fetch_optional(&mut *connection)
        .await?;
    let Some(row) = row else {
        return Ok(None);
    };
    let mut row = item_row(&row)?;
    load_children(connection, scope, &mut row.item).await?;
    let (history, history_truncated) = load_history(connection, scope, work_item_id).await?;
    Ok(Some((
        WorkItemAggregate {
            item: row.item,
            source_routing_decision_id: row.source_routing_decision_id,
            created_by: row.created_by,
            created_at: row.created_at,
            updated_at: row.updated_at,
            completed_at: row.completed_at,
            cancelled_at: row.cancelled_at,
            history,
            history_truncated,
        },
        row.project_status,
    )))
}

async fn require_aggregate(
    connection: &mut PgConnection,
    scope: &CompanyScope,
    work_item_id: Uuid,
) -> Result<WorkItemAggregate> {
    load_aggregate(connection, scope, work_item_id, false)
        .await?
        .map(|(aggregate, _)| aggregate)
        .ok_or(WorkError::WorkItemNotFound { work_item_id })
}

/// How an item mutation locks the item's project row before the item row.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ProjectLock {
    /// Ordinary item mutation: fence out a concurrent archive.
    Share,
    /// Dependency-graph mutation: serialize with every other graph mutation
    /// of the project so cycle checks see each other's edges.
    Exclusive,
}

/// Locks the item's project row, then the item row, refuses archived
/// projects, and compares the version.
///
/// This is the only place an item row is locked. The order is fixed at
/// project → item in three statements (see the module docs): the project
/// id is read unlocked and company-scoped first, which is safe because the
/// `work_items_guard` trigger pins `project_id` for the life of the row. An
/// unknown id and an id of another company both fail as not found before
/// any lock is taken.
async fn lock_item(
    connection: &mut PgConnection,
    scope: &CompanyScope,
    work_item_id: Uuid,
    expected_version: i64,
    project_lock: ProjectLock,
) -> Result<WorkItem> {
    // 1. Unlocked, company-scoped read of the immutable owning project.
    let project_id: Uuid = sqlx::query(ITEM_PROJECT_SQL)
        .bind(scope.company_id())
        .bind(work_item_id)
        .fetch_optional(&mut *connection)
        .await?
        .ok_or(WorkError::WorkItemNotFound { work_item_id })?
        .try_get("project_id")?;

    // 2. Project row first. The foreign key guarantees it exists in the
    //    same company, so a miss is a broken row, not a scope failure.
    let project_sql = match project_lock {
        ProjectLock::Share => PROJECT_STATUS_FOR_SHARE_SQL,
        ProjectLock::Exclusive => PROJECT_STATUS_FOR_UPDATE_SQL,
    };
    let status: String = sqlx::query(project_sql)
        .bind(scope.company_id())
        .bind(project_id)
        .fetch_optional(&mut *connection)
        .await?
        .ok_or_else(|| invalid("work item's project row is missing"))?
        .try_get("status")?;
    let project_status = ProjectStatus::parse(&status)
        .ok_or_else(|| invalid(format!("projects.status holds {status:?}")))?;
    if project_status == ProjectStatus::Archived {
        return Err(WorkError::ProjectArchived { project_id });
    }

    // 3. Item row, re-read under its lock.
    let Some((aggregate, project_status)) =
        load_aggregate(connection, scope, work_item_id, true).await?
    else {
        return Err(WorkError::WorkItemNotFound { work_item_id });
    };
    if aggregate.item.project_id != project_id {
        return Err(invalid("work item changed project under its lock"));
    }
    if project_status == ProjectStatus::Archived {
        return Err(WorkError::ProjectArchived { project_id });
    }
    if aggregate.item.version != expected_version {
        return Err(WorkError::VersionConflict {
            record_id: work_item_id,
            expected: expected_version,
            actual: aggregate.item.version,
        });
    }
    Ok(aggregate.item)
}

// ── Persistence of one event ─────────────────────────────────────────────────

async fn insert_history(
    connection: &mut PgConnection,
    scope: &CompanyScope,
    item: &WorkItem,
    actor: &WorkActor,
    event: &WorkEvent,
) -> Result<()> {
    let payload = serde_json::to_value(event).map_err(ControlError::Serde)?;
    let (from_state, to_state) = match event.state_change() {
        Some((from, to)) => (Some(from.as_str()), Some(to.as_str())),
        None => (None, None),
    };
    sqlx::query(INSERT_HISTORY_SQL)
        .bind(scope.company_id())
        .bind(item.id)
        .bind(item.version - 1)
        .bind(item.version)
        .bind(event.event_type())
        .bind(actor.type_str())
        .bind(actor.id_str())
        .bind(from_state)
        .bind(to_state)
        .bind(payload)
        .execute(&mut *connection)
        .await?;
    Ok(())
}

/// Advances the item row from `expected_version` to `item.version` and
/// appends the event. The guard trigger refuses any other step.
async fn persist_event(
    connection: &mut PgConnection,
    scope: &CompanyScope,
    item: &WorkItem,
    expected_version: i64,
    actor: &WorkActor,
    event: &WorkEvent,
) -> Result<()> {
    let updated = sqlx::query(UPDATE_ITEM_SQL)
        .bind(scope.company_id())
        .bind(item.id)
        .bind(item.state.as_str())
        .bind(item.version)
        .bind(expected_version)
        .execute(&mut *connection)
        .await?
        .rows_affected();
    if updated != 1 {
        return Err(WorkError::VersionConflict {
            record_id: item.id,
            expected: expected_version,
            actual: item.version,
        });
    }
    insert_history(connection, scope, item, actor, event).await
}

async fn insert_project_history(
    connection: &mut PgConnection,
    scope: &CompanyScope,
    project: &Project,
    actor: &WorkActor,
    event: &ProjectEvent,
) -> Result<()> {
    let payload = serde_json::to_value(event).map_err(ControlError::Serde)?;
    sqlx::query(
        "INSERT INTO project_history
             (company_id, project_id, sequence, event_type, actor_type, actor_id, payload)
         VALUES ($1, $2, $3, $4, $5, $6, $7)",
    )
    .bind(scope.company_id())
    .bind(project.id)
    .bind(project.version - 1)
    .bind(event.event_type())
    .bind(actor.type_str())
    .bind(actor.id_str())
    .bind(payload)
    .execute(&mut *connection)
    .await?;
    Ok(())
}

async fn insert_attachment(
    connection: &mut PgConnection,
    scope: &CompanyScope,
    work_item_id: Uuid,
    attachment: &WorkAttachment,
    actor: &WorkActor,
) -> Result<()> {
    let (message_id, decision_id, run_id) = match &attachment.reference {
        AttachmentRef::OfficeMessage { message_id } => {
            (Some(MessageId::parse_hex(message_id)?), None, None)
        }
        AttachmentRef::RoutingDecision { decision_id } => (None, Some(*decision_id), None),
        AttachmentRef::Run { run_id } => (None, None, Some(*run_id)),
    };
    sqlx::query(
        "INSERT INTO work_attachments
             (company_id, work_item_id, id, kind, message_id, routing_decision_id, run_id,
              label, attached_by_type, attached_by_id)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)",
    )
    .bind(scope.company_id())
    .bind(work_item_id)
    .bind(attachment.id)
    .bind(attachment.reference.kind_str())
    .bind(message_id.as_ref().map(|id| id.as_bytes().to_vec()))
    .bind(decision_id)
    .bind(run_id)
    .bind(attachment.label.as_deref())
    .bind(actor.type_str())
    .bind(actor.id_str())
    .execute(&mut *connection)
    .await?;
    Ok(())
}

async fn existing_item_for_source(
    connection: &mut PgConnection,
    scope: &CompanyScope,
    message_id: MessageId,
) -> Result<Option<Uuid>> {
    let row =
        sqlx::query("SELECT id FROM work_items WHERE company_id = $1 AND source_message_id = $2")
            .bind(scope.company_id())
            .bind(message_id.as_bytes().as_slice())
            .fetch_optional(&mut *connection)
            .await?;
    row.map(|row| row.try_get("id"))
        .transpose()
        .map_err(Into::into)
}

/// Whether a promotion input is a replay of the item the message was
/// already promoted to: same project and same immutable creation
/// definition. Approval gates are compared as a set (they are unique per
/// item); criteria are ordered.
fn is_promotion_replay(existing: &WorkItem, input: &NewWorkItem) -> bool {
    let gates =
        |gates: &[(String, bool)]| -> BTreeMap<String, bool> { gates.iter().cloned().collect() };
    existing.project_id == input.project_id
        && existing.title == input.title
        && existing.description == input.description
        && existing.priority == input.priority
        && existing
            .criteria
            .iter()
            .map(|criterion| criterion.text.as_str())
            .eq(input.criteria.iter().map(String::as_str))
        && gates(
            &existing
                .approvals
                .iter()
                .map(|gate| (gate.gate.clone(), gate.required))
                .collect::<Vec<_>>(),
        ) == gates(
            &input
                .approvals
                .iter()
                .map(|gate| (gate.gate.clone(), gate.required))
                .collect::<Vec<_>>(),
        )
}

/// Resolves a promotion replay: returns the item the message was already
/// promoted to when the input matches it, and refuses a call that names a
/// different project or definition. Runs on the creation connection;
/// the existing item is read as it stands, whether or not its
/// project has since been archived.
async fn replayed_promotion(
    connection: &mut PgConnection,
    scope: &CompanyScope,
    existing: Uuid,
    input: &NewWorkItem,
    message_id: MessageId,
) -> Result<WorkItemCreation> {
    let item = require_aggregate(connection, scope, existing).await?;
    if !is_promotion_replay(&item.item, input) {
        return Err(WorkError::PromotionConflict {
            message_id: message_id.to_hex(),
            work_item_id: existing,
        });
    }
    Ok(WorkItemCreation {
        item,
        created: false,
    })
}

// ── Repository ───────────────────────────────────────────────────────────────

mod authorized;
mod commands;
mod creation;
mod reads;
pub use authorized::*;

impl WorkRepository for PgControlPlane {
    async fn create_project(
        &self,
        scope: &CompanyScope,
        command: &CreateProject,
    ) -> Result<ProjectCreation> {
        let mut tx = self.pool().begin().await?;
        let result = creation::create_project_on(&mut tx, scope, command).await?;
        tx.commit().await?;
        Ok(result)
    }

    async fn archive_project(
        &self,
        scope: &CompanyScope,
        command: &ArchiveProject,
    ) -> Result<ProjectRecord> {
        let mut tx = self.pool().begin().await?;
        let result = commands::archive_project_on(&mut tx, scope, command).await?;
        tx.commit().await?;
        Ok(result)
    }

    async fn project(&self, scope: &CompanyScope, project_id: Uuid) -> Result<ProjectRecord> {
        let mut tx = self.pool().begin().await?;
        let result = reads::project_on(&mut tx, scope, project_id).await?;
        tx.commit().await?;
        Ok(result)
    }

    async fn create_work_item(
        &self,
        scope: &CompanyScope,
        command: &CreateWorkItem,
    ) -> Result<WorkItemCreation> {
        let mut tx = self.pool().begin().await?;
        let result = creation::create_work_item_on(&mut tx, scope, command).await?;
        tx.commit().await?;
        Ok(result)
    }

    async fn assign_employee(
        &self,
        scope: &CompanyScope,
        command: &AssignEmployee,
    ) -> Result<WorkItemAggregate> {
        let mut tx = self.pool().begin().await?;
        let result = commands::assign_employee_on(&mut tx, scope, command).await?;
        tx.commit().await?;
        Ok(result)
    }

    async fn add_dependency(
        &self,
        scope: &CompanyScope,
        command: &AddDependency,
    ) -> Result<WorkItemAggregate> {
        let mut tx = self.pool().begin().await?;
        let result = commands::add_dependency_on(&mut tx, scope, command).await?;
        tx.commit().await?;
        Ok(result)
    }

    async fn transition_work_item(
        &self,
        scope: &CompanyScope,
        command: &TransitionWorkItem,
    ) -> Result<WorkItemAggregate> {
        let mut tx = self.pool().begin().await?;
        let result = commands::transition_work_item_on(&mut tx, scope, command).await?;
        tx.commit().await?;
        Ok(result)
    }

    async fn satisfy_criterion(
        &self,
        scope: &CompanyScope,
        command: &SatisfyCriterion,
    ) -> Result<WorkItemAggregate> {
        let mut tx = self.pool().begin().await?;
        let result = commands::satisfy_criterion_on(&mut tx, scope, command).await?;
        tx.commit().await?;
        Ok(result)
    }

    async fn resolve_approval(
        &self,
        scope: &CompanyScope,
        command: &ResolveApproval,
    ) -> Result<WorkItemAggregate> {
        let mut tx = self.pool().begin().await?;
        let result = commands::resolve_approval_on(&mut tx, scope, command).await?;
        tx.commit().await?;
        Ok(result)
    }

    async fn attach_record(
        &self,
        scope: &CompanyScope,
        command: &AttachRecord,
    ) -> Result<WorkItemAggregate> {
        let mut tx = self.pool().begin().await?;
        let result = commands::attach_record_on(&mut tx, scope, command).await?;
        tx.commit().await?;
        Ok(result)
    }

    async fn work_item(
        &self,
        scope: &CompanyScope,
        work_item_id: Uuid,
    ) -> Result<WorkItemAggregate> {
        let mut tx = self.pool().begin().await?;
        let result = reads::work_item_on(&mut tx, scope, work_item_id).await?;
        tx.commit().await?;
        Ok(result)
    }

    async fn list_project_work(
        &self,
        scope: &CompanyScope,
        project_id: Uuid,
        query: &WorkListQuery,
    ) -> Result<WorkListPage> {
        let mut tx = self.pool().begin().await?;
        let result = reads::list_project_work_on(&mut tx, scope, project_id, query).await?;
        tx.commit().await?;
        Ok(result)
    }
}
