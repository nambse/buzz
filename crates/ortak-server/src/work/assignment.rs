//! Closed, actor-free assignment changes through the same authorized mutation journal.
use super::*;
use axum::{
    extract::{rejection::JsonRejection, Path, State},
    Extension, Json,
};
use ortak_domain::{AssignmentRole, EmployeeId};
use ortak_work::WorkMutation;
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Release {
    operation_id: Uuid,
    expected_version: i64,
    reason: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Reassign {
    operation_id: Uuid,
    expected_version: i64,
    replacement_employee_id: EmployeeId,
    role: AssignmentRole,
    reason: String,
}
pub(super) async fn release(
    State(state): State<ApiState>,
    Extension(p): Extension<Principal>,
    Path((id, employee_id)): Path<(Uuid, EmployeeId)>,
    body: std::result::Result<Json<Release>, JsonRejection>,
) -> Result<Json<Value>> {
    let Json(body) = body.map_err(|_| ApiError::invalid())?;
    let result = authorized(&state, &p)?
        .mutate(
            body.operation_id,
            id,
            body.expected_version,
            WorkMutation::ReleaseAssignment {
                employee_id,
                reason: body.reason,
            },
        )
        .await?;
    projection::bounded(json!({"work_item":projection::item(&result, &p)}))
}
pub(super) async fn reassign(
    State(state): State<ApiState>,
    Extension(p): Extension<Principal>,
    Path((id, employee_id)): Path<(Uuid, EmployeeId)>,
    body: std::result::Result<Json<Reassign>, JsonRejection>,
) -> Result<Json<Value>> {
    let Json(body) = body.map_err(|_| ApiError::invalid())?;
    let result = authorized(&state, &p)?
        .mutate(
            body.operation_id,
            id,
            body.expected_version,
            WorkMutation::Reassign {
                employee_id,
                replacement_employee_id: body.replacement_employee_id,
                role: body.role,
                reason: body.reason,
            },
        )
        .await?;
    projection::bounded(json!({"work_item":projection::item(&result, &p)}))
}
