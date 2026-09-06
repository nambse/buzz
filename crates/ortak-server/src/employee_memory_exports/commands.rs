//! Private signed-command facade mounted under the existing NIP98 middleware.
//! SQL receives resolved identity, never authenticates caller-set JSON or GUCs.
use super::*;
use crate::{
    auth::Principal,
    error::{ApiError, Result as ApiResult},
    routes::ApiState,
};
use axum::{
    Extension, Json, Router,
    extract::{Path, State, rejection::JsonRejection},
    http::StatusCode,
    routing::{get, post},
};
use ortak_domain::EmployeeId;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use sqlx::{Postgres, Transaction};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Command {
    operation_id: Uuid,
    expected_version: i32,
}

// Middleware supplies the existing host-bound fresh-NIP98 deployment Principal.
pub(crate) fn router() -> Router<ApiState> {
    Router::new()
        .route(
            "/api/v1/employees/{employee_id}/reviewed-memory/{fact_id}/export",
            get(inspect).post(publish),
        )
        .route(
            "/api/v1/employees/{employee_id}/reviewed-memory/{fact_id}/export/retry/{action}",
            post(retry),
        )
}
struct Access<'a> {
    state: &'a ApiState,
    principal: &'a Principal,
    employee: EmployeeId,
}
impl<'a> Access<'a> {
    fn new(state: &'a ApiState, principal: &'a Principal, employee: EmployeeId) -> ApiResult<Self> {
        if !principal.grant.employee_ids.contains(&employee) {
            return Err(forbidden());
        }
        Ok(Self {
            state,
            principal,
            employee,
        })
    }
    fn company(&self) -> Uuid {
        self.principal.scope.company_id()
    }
    fn actor(&self) -> String {
        self.principal.public_key.to_hex()
    }
    async fn begin(&self) -> ApiResult<(Transaction<'static, Postgres>, DateTime<Utc>)> {
        let mut tx = self.state.control.pool().begin().await?;
        bounds(&mut tx).await?;
        sqlx::query("SELECT ortak_lock_office_authority($1)")
            .bind(self.company())
            .execute(&mut *tx)
            .await?;
        if !crate::auth::human_allowed_on(
            &mut tx,
            &self.principal.scope,
            self.state.config.community_id,
            &self.principal.public_key,
        )
        .await?
            || !sqlx::query_scalar::<_, bool>(
                "SELECT ortak_employee_memory_command_current($1,$2,$3,'retry_withdraw')",
            )
            .bind(self.company())
            .bind(self.employee.as_str())
            .bind(self.principal.public_key.to_bytes().as_slice())
            .fetch_one(&mut *tx)
            .await?
        {
            return Err(forbidden());
        }
        let deadline = sqlx::query_scalar("SELECT clock_timestamp()+interval '5 seconds'")
            .fetch_one(&mut *tx)
            .await?;
        Ok((tx, deadline))
    }
    async fn finish(
        &self,
        mut tx: Transaction<'_, Postgres>,
        deadline: DateTime<Utc>,
        value: Value,
    ) -> ApiResult<Json<Value>> {
        if serde_json::to_vec(&value)
            .map_err(|_| ApiError::unavailable())?
            .len()
            > 16384
            || !sqlx::query_scalar::<_, bool>("SELECT clock_timestamp()<$1")
                .bind(deadline)
                .fetch_one(&mut *tx)
                .await?
        {
            return Err(ApiError::unavailable());
        }
        tx.commit().await?;
        Ok(Json(value))
    }
    async fn fact(&self, c: &mut PgConnection, fact: Uuid) -> ApiResult<(Uuid, Uuid)> {
        sqlx::query_as(
            "SELECT source_channel_id,destination_channel_id FROM employee_reviewed_memory_facts
            WHERE company_id=$1 AND id=$2 AND employee_id=$3 AND approved_by=$4",
        )
        .bind(self.company())
        .bind(fact)
        .bind(self.employee.as_str())
        .bind(self.principal.public_key.to_bytes().as_slice())
        .fetch_optional(c)
        .await?
        .ok_or_else(forbidden)
    }
    async fn lock(
        &self,
        c: &mut PgConnection,
        fact: Uuid,
        source: Uuid,
        destination: Uuid,
    ) -> ApiResult<()> {
        sqlx::query("SELECT channel_id FROM employee_memory_channel_authorities WHERE company_id=$1 AND community_id=$2 AND employee_id=$3
            AND channel_id IN($4,$5) ORDER BY channel_id FOR SHARE")
            .bind(self.company()).bind(self.state.config.community_id).bind(self.employee.as_str()).bind(source).bind(destination).fetch_all(&mut *c).await?;
        sqlx::query(
            "SELECT id FROM employee_reviewed_memory_facts WHERE company_id=$1 AND id=$2 FOR SHARE",
        )
        .bind(self.company())
        .bind(fact)
        .fetch_one(c)
        .await?;
        Ok(())
    }
    async fn metadata(&self, c: &mut PgConnection, fact: Uuid) -> ApiResult<Value> {
        // Original approver and employee path remain mandatory even when the
        // capability/source/membership/lifecycle that admitted sharing is gone.
        self.fact(c, fact).await?;
        let row=sqlx::query("SELECT target_id,created_at FROM employee_reviewed_memory_exports WHERE company_id=$1 AND fact_id=$2")
            .bind(self.company()).bind(fact).fetch_optional(&mut *c).await?;
        let Some(row) = row else {
            return Ok(json!({"fact_id":fact,"export":null}));
        };
        let jobs=sqlx::query("SELECT action,state,attempt_count,total_attempts,retry_version,last_error_code,
            EXISTS(SELECT 1 FROM employee_reviewed_memory_export_receipts r WHERE r.company_id=j.company_id AND r.fact_id=j.fact_id AND r.action=j.action) AS acknowledged
            FROM employee_reviewed_memory_export_jobs j WHERE company_id=$1 AND fact_id=$2 ORDER BY action LIMIT 3")
            .bind(self.company()).bind(fact).fetch_all(&mut *c).await?;
        if jobs.len() != 2 {
            return Err(ApiError::unavailable());
        }
        let jobs=jobs.into_iter().map(|r|->ApiResult<Value>{Ok(json!({"action":r.try_get::<String,_>("action")?,"state":r.try_get::<String,_>("state")?,
            "attempt_count":r.try_get::<i32,_>("attempt_count")?,"total_attempts":r.try_get::<i32,_>("total_attempts")?,
            "retry_version":r.try_get::<i32,_>("retry_version")?,"last_error_code":r.try_get::<Option<String>,_>("last_error_code")?,
            "acknowledged":r.try_get::<bool,_>("acknowledged")?}))}).collect::<ApiResult<Vec<_>>>()?;
        Ok(
            json!({"fact_id":fact,"export":{"target_id":row.try_get::<Uuid,_>("target_id")?,"created_at":row.try_get::<DateTime<Utc>,_>("created_at")?,"jobs":jobs}}),
        )
    }
    async fn command(&self, fact: Uuid, action: &str, request: Command) -> ApiResult<Json<Value>> {
        if fact.is_nil()
            || request.operation_id.is_nil()
            || (action == "publish" && request.expected_version != 1)
            || (action != "publish" && !(0..8).contains(&request.expected_version))
        {
            return Err(ApiError::invalid());
        }
        let hash = command_hash(request.operation_id, fact, action, request.expected_version)?;
        let (mut tx, deadline) = self.begin().await?;
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))")
            .bind(format!(
                "ortak-employee-export-command:{}:{}:{}",
                self.company(),
                self.actor(),
                request.operation_id
            ))
            .execute(&mut *tx)
            .await?;
        // Replay is resolved before new source/expiry/capability admission. Its
        // immutable effect remains historical; metadata exposes no saved text.
        let replay=sqlx::query("SELECT o.fact_id,o.action,o.request_hash,o.result_version,f.employee_id FROM employee_reviewed_memory_export_commands o
            JOIN employee_reviewed_memory_facts f ON f.company_id=o.company_id AND f.id=o.fact_id
            WHERE o.company_id=$1 AND o.actor_pubkey=$2 AND o.operation_id=$3")
            .bind(self.company()).bind(self.actor()).bind(request.operation_id).fetch_optional(&mut *tx).await?;
        if let Some(r) = replay {
            if r.try_get::<String, _>("employee_id")? != self.employee.as_str() {
                return Err(forbidden());
            }
            if r.try_get::<Uuid, _>("fact_id")? != fact
                || r.try_get::<String, _>("action")? != action
                || r.try_get::<Vec<u8>, _>("request_hash")? != hash
            {
                return Err(conflict());
            }
            let value = json!({"operation_id":request.operation_id,"created":false,"result_version":r.try_get::<i32,_>("result_version")?,
                "record":self.metadata(&mut tx,fact).await?});
            return self.finish(tx, deadline, value).await;
        }
        let (source, destination) = self.fact(&mut tx, fact).await?;
        if action != "retry_withdraw"
            && (!self.principal.grant.can_review_employee_memory
                || !self.principal.grant.channel_ids.contains(&source)
                || !self.principal.grant.channel_ids.contains(&destination))
        {
            return Err(forbidden());
        }
        self.lock(&mut tx, fact, source, destination).await?;
        let version = if action == "publish" {
            let targets:Vec<Uuid>=sqlx::query_scalar("SELECT id FROM employee_reviewed_memory_targets WHERE company_id=$1 AND employee_id=$2
                AND destination_channel_id=$3 AND enabled AND valid_until>clock_timestamp() ORDER BY id LIMIT 2 FOR SHARE")
                .bind(self.company()).bind(self.employee.as_str()).bind(destination).fetch_all(&mut *tx).await?;
            if targets.len() != 1 {
                return Err(conflict());
            }
            let target = targets[0];
            if !sqlx::query_scalar::<_, bool>(
                "SELECT ortak_employee_reviewed_export_eligible($1,$2,$3)",
            )
            .bind(self.company())
            .bind(fact)
            .bind(target)
            .fetch_one(&mut *tx)
            .await?
            {
                return Err(forbidden());
            }
            sqlx::query("INSERT INTO employee_reviewed_memory_exports(company_id,community_id,fact_id,destination_channel_id,employee_id,target_id,
                employee_revision_id,employee_lifecycle_epoch,content_hash,source_hash,sharing_hash,requested_by,operation_id)
                SELECT f.company_id,f.community_id,f.id,f.destination_channel_id,f.employee_id,t.id,t.employee_revision_id,t.employee_lifecycle_epoch,
                    f.content_hash,f.source_hash,f.sharing_hash,$4,$5 FROM employee_reviewed_memory_facts f
                JOIN employee_reviewed_memory_targets t ON t.company_id=f.company_id AND t.id=$3 WHERE f.company_id=$1 AND f.id=$2")
                .bind(self.company()).bind(fact).bind(target).bind(self.actor()).bind(request.operation_id).execute(&mut *tx).await?;
            sqlx::query("INSERT INTO employee_reviewed_memory_export_jobs(company_id,community_id,fact_id,action,idempotency_key,request_hash,next_attempt_at)
                SELECT f.company_id,f.community_id,f.id,v.action,'employee-reviewed:'||v.action||':'||f.company_id::text||':'||f.id::text,
                    ortak_employee_reviewed_request_hash(f.company_id,f.id,v.action),CASE WHEN v.action='withdraw' THEN f.expires_at ELSE clock_timestamp() END
                FROM employee_reviewed_memory_facts f CROSS JOIN (VALUES('publish'),('withdraw')) v(action) WHERE f.company_id=$1 AND f.id=$2")
                .bind(self.company()).bind(fact).execute(&mut *tx).await?;
            0
        } else {
            if action == "retry_publish" {
                sqlx::query("SELECT t.id FROM employee_reviewed_memory_targets t JOIN employee_reviewed_memory_exports x
                    ON x.company_id=t.company_id AND x.target_id=t.id WHERE x.company_id=$1 AND x.fact_id=$2 FOR SHARE OF t")
                    .bind(self.company()).bind(fact).fetch_optional(&mut *tx).await?;
            }
            if action=="retry_publish" && !sqlx::query_scalar::<_,bool>("SELECT coalesce((SELECT ortak_employee_reviewed_export_eligible(company_id,fact_id,target_id)
                FROM employee_reviewed_memory_exports WHERE company_id=$1 AND fact_id=$2),false)")
                .bind(self.company()).bind(fact).fetch_one(&mut *tx).await? {return Err(forbidden());}
            let version:Option<i32>=sqlx::query_scalar("UPDATE employee_reviewed_memory_export_jobs SET state='pending',attempt_count=0,retry_version=retry_version+1,
                next_attempt_at=clock_timestamp(),last_error_code=NULL,lease_token=NULL,lease_expires_at=NULL
                WHERE company_id=$1 AND fact_id=$2 AND action=$3 AND state='failed' AND retry_version=$4 AND retry_version<8 RETURNING retry_version")
                .bind(self.company()).bind(fact).bind(action.strip_prefix("retry_").ok_or_else(ApiError::invalid)?)
                .bind(request.expected_version).fetch_optional(&mut *tx).await?;
            version.ok_or_else(conflict)?
        };
        sqlx::query("INSERT INTO employee_reviewed_memory_export_commands(company_id,community_id,actor_pubkey,operation_id,fact_id,action,request_hash,result_version,auth_event_id,valid_before)
            VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)")
            .bind(self.company()).bind(self.state.config.community_id).bind(self.actor()).bind(request.operation_id).bind(fact).bind(action)
            .bind(hash).bind(version).bind(&self.principal.auth_event_id).bind(deadline).execute(&mut *tx).await?;
        let value = json!({"operation_id":request.operation_id,"created":true,"result_version":version,"record":self.metadata(&mut tx,fact).await?});
        self.finish(tx, deadline, value).await
    }
}
fn command_hash(operation: Uuid, fact: Uuid, action: &str, version: i32) -> ApiResult<Vec<u8>> {
    Ok(Sha256::digest(
        serde_json::to_vec(
            &json!({"format":"ortak-reviewed-employee-export-command/1","operation_id":operation,
        "fact_id":fact,"action":action,"expected_version":version}),
        )
        .map_err(|_| ApiError::invalid())?,
    )
    .to_vec())
}
fn forbidden() -> ApiError {
    ApiError(StatusCode::FORBIDDEN, "forbidden")
}
fn conflict() -> ApiError {
    ApiError(StatusCode::CONFLICT, "employee_memory_export_conflict")
}
async fn inspect(
    State(state): State<ApiState>,
    Extension(p): Extension<Principal>,
    Path((employee, fact)): Path<(EmployeeId, Uuid)>,
) -> ApiResult<Json<Value>> {
    within(async {
        let access = Access::new(&state, &p, employee)?;
        let (mut tx, deadline) = access.begin().await?;
        let value = access.metadata(&mut tx, fact).await?;
        access.finish(tx, deadline, value).await
    })
    .await
}
async fn publish(
    State(state): State<ApiState>,
    Extension(p): Extension<Principal>,
    Path((employee, fact)): Path<(EmployeeId, Uuid)>,
    body: std::result::Result<Json<Command>, JsonRejection>,
) -> ApiResult<Json<Value>> {
    let Json(body) = body.map_err(|_| ApiError::invalid())?;
    within(async {
        Access::new(&state, &p, employee)?
            .command(fact, "publish", body)
            .await
    })
    .await
}
async fn retry(
    State(state): State<ApiState>,
    Extension(p): Extension<Principal>,
    Path((employee, fact, action)): Path<(EmployeeId, Uuid, String)>,
    body: std::result::Result<Json<Command>, JsonRejection>,
) -> ApiResult<Json<Value>> {
    let Json(body) = body.map_err(|_| ApiError::invalid())?;
    let action = match action.as_str() {
        "publish" => "retry_publish",
        "withdraw" => "retry_withdraw",
        _ => return Err(ApiError::invalid()),
    };
    within(async {
        Access::new(&state, &p, employee)?
            .command(fact, action, body)
            .await
    })
    .await
}

async fn within(
    work: impl std::future::Future<Output = ApiResult<Json<Value>>>,
) -> ApiResult<Json<Value>> {
    tokio::time::timeout(Duration::from_secs(5), work)
        .await
        .map_err(|_| ApiError::unavailable())?
}
