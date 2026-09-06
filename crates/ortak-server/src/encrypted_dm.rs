//! Fresh signed public pair observations for the purpose-specific native view.
use axum::{
    extract::{Path, State},
    Extension, Json,
};
use ortak_domain::EmployeeId;
use ortak_runtime::postgres::confidential::{
    ConfidentialAdmissionError, EncryptedDmAuthority, PgConfidentialRuns,
};
use uuid::Uuid;

use crate::{
    auth::{Principal, RequestAuthority},
    error::{ApiError, Result},
    routes::ApiState,
};

pub(super) async fn authority(
    State(state): State<ApiState>,
    Extension(principal): Extension<Principal>,
    Extension(fence): Extension<RequestAuthority>,
    Path(channel): Path<Uuid>,
) -> Result<Json<EncryptedDmAuthority>> {
    // Middleware retains the current human's Office fence until serialization.
    // The repository takes a second shared observation on the bounded API pool.
    // No public-key parameter or client-supplied identity can replace the signer.
    let held = fence.0.lock().await;
    if held.is_none() {
        return Err(ApiError::unavailable());
    }
    if !principal.grant.channel_ids.contains(&channel) {
        return Err(ApiError::not_found());
    }
    let current = PgConfidentialRuns::new(state.control.pool().clone())
        .authority(&principal.scope, channel, &principal.public_key.to_bytes())
        .await
        .map_err(|error| match error {
            ConfidentialAdmissionError::Unavailable => ApiError::unavailable(),
            _ => ApiError::not_found(),
        })?
        .ok_or_else(ApiError::not_found)?;
    let employee = EmployeeId::parse(&current.employee_id).map_err(|_| ApiError::unavailable())?;
    if !principal.grant.employee_ids.contains(&employee)
        || current.community_id != state.config.community_id
        || current.company_id != principal.scope.company_id()
        || current.valid_before <= chrono::Utc::now()
    {
        return Err(ApiError::not_found());
    }
    Ok(Json(current))
}
