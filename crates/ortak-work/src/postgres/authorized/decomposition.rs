//! Fresh-child creation, immutable links and independently authorized navigation.
use super::*;
use ortak_domain::{NewChildWork, MAX_WORK_CHILDREN, MAX_WORK_DEPTH};

/// One atomic decomposition result; replay returns currently authorized records.
#[derive(Clone, Debug)]
pub struct WorkChildCreation {
    /// Parent with its one committed structural-history advance.
    pub parent: WorkItemAggregate,
    /// Independently defined child.
    pub child: WorkItemAggregate,
    /// False when a current-authority replay returned the same child.
    pub created: bool,
}
/// Bounded links visible under the current item's own project/source authority.
#[derive(Clone, Debug)]
pub struct WorkDecomposition {
    /// Selected item.
    pub work_item_id: Uuid,
    /// Coherent item version for this read.
    pub work_version: i64,
    /// Parent, only when its own source remains visible.
    pub parent: Option<WorkSummary>,
    /// At most 32 visible children; hidden endpoints are omitted entirely.
    pub children: Vec<WorkSummary>,
}
impl AuthorizedWork {
    /// Read structural links without inheriting their source or content authority.
    pub async fn decomposition(&self, id: Uuid) -> Result<WorkDecomposition> {
        bounded(async {
            let (mut tx, deadline) = self.begin().await?;
            let (project, item) = self.item_on(&mut tx, id, false).await?;
            let mut q = sqlx::QueryBuilder::new("SELECT w.id,w.project_id,w.title,w.priority,w.state,w.version,w.source_message_id,w.created_at,w.updated_at,(d.child_id=$4) AS is_parent FROM work_decomposition d JOIN work_items w ON w.company_id=d.company_id AND w.project_id=d.project_id AND w.id=CASE WHEN d.child_id=$4 THEN d.parent_id ELSE d.child_id END WHERE d.company_id=$1 AND (d.child_id=$4 OR d.parent_id=$4) AND ");
            q.push(authority::SOURCE_VISIBLE).push(" ORDER BY w.id LIMIT 34 FOR SHARE OF d,w");
            let rows = q.build().bind(self.scope.company_id()).bind(self.principal.community_id)
                .bind(project.channel_id).bind(id).fetch_all(&mut *tx).await?;
            if rows.len() > MAX_WORK_CHILDREN + 1 { return Err(invalid("decomposition exceeds its bound")); }
            let mut parent = None;
            let mut children = Vec::new();
            for row in rows {
                let summary = summary_from_row(&row)?;
                if row.try_get::<bool,_>("is_parent")? { parent=Some(summary); }
                else { children.push(summary); }
            }
            self.finish(tx,deadline).await?;
            Ok(WorkDecomposition {work_item_id:id,work_version:item.item.version,parent,children})
        }).await
    }

    /// Create one independent child and parent history under one signed receipt.
    pub async fn create_child(
        &self,
        operation: Uuid,
        parent: Uuid,
        expected_version: i64,
        definition: NewChildWork,
    ) -> Result<WorkChildCreation> {
        bounded(self.create_child_inner(operation, parent, expected_version, definition)).await
    }
    async fn create_child_inner(
        &self,
        operation: Uuid,
        parent: Uuid,
        expected_version: i64,
        definition: NewChildWork,
    ) -> Result<WorkChildCreation> {
        definition.validate()?;
        if parent.is_nil() || expected_version < 1 {
            return Err(WorkError::InvalidQuery(
                "invalid decomposition target or version",
            ));
        }
        let hash = fingerprint(("create_child", parent, expected_version, &definition))?;
        let (mut tx, deadline) = self.begin().await?;
        let receipt = self
            .operation_on(&mut tx, operation, "create_work_item", &hash)
            .await?;
        let project: Uuid = sqlx::query_scalar(ITEM_PROJECT_SQL)
            .bind(self.scope.company_id())
            .bind(parent)
            .fetch_optional(&mut *tx)
            .await?
            .ok_or(WorkError::WorkItemNotFound {
                work_item_id: parent,
            })?;
        // Graph edits never upgrade SHARE to EXCLUSIVE: project X comes first.
        sqlx::query(PROJECT_FOR_UPDATE_SQL)
            .bind(self.scope.company_id())
            .bind(project)
            .fetch_optional(&mut *tx)
            .await?
            .ok_or(WorkError::ProjectNotFound {
                project_id: project,
            })?;
        let (authorized, parent_record) = self.item_on(&mut tx, parent, true).await?;
        self.contribute(authorized.role)?;
        if let Some(receipt) = receipt {
            let child = receipt.work_item_id.ok_or(WorkError::OperationConflict)?;
            let linked:bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM work_decomposition WHERE company_id=$1 AND project_id=$2 AND parent_id=$3 AND child_id=$4 AND operation_id=$5 AND actor_pubkey=$6 AND parent_version=$7)")
                .bind(self.scope.company_id()).bind(project).bind(parent).bind(child).bind(operation)
                .bind(&self.principal.public_key).bind(expected_version.checked_add(1)).fetch_one(&mut *tx).await?;
            if receipt.project_id != project || receipt.result_version != 1 || !linked {
                return Err(WorkError::OperationConflict);
            }
            let (_, child) = self.item_on(&mut tx, child, false).await?;
            self.finish(tx, deadline).await?;
            return Ok(WorkChildCreation {
                parent: parent_record,
                child,
                created: false,
            });
        }
        let mut item = lock_item(
            &mut tx,
            &self.scope,
            parent,
            expected_version,
            ProjectLock::Exclusive,
        )
        .await?;
        let child = Uuid::new_v4();
        let event = item.record_child_created(child)?;
        let depth:i16 = sqlx::query_scalar("SELECT coalesce((SELECT depth FROM work_decomposition WHERE company_id=$1 AND child_id=$2),0)::smallint")
            .bind(self.scope.company_id()).bind(parent).fetch_one(&mut *tx).await?;
        let count: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM work_decomposition WHERE company_id=$1 AND parent_id=$2",
        )
        .bind(self.scope.company_id())
        .bind(parent)
        .fetch_one(&mut *tx)
        .await?;
        if depth >= MAX_WORK_DEPTH || count >= MAX_WORK_CHILDREN as i64 {
            return Err(WorkError::InvalidQuery("decomposition limit reached"));
        }
        sqlx::query("INSERT INTO work_decomposition(company_id,project_id,parent_id,child_id,parent_version,depth,actor_pubkey,operation_id) VALUES($1,$2,$3,$4,$5,$6,$7,$8)")
            .bind(self.scope.company_id()).bind(project).bind(parent).bind(child).bind(item.version).bind(depth+1)
            .bind(&self.principal.public_key).bind(operation).execute(&mut *tx).await?;
        let created = creation::create_manual_child_on(
            &mut tx,
            &self.scope,
            &CreateWorkItem {
                input: definition.into_item(project),
                actor: self.actor(),
            },
            child,
        )
        .await?;
        if !created.created || created.item.item.id != child {
            return Err(invalid("child reservation differs from creation"));
        }
        persist_event(
            &mut tx,
            &self.scope,
            &item,
            expected_version,
            &self.actor(),
            &event,
        )
        .await?;
        self.record_on(
            &mut tx,
            operation,
            "create_work_item",
            &hash,
            project,
            Some(child),
            1,
            deadline,
        )
        .await?;
        let parent = require_aggregate(&mut tx, &self.scope, parent).await?;
        self.finish(tx, deadline).await?;
        Ok(WorkChildCreation {
            parent,
            child: created.item,
            created: true,
        })
    }
}
