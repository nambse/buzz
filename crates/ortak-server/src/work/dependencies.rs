//! Current dependency read and explicit graph mutation endpoints.
use super::*;
use axum::{
    extract::{rejection::JsonRejection, Path, State},
    Extension, Json,
};
use ortak_work::DependencyAction;
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Add {
    operation_id: Uuid,
    expected_version: i64,
    depends_on: Uuid,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Remove {
    operation_id: Uuid,
    expected_version: i64,
    reason: String,
}

pub(super) async fn list(
    State(state): State<ApiState>,
    Extension(p): Extension<Principal>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>> {
    let page = authorized(&state, &p)?.dependencies(id).await?;
    let entries: Vec<_> = page
        .dependencies
        .iter()
        .map(|edge| json!({"id":edge.id,"target":edge.target.as_ref().map(projection::summary)}))
        .collect();
    projection::bounded(
        json!({"work_item_id":page.work_item_id,"work_version":page.work_version,"dependencies":entries}),
    )
}
pub(super) async fn add(
    State(state): State<ApiState>,
    Extension(p): Extension<Principal>,
    Path(id): Path<Uuid>,
    body: std::result::Result<Json<Add>, JsonRejection>,
) -> Result<Json<Value>> {
    let Json(body) = body.map_err(|_| ApiError::invalid())?;
    let result = authorized(&state, &p)?
        .mutate_dependency(
            body.operation_id,
            id,
            body.expected_version,
            DependencyAction::Add {
                depends_on: body.depends_on,
            },
        )
        .await?;
    projection::bounded(json!({"work_item":projection::item(&result,&p)}))
}
pub(super) async fn remove(
    State(state): State<ApiState>,
    Extension(p): Extension<Principal>,
    Path((id, dependency_id)): Path<(Uuid, Uuid)>,
    body: std::result::Result<Json<Remove>, JsonRejection>,
) -> Result<Json<Value>> {
    let Json(body) = body.map_err(|_| ApiError::invalid())?;
    let result = authorized(&state, &p)?
        .mutate_dependency(
            body.operation_id,
            id,
            body.expected_version,
            DependencyAction::Remove {
                dependency_id,
                reason: body.reason,
            },
        )
        .await?;
    projection::bounded(json!({"work_item":projection::item(&result,&p)}))
}
