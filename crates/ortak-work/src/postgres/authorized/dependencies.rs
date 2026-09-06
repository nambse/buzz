//! Bounded graph edits acquire the exclusive project fence before any item lock.
use super::*;
use serde::Serialize;

/// One actor-free dependency command, hashed with its source and observed version.
#[derive(Clone, Debug, Serialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum DependencyAction {
    /// Add or reactivate a currently visible same-project blocker.
    Add {
        /// Target work item.
        depends_on: Uuid,
    },
    /// Release a retained edge, including when its target is no longer visible.
    Remove {
        /// Opaque edge identity, scoped to the source work item.
        dependency_id: Uuid,
        /// Bounded nonempty human explanation.
        reason: String,
    },
}
/// One active relation, disclosing its target only under current source authority.
#[derive(Clone, Debug)]
pub struct WorkDependencyView {
    /// Stable retained edge identity; safe to use for hidden-target removal.
    pub id: Uuid,
    /// Current authorized target, absent after canonical source removal.
    pub target: Option<WorkSummary>,
}
/// At most 32 active dependencies under one coherent work version.
#[derive(Clone, Debug)]
pub struct WorkDependencyPage {
    /// Source work item.
    pub work_item_id: Uuid,
    /// Source version associated with this read.
    pub work_version: i64,
    /// Bounded active dependencies, including opaque hidden-target recovery entries.
    pub dependencies: Vec<WorkDependencyView>,
}

impl AuthorizedWork {
    /// Read current dependency targets without treating retained relations as grants.
    pub async fn dependencies(&self, id: Uuid) -> Result<WorkDependencyPage> {
        bounded(async {
            let (mut tx, deadline) = self.begin().await?;
            let (project, source) = self.item_on(&mut tx, id, false).await?;
            let mut q = sqlx::QueryBuilder::new("SELECT d.id AS dependency_id,w.id,w.project_id,w.title,w.priority,w.state,w.version,w.source_message_id,w.created_at,w.updated_at,");
            q.push(authority::SOURCE_VISIBLE).push(" AS visible FROM work_dependencies d
                JOIN work_items w ON w.company_id=d.company_id AND w.project_id=d.project_id AND w.id=d.depends_on_work_item_id
                WHERE d.company_id=$1 AND d.work_item_id=$4 AND d.released_at IS NULL ORDER BY d.id LIMIT 33 FOR SHARE OF d,w");
            let rows = q.build().bind(self.scope.company_id()).bind(self.principal.community_id)
                .bind(project.channel_id).bind(id).fetch_all(&mut *tx).await?;
            if rows.len() > ortak_domain::MAX_WORK_DEPENDENCIES { return Err(invalid("dependency count exceeds the domain bound")); }
            let dependencies = rows.iter().map(|row| Ok(WorkDependencyView {
                id: row.try_get("dependency_id")?, target: if row.try_get("visible")? { Some(summary_from_row(row)?) } else { None },
            })).collect::<Result<Vec<_>>>()?;
            self.finish(tx,deadline).await?;
            Ok(WorkDependencyPage { work_item_id:id, work_version:source.item.version, dependencies })
        }).await
    }

    /// Atomically add/remove a dependency with current scope, graph locks and a receipt.
    pub async fn mutate_dependency(
        &self,
        operation: Uuid,
        id: Uuid,
        version: i64,
        action: DependencyAction,
    ) -> Result<WorkItemAggregate> {
        bounded(self.dependency_inner(operation, id, version, action)).await
    }

    async fn dependency_inner(
        &self,
        operation: Uuid,
        id: Uuid,
        version: i64,
        action: DependencyAction,
    ) -> Result<WorkItemAggregate> {
        if version < 1 {
            return Err(WorkError::InvalidQuery(
                "expected_version must be at least 1",
            ));
        }
        match &action {
            DependencyAction::Add { depends_on } if depends_on.is_nil() => {
                return Err(WorkError::InvalidQuery("dependency target is invalid"))
            }
            DependencyAction::Remove {
                dependency_id,
                reason,
            } if dependency_id.is_nil()
                || reason.trim().is_empty()
                || reason.len() > ortak_domain::MAX_WORK_REASON_BYTES
                || reason.chars().any(char::is_control)
                || ortak_control::run_event::RedactionPolicy::new().redact(reason) != *reason =>
            {
                return Err(WorkError::InvalidQuery("dependency removal is invalid"))
            }
            _ => {}
        }
        let hash = fingerprint(("dependency", id, version, &action))?;
        let (mut tx, deadline) = self.begin().await?;
        let receipt = self
            .operation_on(&mut tx, operation, "mutate_work_item", &hash)
            .await?;
        let missing = || WorkError::WorkItemNotFound { work_item_id: id };
        let project_id: Uuid = sqlx::query_scalar(ITEM_PROJECT_SQL)
            .bind(self.scope.company_id())
            .bind(id)
            .fetch_optional(&mut *tx)
            .await?
            .ok_or_else(missing)?;
        // Never acquire a shared project lock then upgrade it: opposite concurrent
        // graph requests would each hold SHARE while waiting for the other's release.
        sqlx::query(PROJECT_FOR_UPDATE_SQL)
            .bind(self.scope.company_id())
            .bind(project_id)
            .fetch_optional(&mut *tx)
            .await?
            .ok_or_else(missing)?;
        let (project, current) = self.item_on(&mut tx, id, true).await?;
        self.contribute(project.role)?;
        if let DependencyAction::Add { depends_on } = &action {
            // Check same-project identity before acquiring a second project's lock.
            let target_project: Option<Uuid> = sqlx::query_scalar(ITEM_PROJECT_SQL)
                .bind(self.scope.company_id())
                .bind(depends_on)
                .fetch_optional(&mut *tx)
                .await?;
            if target_project != Some(project_id) {
                return Err(WorkError::WorkItemNotFound {
                    work_item_id: *depends_on,
                });
            }
            self.item_on(&mut tx, *depends_on, false).await?;
        }
        if let Some(receipt) = receipt {
            if receipt.project_id != project_id || receipt.work_item_id != Some(id) {
                return Err(WorkError::OperationConflict);
            }
            self.finish(tx, deadline).await?;
            return Ok(current);
        }
        let actor = self.actor();
        let result = match action {
            DependencyAction::Add { depends_on } => {
                commands::add_dependency_on(
                    &mut tx,
                    &self.scope,
                    &AddDependency {
                        work_item_id: id,
                        expected_version: version,
                        depends_on,
                        actor: actor.clone(),
                    },
                )
                .await?
            }
            DependencyAction::Remove {
                dependency_id,
                reason,
            } => {
                let mut item =
                    lock_item(&mut tx, &self.scope, id, version, ProjectLock::Exclusive).await?;
                let target:Option<Uuid> = sqlx::query_scalar("SELECT depends_on_work_item_id FROM work_dependencies WHERE company_id=$1 AND work_item_id=$2 AND id=$3 AND released_at IS NULL")
                    .bind(self.scope.company_id()).bind(id).bind(dependency_id).fetch_optional(&mut *tx).await?;
                let target = target.ok_or(ortak_domain::DomainError::UnknownWorkDependency)?;
                let event = item.remove_dependency(target, reason)?;
                let affected = sqlx::query("UPDATE work_dependencies SET released_at=clock_timestamp() WHERE company_id=$1 AND work_item_id=$2 AND id=$3 AND released_at IS NULL")
                    .bind(self.scope.company_id()).bind(id).bind(dependency_id).execute(&mut *tx).await?.rows_affected();
                if affected != 1 {
                    return Err(invalid("dependency release row disagrees with aggregate"));
                }
                persist_event(&mut tx, &self.scope, &item, version, &actor, &event).await?;
                require_aggregate(&mut tx, &self.scope, id).await?
            }
        };
        self.record_on(
            &mut tx,
            operation,
            "mutate_work_item",
            &hash,
            project_id,
            Some(id),
            result.item.version,
            deadline,
        )
        .await?;
        self.finish(tx, deadline).await?;
        Ok(result)
    }
}
