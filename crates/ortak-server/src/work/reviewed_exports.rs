//! Opaque human commands; selected Honcho identities never come from API payloads.
use super::*;
use axum::{
    extract::{rejection::JsonRejection, Path, State},
    Extension, Json,
};
use ortak_work::reviewed_exports::ReviewedExportAction;
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;
type Body<T> = std::result::Result<Json<T>, JsonRejection>;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Publication {
    operation_id: Uuid,
    expected_version: i64,
    confirmed: bool,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Retry {
    operation_id: Uuid,
    retry_version: i32,
}
pub(super) async fn publish(
    State(state): State<ApiState>,
    Extension(p): Extension<Principal>,
    Path((project, fact)): Path<(Uuid, Uuid)>,
    body: Body<Publication>,
) -> Result<Json<Value>> {
    let Json(body) = body.map_err(|_| ApiError::invalid())?;
    let result = authorized(&state, &p)?
        .publish_reviewed_fact(
            body.operation_id,
            project,
            fact,
            body.expected_version,
            body.confirmed,
        )
        .await?;
    projection::bounded(json!({"export":result}))
}
pub(super) async fn retry(
    State(state): State<ApiState>,
    Extension(p): Extension<Principal>,
    Path((project, fact, action)): Path<(Uuid, Uuid, ReviewedExportAction)>,
    body: Body<Retry>,
) -> Result<Json<Value>> {
    let Json(body) = body.map_err(|_| ApiError::invalid())?;
    let result = authorized(&state, &p)?
        .retry_reviewed_export(body.operation_id, project, fact, action, body.retry_version)
        .await?;
    projection::bounded(json!({"export":result}))
}
pub(super) async fn publish_conversation(
    State(state): State<ApiState>,
    Extension(p): Extension<Principal>,
    Path((project, fact)): Path<(Uuid, Uuid)>,
    body: Body<Publication>,
) -> Result<Json<Value>> {
    let Json(body) = body.map_err(|_| ApiError::invalid())?;
    let result = authorized(&state, &p)?
        .publish_reviewed_conversation_fact(
            body.operation_id,
            project,
            fact,
            body.expected_version,
            body.confirmed,
        )
        .await?;
    projection::bounded(json!({"export":result}))
}

pub(super) async fn retry_conversation(
    State(state): State<ApiState>,
    Extension(p): Extension<Principal>,
    Path((project, fact, action)): Path<(Uuid, Uuid, ReviewedExportAction)>,
    body: Body<Retry>,
) -> Result<Json<Value>> {
    let Json(body) = body.map_err(|_| ApiError::invalid())?;
    let result = authorized(&state, &p)?
        .retry_reviewed_conversation_export(
            body.operation_id,
            project,
            fact,
            action,
            body.retry_version,
        )
        .await?;
    projection::bounded(json!({"export":result}))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn publication_requires_explicit_confirmation_and_rejects_remote_authority_fields() {
        let body = json!({"operation_id":Uuid::new_v4(),"expected_version":1,"confirmed":true});
        assert!(serde_json::from_value::<Publication>(body.clone()).is_ok());
        for field in [
            "company_id",
            "employee_id",
            "target_id",
            "binding",
            "creation_receipt",
            "request_hash",
            "content",
        ] {
            let mut bad = body.clone();
            bad[field] = json!("injected");
            assert!(serde_json::from_value::<Publication>(bad).is_err());
        }
        let mut missing = body;
        missing.as_object_mut().unwrap().remove("confirmed");
        assert!(serde_json::from_value::<Publication>(missing).is_err());
        assert!(serde_json::from_value::<Retry>(
            json!({"operation_id":Uuid::new_v4(),"retry_version":0,"idempotency_key":"different"})
        )
        .is_err());
    }
}
