//! Explicit Work execution and current-authority reads of its retained evidence.
use super::*;
use axum::{
    extract::{rejection::JsonRejection, Path, State},
    Extension, Json,
};
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ExecutionRequest {
    operation_id: Uuid,
    expected_version: i64,
    employee_id: ortak_domain::EmployeeId,
}
pub(super) async fn start(
    State(state): State<ApiState>,
    Extension(p): Extension<Principal>,
    Path(id): Path<Uuid>,
    body: std::result::Result<Json<ExecutionRequest>, JsonRejection>,
) -> Result<Json<Value>> {
    let Json(body) = body.map_err(|_| ApiError::invalid())?;
    let receipt = authorized(&state, &p)?
        .request_execution(
            body.operation_id,
            id,
            body.expected_version,
            body.employee_id,
        )
        .await?;
    projection::bounded(json!({"execution":receipt}))
}
pub(super) async fn list(
    State(state): State<ApiState>,
    Extension(p): Extension<Principal>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>> {
    projection::bounded(json!({"executions":authorized(&state,&p)?.executions(id).await?}))
}
pub(super) async fn artifact(
    State(state): State<ApiState>,
    Extension(p): Extension<Principal>,
    Path((item, id)): Path<(Uuid, Uuid)>,
) -> Result<axum::response::Response> {
    use axum::response::IntoResponse;
    let artifact = authorized(&state, &p)?.text_artifact(item, id).await?;
    Ok((
        [
            (
                axum::http::header::CONTENT_TYPE,
                "text/plain; charset=utf-8",
            ),
            (axum::http::header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
            (axum::http::header::CACHE_CONTROL, "no-store"),
        ],
        artifact.content,
    )
        .into_response())
}
