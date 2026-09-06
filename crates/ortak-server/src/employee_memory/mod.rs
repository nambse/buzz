//! Signed employee-owned review. This private facade is the authentication
//! boundary; SQL checks current data, not possession of a signature or grant.
//! No publication, target ownership, runtime use or project fallback is exposed.

use axum::{
    extract::{
        rejection::{JsonRejection, QueryRejection},
        Path, Query, State,
    },
    http::StatusCode,
    routing::{get, post},
    Extension, Json, Router,
};
use chrono::{DateTime, Utc};
use ortak_control::{
    memory::employee::*, office_identity::OfficePublicKey, CompanyScope, MessageId,
};
use ortak_domain::EmployeeId;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use sqlx::{PgConnection, Postgres, Row, Transaction};
use uuid::Uuid;

use crate::{
    auth::Principal,
    error::{ApiError, Result},
    routes::ApiState,
};
use types::*;

mod authority;
mod operations;
mod reads;
mod source;
mod types;
mod wire;

// No public constructor, Deserialize implementation, caller-supplied actor or
// SQL session grant. Only this module's signed handlers construct the facade.
struct Review<'a> {
    state: &'a ApiState,
    principal: &'a Principal,
    employee: EmployeeId,
}

pub(crate) fn router() -> Router<ApiState> {
    Router::new()
        .route(
            "/api/v1/employees/{employee_id}/reviewed-memory",
            get(list).post(approve),
        )
        .route(
            "/api/v1/employees/{employee_id}/reviewed-memory/preview",
            post(preview),
        )
        .route(
            "/api/v1/employees/{employee_id}/reviewed-memory/{fact_id}/stop",
            post(stop),
        )
}

// The larger request bound applies only to these three exact POST shapes.
pub(crate) fn has_review_body(path: &str) -> bool {
    let Some(rest) = path.strip_prefix("/api/v1/employees/") else {
        return false;
    };
    let parts: Vec<_> = rest.split('/').collect();
    match parts.as_slice() {
        [employee, "reviewed-memory"] | [employee, "reviewed-memory", "preview"] => {
            EmployeeId::parse(*employee).is_ok()
        }
        [employee, "reviewed-memory", fact, "stop"] => {
            EmployeeId::parse(*employee).is_ok()
                && Uuid::parse_str(fact).is_ok_and(|id| !id.is_nil())
        }
        _ => false,
    }
}

async fn preview(
    State(state): State<ApiState>,
    Extension(principal): Extension<Principal>,
    Path(employee): Path<EmployeeId>,
    body: std::result::Result<Json<PreviewRequest>, JsonRejection>,
) -> Result<Json<Value>> {
    let Json(body) = body.map_err(|_| ApiError::invalid())?;
    let access = Review::new(&state, &principal, employee)?;
    bounded(async {
        access
            .preview(body)
            .await
            .map(|value| json!({"preview":value}))
    })
    .await
}

async fn approve(
    State(state): State<ApiState>,
    Extension(principal): Extension<Principal>,
    Path(employee): Path<EmployeeId>,
    body: std::result::Result<Json<Approval>, JsonRejection>,
) -> Result<Json<Value>> {
    let Json(body) = body.map_err(|_| ApiError::invalid())?;
    let access = Review::new(&state, &principal, employee)?;
    bounded(access.approve(body)).await
}

async fn list(
    State(state): State<ApiState>,
    Extension(principal): Extension<Principal>,
    Path(employee): Path<EmployeeId>,
    query: std::result::Result<Query<Page>, QueryRejection>,
) -> Result<Json<Value>> {
    let Query(query) = query.map_err(|_| ApiError::invalid())?;
    let access = Review::new(&state, &principal, employee)?;
    bounded(access.list(query)).await
}

async fn stop(
    State(state): State<ApiState>,
    Extension(principal): Extension<Principal>,
    Path((employee, fact)): Path<(EmployeeId, Uuid)>,
    body: std::result::Result<Json<Stop>, JsonRejection>,
) -> Result<Json<Value>> {
    let Json(body) = body.map_err(|_| ApiError::invalid())?;
    let access = Review::new(&state, &principal, employee)?;
    bounded(access.stop(fact, body)).await
}

async fn bounded(work: impl std::future::Future<Output = Result<Value>>) -> Result<Json<Value>> {
    let value = tokio::time::timeout(std::time::Duration::from_secs(5), work)
        .await
        .map_err(|_| ApiError::unavailable())??;
    if serde_json::to_vec(&value)
        .map_err(|_| ApiError::unavailable())?
        .len()
        > 262_144
    {
        return Err(ApiError::unavailable());
    }
    Ok(Json(value))
}
fn forbidden() -> ApiError {
    ApiError(StatusCode::FORBIDDEN, "forbidden")
}
fn conflict() -> ApiError {
    ApiError(StatusCode::CONFLICT, "employee_memory_conflict")
}
fn digest(bytes: &[u8]) -> EmployeeMemoryDigest {
    EmployeeMemoryDigest::from_bytes(Sha256::digest(bytes).into())
}
