//! Explicit human review and retained stop-use operations for project context.
use super::*;
use axum::{
    extract::{
        rejection::{JsonRejection, QueryRejection},
        Path, Query, State,
    },
    Extension, Json,
};
use ortak_domain::EmployeeId;
use ortak_work::ReviewedFactDraft;
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

type Body<T> = std::result::Result<Json<T>, JsonRejection>;
type Params<T> = std::result::Result<Query<T>, QueryRejection>;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Approval {
    operation_id: Uuid,
    fact: ReviewedFactDraft,
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
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Recall {
    employee_id: EmployeeId,
    query: String,
}

pub(super) async fn approve(
    State(state): State<ApiState>,
    Extension(p): Extension<Principal>,
    Path(project): Path<Uuid>,
    body: Body<Approval>,
) -> Result<Json<Value>> {
    let Json(body) = body.map_err(|_| ApiError::invalid())?;
    let receipt = authorized(&state, &p)?
        .promote_reviewed_fact(body.operation_id, project, body.fact)
        .await?;
    projection::bounded(json!(receipt))
}

pub(super) async fn revoke(
    State(state): State<ApiState>,
    Extension(p): Extension<Principal>,
    Path((project, fact)): Path<(Uuid, Uuid)>,
    body: Body<Revocation>,
) -> Result<Json<Value>> {
    let Json(body) = body.map_err(|_| ApiError::invalid())?;
    let receipt = authorized(&state, &p)?
        .revoke_reviewed_fact(
            body.operation_id,
            project,
            fact,
            body.expected_version,
            body.reason,
        )
        .await?;
    projection::bounded(json!(receipt))
}

pub(super) async fn list(
    State(state): State<ApiState>,
    Extension(p): Extension<Principal>,
    Path(project): Path<Uuid>,
    query: Params<Page>,
) -> Result<Json<Value>> {
    let Query(query) = query.map_err(|_| ApiError::invalid())?;
    let page = authorized(&state, &p)?
        .reviewed_facts(project, query.employee_id, query.after)
        .await?;
    projection::bounded(json!(page))
}

pub(super) async fn recall(
    State(state): State<ApiState>,
    Extension(p): Extension<Principal>,
    Path(project): Path<Uuid>,
    body: Body<Recall>,
) -> Result<Json<Value>> {
    let Json(body) = body.map_err(|_| ApiError::invalid())?;
    let result = authorized(&state, &p)?
        .recall_reviewed_facts(project, body.employee_id, body.query)
        .await?;
    projection::bounded(json!(result))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn approval_requires_explicit_review_and_rejects_authority_injection() {
        let body = json!({"operation_id":Uuid::new_v4(),"fact":{
            "employee_id":"cem","source":{"kind":"conversation","message_id":"ab".repeat(32)},
            "content":"Reviewed fact","expires_at":"2026-09-06T12:00:00Z","reviewed":true}});
        assert!(serde_json::from_value::<Approval>(body.clone()).is_ok());
        for field in ["company_id", "approved_by", "scope"] {
            let mut bad = body.clone();
            bad["fact"][field] = json!("injected");
            assert!(serde_json::from_value::<Approval>(bad).is_err());
        }
        let mut missing = body.clone();
        missing["fact"].as_object_mut().unwrap().remove("reviewed");
        assert!(serde_json::from_value::<Approval>(missing).is_err());
        let mut source = body;
        source["fact"]["source"]["raw_output"] = json!("unapproved");
        assert!(serde_json::from_value::<Approval>(source).is_err());
    }
}
