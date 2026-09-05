//! Read-only manual assignment queue with the same project and source audience as Work.
use super::*;
mod cursor;
mod sql;
mod types;
pub use types::*;

impl AuthorizedWork {
    /// List outstanding manual assignments for one configured employee.
    /// Includes every active assignment role in active projects, excluding completed
    /// and cancelled work. An inactive employee remains inspectable; this is not an
    /// execution-readiness or dispatch queue. Newest work is first, at most 25 rows.
    /// A cursor is bound to the employee, principal, configured audiences, and company.
    pub async fn employee_queue(
        &self,
        employee: &EmployeeId,
        cursor: Option<&str>,
        limit: u32,
    ) -> Result<EmployeeWorkQueuePage> {
        bounded(self.employee_queue_inner(employee, cursor, limit)).await
    }
    async fn employee_queue_inner(
        &self,
        employee: &EmployeeId,
        cursor: Option<&str>,
        limit: u32,
    ) -> Result<EmployeeWorkQueuePage> {
        let missing = || WorkError::EmployeeNotFound {
            employee_id: employee.clone(),
        };
        if !self.principal.employee_ids.contains(employee) {
            return Err(missing());
        }
        let context = self.queue_context(employee)?;
        let cursor = cursor::decode(cursor, &context)?;
        let limit = limit.clamp(1, 25);
        let (mut tx, deadline) = self.begin().await?;
        let exists: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM employees WHERE company_id=$1 AND id=$2)",
        )
        .bind(self.scope.company_id())
        .bind(employee.as_str())
        .fetch_one(&mut *tx)
        .await?;
        if !exists {
            return Err(missing());
        }
        let candidates = self
            .queue_rows(&mut tx, employee, cursor, limit, None)
            .await?;
        // Take all parent fences before item/assignment locks. A fresh project_on
        // check after each lock defeats a grant snapshot read before a writer commits.
        let projects = candidates
            .iter()
            .map(|r| r.try_get::<Uuid, _>("project_id"))
            .collect::<std::result::Result<BTreeSet<_>, _>>()?;
        for project in projects {
            self.project_on(&mut tx, project).await?;
        }
        let ids = candidates
            .iter()
            .map(|r| r.try_get::<Uuid, _>("id"))
            .collect::<std::result::Result<BTreeSet<_>, _>>()?;
        for id in &ids {
            sqlx::query("SELECT id FROM work_items WHERE company_id=$1 AND id=$2 FOR SHARE")
                .bind(self.scope.company_id())
                .bind(id)
                .fetch_optional(&mut *tx)
                .await?
                .ok_or(WorkError::OperationConflict)?;
        }
        // The retained assignment table also permits direct status updates; pin
        // those rows explicitly, not merely their usual repository parent lock.
        for id in &ids {
            sqlx::query("SELECT employee_id FROM work_assignments WHERE company_id=$1 AND work_item_id=$2 AND employee_id=$3 FOR SHARE")
                .bind(self.scope.company_id()).bind(id).bind(employee.as_str()).fetch_optional(&mut *tx).await?.ok_or(WorkError::OperationConflict)?;
        }
        let ids: Vec<_> = ids.into_iter().collect();
        let current = self
            .queue_rows(&mut tx, employee, cursor, limit, Some(&ids))
            .await?;
        // Never return a stale/partially filtered page after a concurrent release,
        // terminal transition, archive, or access change. The caller can reread.
        if current.len() != candidates.len() {
            return Err(WorkError::OperationConflict);
        }
        let mut items = Vec::new();
        for row in current.iter().take(limit as usize) {
            let role: String = row.try_get("assignment_role")?;
            items.push(EmployeeWorkQueueEntry {
                work: summary_from_row(row)?,
                assignment_role: AssignmentRole::parse(&role)
                    .ok_or_else(|| invalid("invalid assignment role"))?,
            });
        }
        let next_cursor = if current.len() > limit as usize {
            items
                .last()
                .map(|item| cursor::encode(&context, &item.work))
        } else {
            None
        };
        self.finish(tx, deadline).await?;
        Ok(EmployeeWorkQueuePage {
            employee_id: employee.clone(),
            items,
            next_cursor,
        })
    }
}
