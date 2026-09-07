//! Explicit conversation review through the signed Work authority facade.

use super::*;
use axum::{
    extract::{
        rejection::{JsonRejection, QueryRejection},
        Path, Query, State,
    },
    Extension, Json,
};
use ortak_domain::EmployeeId;
use ortak_work::{ReviewedConversationFactDraft, ReviewedConversationPreviewRequest};
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Approval {
    operation_id: Uuid,
    fact: ReviewedConversationFactDraft,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Revocation {
    operation_id: Uuid,
    expected_version: i64,
    reason: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Page {
    employee_id: EmployeeId,
    after: Option<Uuid>,
}

pub(super) async fn preview(
    State(state): State<ApiState>,
    Extension(principal): Extension<Principal>,
    Path(project): Path<Uuid>,
    body: std::result::Result<Json<ReviewedConversationPreviewRequest>, JsonRejection>,
) -> Result<Json<Value>> {
    let Json(request) = body.map_err(|_| ApiError::invalid())?;
    let preview = authorized(&state, &principal)?
        .preview_conversation_memory(project, request)
        .await?;
    projection::bounded(json!({"preview": preview}))
}

pub(super) async fn approve(
    State(state): State<ApiState>,
    Extension(principal): Extension<Principal>,
    Path(project): Path<Uuid>,
    body: std::result::Result<Json<Approval>, JsonRejection>,
) -> Result<Json<Value>> {
    let Json(body) = body.map_err(|_| ApiError::invalid())?;
    let receipt = authorized(&state, &principal)?
        .promote_conversation_fact(body.operation_id, project, body.fact)
        .await?;
    projection::bounded(json!(receipt))
}

pub(super) async fn list(
    State(state): State<ApiState>,
    Extension(principal): Extension<Principal>,
    Path(project): Path<Uuid>,
    query: std::result::Result<Query<Page>, QueryRejection>,
) -> Result<Json<Value>> {
    let Query(query) = query.map_err(|_| ApiError::invalid())?;
    let page = authorized(&state, &principal)?
        .conversation_facts(project, query.employee_id, query.after)
        .await?;
    projection::bounded(json!(page))
}

pub(super) async fn revoke(
    State(state): State<ApiState>,
    Extension(principal): Extension<Principal>,
    Path((project, fact)): Path<(Uuid, Uuid)>,
    body: std::result::Result<Json<Revocation>, JsonRejection>,
) -> Result<Json<Value>> {
    let Json(body) = body.map_err(|_| ApiError::invalid())?;
    let receipt = authorized(&state, &principal)?
        .revoke_conversation_fact(
            body.operation_id,
            project,
            fact,
            body.expected_version,
            body.reason,
        )
        .await?;
    projection::bounded(json!(receipt))
}
