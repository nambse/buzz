//! Read-only, narrow projection of durable provisioning records. No adapter,
//! environment lookup, manifest decoding or runner is reachable from these reads.
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Extension, Json,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use chrono::{DateTime, Utc};
use ortak_control::provisioning::{OperationMode, OperationStatus, ProvisioningStep, StepState};
use ortak_domain::EmployeeId;
use serde::{Deserialize, Serialize};
use sqlx::{postgres::PgRow, Row};
use uuid::Uuid;

use crate::{
    auth::Principal,
    error::{ApiError, Result},
    routes::ApiState,
};

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PageQuery {
    cursor: Option<String>,
    limit: Option<u32>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Cursor {
    created_at: DateTime<Utc>,
    operation_id: Uuid,
}
impl Cursor {
    fn decode(value: &str) -> Result<Self> {
        if value.len() > 256 {
            return Err(ApiError::invalid());
        }
        let bytes = URL_SAFE_NO_PAD
            .decode(value)
            .map_err(|_| ApiError::invalid())?;
        let cursor: Self = serde_json::from_slice(&bytes).map_err(|_| ApiError::invalid())?;
        if cursor.operation_id.is_nil() {
            return Err(ApiError::invalid());
        }
        Ok(cursor)
    }
    fn encode(&self) -> Result<String> {
        Ok(URL_SAFE_NO_PAD.encode(serde_json::to_vec(self).map_err(|_| ApiError::unavailable())?))
    }
}

#[derive(Serialize)]
struct Summary {
    operation_id: Uuid,
    employee_id: EmployeeId,
    mode: OperationMode,
    dry_run: bool,
    status: OperationStatus,
    current_step: Option<ProvisioningStep>,
    result_revision_id: Option<Uuid>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    finished_at: Option<DateTime<Utc>>,
}
impl Summary {
    fn read(row: &PgRow) -> Result<Self> {
        let step: Option<String> = row.try_get("current_step")?;
        Ok(Self {
            operation_id: row.try_get("id")?,
            employee_id: EmployeeId::parse(row.try_get::<String, _>("employee_id")?)
                .map_err(|_| ApiError::unavailable())?,
            mode: parse(row.try_get::<String, _>("mode")?)?,
            dry_run: row.try_get("dry_run")?,
            status: parse(row.try_get::<String, _>("status")?)?,
            current_step: step
                .map(|name| ProvisioningStep::parse(&name).ok_or_else(ApiError::unavailable))
                .transpose()?,
            result_revision_id: row.try_get("result_revision_id")?,
            created_at: row.try_get("created_at")?,
            updated_at: row.try_get("updated_at")?,
            finished_at: row.try_get("finished_at")?,
        })
    }
}
fn parse<T: serde::de::DeserializeOwned>(text: String) -> Result<T> {
    serde_json::from_value(serde_json::Value::String(text)).map_err(|_| ApiError::unavailable())
}

async fn authorized(state: &ApiState, principal: &Principal, employee: &EmployeeId) -> Result<()> {
    if !principal.grant.can_manage_employees || principal.grant.role != crate::Role::Operator {
        state
            .audit_principal(principal, "read_employee", "denied", None)
            .await?;
        return Err(ApiError(StatusCode::FORBIDDEN, "forbidden"));
    }
    if !principal.grant.employee_ids.contains(employee) {
        state
            .audit_principal(principal, "read_employee", "not_found", None)
            .await?;
        return Err(ApiError::not_found());
    }
    let exists: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM employees WHERE company_id=$1 AND id=$2)")
            .bind(principal.scope.company_id())
            .bind(employee.as_str())
            .fetch_one(state.control.pool())
            .await?;
    if !exists {
        state
            .audit_principal(principal, "read_employee", "not_found", None)
            .await?;
        return Err(ApiError::not_found());
    }
    Ok(())
}

pub(super) async fn list(
    State(state): State<ApiState>,
    Extension(principal): Extension<Principal>,
    Path(employee): Path<EmployeeId>,
    Query(query): Query<PageQuery>,
) -> Result<Json<serde_json::Value>> {
    authorized(&state, &principal, &employee).await?;
    let cursor = query.cursor.as_deref().map(Cursor::decode).transpose()?;
    let limit = query.limit.unwrap_or(25).clamp(1, 25);
    let rows=sqlx::query("SELECT id,employee_id,mode,dry_run,status,current_step,result_revision_id,created_at,updated_at,finished_at FROM provisioning_operations WHERE company_id=$1 AND employee_id=$2 AND ($3::timestamptz IS NULL OR (created_at,id)<($3,$4::uuid)) ORDER BY created_at DESC,id DESC LIMIT $5")
        .bind(principal.scope.company_id()).bind(employee.as_str()).bind(cursor.as_ref().map(|c|c.created_at)).bind(cursor.as_ref().map(|c|c.operation_id)).bind(i64::from(limit)+1).fetch_all(state.control.pool()).await?;
    let has_more = rows.len() > limit as usize;
    let operations = rows
        .iter()
        .take(limit as usize)
        .map(Summary::read)
        .collect::<Result<Vec<_>>>()?;
    let next_cursor = if has_more {
        operations
            .last()
            .map(|op| {
                Cursor {
                    created_at: op.created_at,
                    operation_id: op.operation_id,
                }
                .encode()
            })
            .transpose()?
    } else {
        None
    };
    Ok(Json(
        serde_json::json!({"employee_id":employee,"operations":operations,"has_more":has_more,"next_cursor":next_cursor,"read_only":true}),
    ))
}

#[derive(Serialize)]
struct Step {
    name: ProvisioningStep,
    state: StepState,
    attempt_count: i32,
    adopted_existing: bool,
    started_at: Option<DateTime<Utc>>,
    finished_at: Option<DateTime<Utc>>,
}

pub(super) async fn detail(
    State(state): State<ApiState>,
    Extension(principal): Extension<Principal>,
    Path((employee, operation_id)): Path<(EmployeeId, Uuid)>,
) -> Result<Json<serde_json::Value>> {
    authorized(&state, &principal, &employee).await?;
    // An explicit snapshot keeps the operation header and its ten steps from
    // tearing while the independent CLI records progress. The middleware keeps
    // current Office authority fenced across this read-only transaction.
    let mut tx = state.control.pool().begin().await?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY")
        .execute(&mut *tx)
        .await?;
    let row=sqlx::query("SELECT id,employee_id,mode,dry_run,status,current_step,result_revision_id,created_at,updated_at,finished_at FROM provisioning_operations WHERE company_id=$1 AND employee_id=$2 AND id=$3")
        .bind(principal.scope.company_id()).bind(employee.as_str()).bind(operation_id).fetch_optional(&mut *tx).await?.ok_or_else(ApiError::not_found)?;
    let operation = Summary::read(&row)?;
    let rows=sqlx::query("SELECT step_index,step_name,state,attempt_count,adopted_existing,started_at,finished_at FROM provisioning_operation_steps WHERE company_id=$1 AND operation_id=$2 ORDER BY step_index LIMIT 11")
        .bind(principal.scope.company_id()).bind(operation_id).fetch_all(&mut *tx).await?;
    if rows.len() != ProvisioningStep::ALL.len() {
        return Err(ApiError::unavailable());
    }
    let steps = rows
        .iter()
        .zip(ProvisioningStep::ALL)
        .map(|(row, name)| {
            if row.try_get::<i16, _>("step_index")? != name.index()
                || row.try_get::<String, _>("step_name")? != name.name()
            {
                return Err(ApiError::unavailable());
            }
            let attempt_count = row.try_get::<i32, _>("attempt_count")?;
            if attempt_count < 0 {
                return Err(ApiError::unavailable());
            }
            Ok(Step {
                name,
                state: parse(row.try_get::<String, _>("state")?)?,
                attempt_count,
                adopted_existing: row.try_get("adopted_existing")?,
                started_at: row.try_get("started_at")?,
                finished_at: row.try_get("finished_at")?,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let runtime_probe: Option<serde_json::Value> = sqlx::query_scalar("SELECT jsonb_build_object('generation',generation,'state',state,'created_at',created_at,'deadline',deadline,'contained_at',contained_at,'error_code',error_code) FROM provisioning_runtime_probes WHERE company_id=$1 AND operation_id=$2 ORDER BY generation DESC LIMIT 1")
        .bind(principal.scope.company_id()).bind(operation_id).fetch_optional(&mut *tx).await?;
    tx.commit().await?;
    Ok(Json(
        serde_json::json!({"operation":operation,"steps":steps,"runtime_probe":runtime_probe,"read_only":true}),
    ))
}
