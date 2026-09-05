//! Shared transaction implementation; never exported outside this crate.

use super::*;

pub(super) async fn project_on(
    connection: &mut PgConnection,
    scope: &CompanyScope,
    project_id: Uuid,
) -> Result<ProjectRecord> {
    let row = sqlx::query(PROJECT_SQL)
        .bind(scope.company_id())
        .bind(project_id)
        .fetch_optional(&mut *connection)
        .await?
        .ok_or(WorkError::ProjectNotFound { project_id })?;
    project_record(&row)
}

pub(super) async fn work_item_on(
    connection: &mut PgConnection,
    scope: &CompanyScope,
    work_item_id: Uuid,
) -> Result<WorkItemAggregate> {
    require_aggregate(&mut *connection, scope, work_item_id).await
}

pub(super) async fn list_project_work_on(
    connection: &mut PgConnection,
    scope: &CompanyScope,
    project_id: Uuid,
    query: &WorkListQuery,
) -> Result<WorkListPage> {
    // Fail closed for unknown and cross-company projects, even when the
    // project would have no work.
    sqlx::query("SELECT 1 FROM projects WHERE company_id = $1 AND id = $2")
        .bind(scope.company_id())
        .bind(project_id)
        .fetch_optional(&mut *connection)
        .await?
        .ok_or(WorkError::ProjectNotFound { project_id })?;
    let page_size = query.page_size();
    let rows = sqlx::query(LIST_SQL)
        .bind(scope.company_id())
        .bind(project_id)
        .bind(query.state_filter())
        .bind(query.cursor.map(|cursor| cursor.created_at()))
        .bind(query.cursor.map(|cursor| cursor.id()))
        .bind(i64::from(page_size) + 1)
        .fetch_all(&mut *connection)
        .await?;
    let mut items = rows
        .iter()
        .map(summary_from_row)
        .collect::<Result<Vec<_>>>()?;
    let next_cursor = if items.len() > page_size as usize {
        items.truncate(page_size as usize);
        items.last().map(WorkListCursor::after)
    } else {
        None
    };
    Ok(WorkListPage { items, next_cursor })
}
