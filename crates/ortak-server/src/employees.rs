use axum::{
    extract::{Path, Query, State},
    Extension, Json,
};
use ortak_domain::{EmployeeId, EmployeeStatus};
use ortak_observability::{projection::bound_row_text, RunListQuery};
use serde::Deserialize;
use sqlx::Row;
use uuid::Uuid;

use crate::{
    auth::{Principal, RequestAuthority},
    error::{ApiError, Result},
    routes::ApiState,
};

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct EmployeeQuery {
    after: Option<EmployeeId>,
    limit: Option<u32>,
}

pub(crate) async fn list(
    State(state): State<ApiState>,
    Extension(principal): Extension<Principal>,
    Query(query): Query<EmployeeQuery>,
) -> Result<Json<serde_json::Value>> {
    let ids = principal
        .grant
        .employee_ids
        .iter()
        .map(|id| id.as_str())
        .collect::<Vec<_>>();
    let limit = query.limit.unwrap_or(25).clamp(1, 25);
    let rows = sqlx::query("SELECT e.id, e.status, e.active_revision_id,
            r.manifest->>'name' AS name, r.manifest->>'title' AS title
          FROM employees e LEFT JOIN employee_revisions r ON r.company_id = e.company_id AND r.id = e.active_revision_id
          WHERE e.company_id = $1 AND e.id = ANY($2) AND ($3::text IS NULL OR e.id > $3)
          ORDER BY e.id LIMIT $4")
        .bind(principal.scope.company_id()).bind(ids).bind(query.after.as_ref().map(|id| id.as_str()))
        .bind(i64::from(limit) + 1).fetch_all(state.control.pool()).await?;
    let has_more = rows.len() > limit as usize;
    let employees = rows
        .iter()
        .take(limit as usize)
        .map(employee_json)
        .collect::<Result<Vec<_>>>()?;
    let next_after = if has_more {
        employees.last().and_then(|e| e.get("employee_id")).cloned()
    } else {
        None
    };
    Ok(Json(
        serde_json::json!({"employees": employees, "has_more": has_more, "next_after": next_after, "can_view_provisioning": principal.grant.can_manage_employees && principal.grant.role == crate::Role::Operator,
            "can_execute_provisioning":principal.grant.can_execute_provisioning && principal.grant.can_manage_employees && principal.grant.role==crate::Role::Operator}),
    ))
}

pub(crate) async fn detail(
    State(state): State<ApiState>,
    Extension(principal): Extension<Principal>,
    Path(employee_id): Path<EmployeeId>,
    Extension(authority): Extension<RequestAuthority>,
) -> Result<Json<serde_json::Value>> {
    if !principal.grant.employee_ids.contains(&employee_id) {
        state
            .audit_principal(&principal, "read_employee", "not_found", None)
            .await?;
        return Err(ApiError::not_found());
    }
    let row = sqlx::query("SELECT e.id, e.status, e.active_revision_id,
            r.manifest->>'name' AS name, r.manifest->>'title' AS title
          FROM employees e LEFT JOIN employee_revisions r ON r.company_id = e.company_id AND r.id = e.active_revision_id
          WHERE e.company_id = $1 AND e.id = $2")
        .bind(principal.scope.company_id()).bind(employee_id.as_str())
        .fetch_optional(state.control.pool()).await?.ok_or_else(ApiError::not_found)?;
    let current = state
        .visible_runs(
            &principal,
            &RunListQuery {
                employee_id: Some(employee_id),
                statuses: vec![
                    ortak_observability::RunStatus::Queued,
                    ortak_observability::RunStatus::Running,
                    ortak_observability::RunStatus::Waiting,
                ],
                limit: Some(1),
                ..RunListQuery::default()
            },
            &authority,
        )
        .await?;
    Ok(Json(
        serde_json::json!({"employee": employee_json(&row)?, "current_run": current.runs.first(), "has_other_current_runs": current.has_more,
        "runtime_health": "not_probed", "permission_enforcement": "not_verified_by_api"}),
    ))
}

fn employee_json(row: &sqlx::postgres::PgRow) -> Result<serde_json::Value> {
    let id: String = row.try_get("id")?;
    let status: String = row.try_get("status")?;
    let status: EmployeeStatus = serde_json::from_value(serde_json::Value::String(status))
        .map_err(|_| ApiError::unavailable())?;
    let name: Option<String> = row.try_get("name")?;
    let title: Option<String> = row.try_get("title")?;
    let revision: Option<Uuid> = row.try_get("active_revision_id")?;
    Ok(
        serde_json::json!({"employee_id": id, "name": name.map(|v| bound_row_text(&v, 128)),
        "title": title.map(|v| bound_row_text(&v, 256)), "status": status, "active_revision_id": revision}),
    )
}
