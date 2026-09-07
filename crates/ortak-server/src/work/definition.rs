//! One authorized, idempotent manual definition amendment.
use crate::{
    auth::Principal,
    error::{ApiError, Result},
    routes::ApiState,
};
use axum::{
    extract::{rejection::JsonRejection, Path, State},
    Extension, Json,
};
use ortak_domain::EditWorkDefinition;
use ortak_work::WorkMutation;
use serde::Deserialize;
use serde_json::Value;
use uuid::Uuid;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct DefinitionRequest {
    operation_id: Uuid,
    expected_version: i64,
    definition: EditWorkDefinition,
}
pub(super) async fn edit(
    State(state): State<ApiState>,
    Extension(principal): Extension<Principal>,
    Path(id): Path<Uuid>,
    body: std::result::Result<Json<DefinitionRequest>, JsonRejection>,
) -> Result<Json<Value>> {
    let Json(body) = body.map_err(|_| ApiError::invalid())?;
    super::routes::mutate(
        &state,
        &principal,
        body.operation_id,
        id,
        body.expected_version,
        WorkMutation::EditDefinition {
            definition: body.definition,
        },
    )
    .await
}
