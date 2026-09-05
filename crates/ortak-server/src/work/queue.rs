//! Signed, read-only manual employee queue. Project permissions never imply execution access.
use super::{authorized, dto::PageQuery, projection};
use crate::{
    auth::Principal,
    error::{ApiError, Result},
    routes::ApiState,
};
use axum::{
    extract::{rejection::QueryRejection, Path, Query, State},
    Extension, Json,
};
use ortak_domain::EmployeeId;
use serde_json::{json, Value};

pub(super) async fn employee_queue(
    State(state): State<ApiState>,
    Extension(principal): Extension<Principal>,
    Path(employee): Path<EmployeeId>,
    query: std::result::Result<Query<PageQuery>, QueryRejection>,
) -> Result<Json<Value>> {
    let Query(query) = query.map_err(|_| ApiError::invalid())?;
    let page = authorized(&state, &principal)?
        .employee_queue(
            &employee,
            query.cursor.as_deref(),
            query.limit.unwrap_or(25),
        )
        .await?;
    let items: Vec<_> = page
        .items
        .iter()
        .map(|entry| {
            let mut item = projection::summary(&entry.work);
            item["assignment_role"] = json!(entry.assignment_role);
            item
        })
        .collect();
    projection::bounded(
        json!({"employee_id":page.employee_id,"work_items":items,"next_cursor":page.next_cursor,"execution_available":false}),
    )
}
