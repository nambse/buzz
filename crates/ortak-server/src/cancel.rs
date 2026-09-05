use axum::{
    extract::{Path, State},
    http::StatusCode,
    Extension, Json,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::Row;
use uuid::Uuid;

use crate::{
    auth::{human_allowed_on, lock_authority, Principal},
    config::Role,
    error::{ApiError, Result},
    routes::ApiState,
    store::audit_on,
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CancelBody {}

#[derive(Serialize)]
pub(crate) struct Cancellation {
    request_id: Uuid,
    run_id: Uuid,
    status: String,
    requested_at: DateTime<Utc>,
}

impl ApiState {
    pub(crate) async fn cancellation(
        &self,
        principal: &Principal,
        run_id: Uuid,
    ) -> Result<Option<Cancellation>> {
        let row = sqlx::query("SELECT id, run_id, status, requested_at FROM run_cancel_requests WHERE company_id = $1 AND run_id = $2")
            .bind(principal.scope.company_id()).bind(run_id).fetch_optional(self.control.pool()).await?;
        row.as_ref().map(from_row).transpose()
    }
}

pub(crate) async fn request(
    State(state): State<ApiState>,
    Extension(principal): Extension<Principal>,
    Path(run_id): Path<Uuid>,
    Json(_body): Json<CancelBody>,
) -> Result<(StatusCode, Json<Cancellation>)> {
    if principal.grant.role != Role::Operator {
        state
            .audit_principal(&principal, "cancel_run", "denied", Some(run_id))
            .await?;
        return Err(ApiError(StatusCode::FORBIDDEN, "forbidden"));
    }
    let mut tx = state.control.pool().begin().await?;
    lock_authority(&mut tx, &principal.scope).await?;
    if !human_allowed_on(
        &mut tx,
        &principal.scope,
        state.config.community_id,
        &principal.public_key,
    )
    .await?
    {
        audit_on(
            &mut tx,
            &principal.scope,
            &principal.public_key,
            &principal.auth_event_id,
            "cancel_run",
            "denied",
            Some(run_id),
        )
        .await?;
        tx.commit().await?;
        return Err(ApiError(StatusCode::FORBIDDEN, "forbidden"));
    }
    let row = sqlx::query("SELECT status FROM runs WHERE company_id = $1 AND id = $2 FOR UPDATE")
        .bind(principal.scope.company_id())
        .bind(run_id)
        .fetch_optional(&mut *tx)
        .await?;
    if row.is_none() || !state.visible_run_on(&mut tx, &principal, run_id).await? {
        audit_on(
            &mut tx,
            &principal.scope,
            &principal.public_key,
            &principal.auth_event_id,
            "cancel_run",
            "not_found",
            Some(run_id),
        )
        .await?;
        tx.commit().await?;
        return Err(ApiError::not_found());
    }
    let status: String = row.ok_or_else(ApiError::not_found)?.try_get("status")?;
    if matches!(status.as_str(), "completed" | "failed" | "cancelled") {
        audit_on(
            &mut tx,
            &principal.scope,
            &principal.public_key,
            &principal.auth_event_id,
            "cancel_run",
            "already_terminal",
            Some(run_id),
        )
        .await?;
        tx.commit().await?;
        return Err(ApiError(StatusCode::CONFLICT, "run_already_terminal"));
    }
    let inserted = sqlx::query(
        "INSERT INTO run_cancel_requests (company_id, run_id, requested_by, auth_event_id)
          VALUES ($1, $2, $3, $4) ON CONFLICT (company_id, run_id) DO NOTHING",
    )
    .bind(principal.scope.company_id())
    .bind(run_id)
    .bind(principal.public_key.to_hex())
    .bind(&principal.auth_event_id)
    .execute(&mut *tx)
    .await?
    .rows_affected()
        == 1;
    let row = sqlx::query("SELECT id, run_id, status, requested_at FROM run_cancel_requests WHERE company_id = $1 AND run_id = $2")
        .bind(principal.scope.company_id()).bind(run_id).fetch_one(&mut *tx).await?;
    let result = from_row(&row)?;
    audit_on(
        &mut tx,
        &principal.scope,
        &principal.public_key,
        &principal.auth_event_id,
        "cancel_run",
        if inserted {
            "requested"
        } else {
            "already_requested"
        },
        Some(run_id),
    )
    .await?;
    tx.commit().await?;
    let status = match result.status.as_str() {
        "pending" => StatusCode::ACCEPTED,
        "acknowledged" => StatusCode::OK,
        "failed" => return Err(ApiError(StatusCode::CONFLICT, "cancellation_failed")),
        _ => return Err(ApiError::unavailable()),
    };
    Ok((status, Json(result)))
}

fn from_row(row: &sqlx::postgres::PgRow) -> Result<Cancellation> {
    let status: String = row.try_get("status")?;
    if !matches!(status.as_str(), "pending" | "acknowledged" | "failed") {
        return Err(ApiError::unavailable());
    }
    Ok(Cancellation {
        request_id: row.try_get("id")?,
        run_id: row.try_get("run_id")?,
        status,
        requested_at: row.try_get("requested_at")?,
    })
}
