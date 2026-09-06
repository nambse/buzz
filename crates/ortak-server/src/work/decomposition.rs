//! Current structural navigation and one atomic, independently defined child.
use super::*;
use axum::{
    extract::{rejection::JsonRejection, Path, State},
    http::StatusCode,
    Extension, Json,
};
use ortak_domain::NewChildWork;
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Create {
    operation_id: Uuid,
    expected_version: i64,
    child: NewChildWork,
}
pub(super) async fn list(
    State(state): State<ApiState>,
    Extension(p): Extension<Principal>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>> {
    let page = authorized(&state, &p)?.decomposition(id).await?;
    projection::bounded(
        json!({"work_item_id":page.work_item_id,"work_version":page.work_version,
        "parent":page.parent.as_ref().map(projection::summary),
        "children":page.children.iter().map(projection::summary).collect::<Vec<_>>()}),
    )
}
pub(super) async fn create(
    State(state): State<ApiState>,
    Extension(p): Extension<Principal>,
    Path(id): Path<Uuid>,
    body: std::result::Result<Json<Create>, JsonRejection>,
) -> Result<(StatusCode, Json<Value>)> {
    let Json(body) = body.map_err(|_| ApiError::invalid())?;
    let result = authorized(&state, &p)?
        .create_child(body.operation_id, id, body.expected_version, body.child)
        .await?;
    Ok((
        if result.created {
            StatusCode::CREATED
        } else {
            StatusCode::OK
        },
        projection::bounded(json!({"work_item":projection::item(&result.parent,&p),
            "child":projection::item(&result.child,&p),"created":result.created}))?,
    ))
}
