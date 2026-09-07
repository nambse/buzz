use super::policy;

async fn capability(state: &ApiState, principal: &Principal) -> Result<()> {
    if policy::enabled(&principal.grant) {
        return Ok(());
    }
    // Default-off deployments may not have the management migration yet.
    // Use the existing audit vocabulary before touching its private tables.
    state
        .audit_principal(principal, "access", "denied", None)
        .await?;
    Err(ApiError(StatusCode::FORBIDDEN, "forbidden"))
}
use crate::{
    auth::Principal,
    error::{ApiError, Result},
    provisioning::ProvisioningConfig,
    routes::ApiState,
};
use axum::{
    extract::{Path, State},
    http::StatusCode,
    Extension, Json,
};
use ortak_domain::EmployeeId;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::{PgConnection, Row};
use uuid::Uuid;

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct DraftBody {
    draft_id: Uuid,
    catalog_id: Uuid,
    expected_revision_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    expected_lifecycle_epoch: Option<i64>,
}

#[derive(Clone, Copy, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub(super) enum Action {
    Adopt,
    Update,
    Retry,
    Compensate,
    Disable,
    Reenable,
}
impl Action {
    fn name(self) -> &'static str {
        match self {
            Self::Adopt => "adopt",
            Self::Update => "update",
            Self::Retry => "retry",
            Self::Compensate => "compensate",
            Self::Disable => "disable",
            Self::Reenable => "reenable",
        }
    }
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct CommandBody {
    idempotency_key: String,
    action: Action,
    draft_id: Option<Uuid>,
    operation_id: Option<Uuid>,
    expected_revision_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    expected_lifecycle_epoch: Option<i64>,
}

async fn access(
    c: &mut PgConnection,
    p: &Principal,
    employee: &str,
    channels: &[Uuid],
    action: &str,
) -> Result<()> {
    if !policy::allowed_on(c, p, employee, channels).await? {
        policy::audit(c, p, Some(employee), None, action, "denied").await?;
        return Err(ApiError(StatusCode::FORBIDDEN, "forbidden"));
    }
    Ok(())
}
async fn baseline(
    c: &mut PgConnection,
    p: &Principal,
    employee: &str,
) -> Result<(Option<Uuid>, Option<String>, i64)> {
    let row = sqlx::query(
        "SELECT active_revision_id,status,lifecycle_epoch FROM employees WHERE company_id=$1 AND id=$2 FOR SHARE",
    )
    .bind(p.scope.company_id())
    .bind(employee)
    .fetch_optional(c)
    .await?;
    Ok(match row {
        Some(row) => (
            row.try_get("active_revision_id")?,
            Some(row.try_get("status")?),
            row.try_get("lifecycle_epoch")?,
        ),
        None => (None, None, 0),
    })
}
fn configuration(value: Value, p: &Principal) -> Result<ProvisioningConfig> {
    let config: ProvisioningConfig =
        serde_json::from_value(value).map_err(|_| ApiError::unavailable())?;
    config
        .validate(&p.scope)
        .map_err(|_| ApiError::unavailable())?;
    Ok(config)
}

pub(super) async fn catalog(
    State(state): State<ApiState>,
    Extension(p): Extension<Principal>,
) -> Result<Json<Value>> {
    capability(&state, &p).await?;
    let mut tx = state.control.pool().begin().await?;
    let permitted = match p.grant.employee_ids.first() {
        Some(employee) => policy::allowed_on(&mut tx, &p, employee.as_str(), &[]).await?,
        None => false,
    };
    if !permitted {
        policy::audit(&mut tx, &p, None, None, "catalog", "denied").await?;
        tx.commit().await?;
        return Err(ApiError(StatusCode::FORBIDDEN, "forbidden"));
    }
    let employee_ids: Vec<_> = p.grant.employee_ids.iter().map(|id| id.as_str()).collect();
    let rows=sqlx::query("SELECT id,employee_id,label,configuration FROM prepared_employee_catalog WHERE company_id=$1 AND enabled AND employee_id=ANY($2) ORDER BY employee_id,id LIMIT 64")
        .bind(p.scope.company_id()).bind(employee_ids).fetch_all(&mut *tx).await?;
    let mut choices = Vec::new();
    for row in rows {
        let employee: String = row.try_get("employee_id")?;
        let config = configuration(row.try_get("configuration")?, &p)?;
        if !policy::allowed_on(&mut tx, &p, &employee, &config.office.employees[0].channels).await?
        {
            continue;
        }
        let (revision, status, epoch) = baseline(&mut tx, &p, &employee).await?;
        choices.push(json!({"catalog_id":row.try_get::<Uuid,_>("id")?,"employee_id":employee,"label":row.try_get::<String,_>("label")?,
            "model":config.manifest.employee.runtime.model,"thinking":config.manifest.employee.runtime.options.get("reasoning_effort"),
            "expected_revision_id":revision,"expected_lifecycle_epoch":epoch,"status":status,"can_save_draft":true}));
    }
    let mut employees = Vec::new();
    for employee in &p.grant.employee_ids {
        if !policy::allowed_on(&mut tx, &p, employee.as_str(), &[]).await? {
            continue;
        }
        let (revision, status, epoch) = baseline(&mut tx, &p, employee.as_str()).await?;
        if status.is_some() {
            employees.push(json!({"employee_id":employee,"status":status,"expected_revision_id":revision,"expected_lifecycle_epoch":epoch}));
        }
    }
    policy::audit(&mut tx, &p, None, None, "catalog", "read").await?;
    tx.commit().await?;
    Ok(Json(
        json!({"choices":choices,"employees":employees,"create_supported":false,"lifecycle_supported":true}),
    ))
}

pub(super) async fn draft(
    State(state): State<ApiState>,
    Extension(p): Extension<Principal>,
    Path(employee): Path<EmployeeId>,
    Json(body): Json<DraftBody>,
) -> Result<(StatusCode, Json<Value>)> {
    capability(&state, &p).await?;
    if body.draft_id.is_nil() || body.catalog_id.is_nil() {
        return Err(ApiError::invalid());
    }
    let mut tx = state.control.pool().begin().await?;
    if let Err(e) = access(&mut tx, &p, employee.as_str(), &[], "draft").await {
        tx.commit().await?;
        return Err(e);
    }
    if let Some(saved) = sqlx::query("SELECT catalog_id,actor,employee_id,expected_revision_id,employee_lifecycle_epoch,reenable,configuration FROM employee_configuration_drafts WHERE company_id=$1 AND id=$2")
        .bind(p.scope.company_id()).bind(body.draft_id).fetch_optional(&mut *tx).await? {
        let same = saved.try_get::<Uuid,_>("catalog_id")? == body.catalog_id
            && saved.try_get::<String,_>("actor")? == p.public_key.to_hex()
            && saved.try_get::<String,_>("employee_id")? == employee.as_str()
            && saved.try_get::<Option<Uuid>,_>("expected_revision_id")? == body.expected_revision_id
            && body.expected_lifecycle_epoch.is_none_or(|epoch| saved.try_get::<i64,_>("employee_lifecycle_epoch").ok()==Some(epoch));
        if !same {
            policy::audit(&mut tx,&p,Some(employee.as_str()),None,"draft","conflict").await?;
            tx.commit().await?; return Err(ApiError(StatusCode::CONFLICT,"draft_changed"));
        }
        let config=configuration(saved.try_get("configuration")?,&p)?;
        if let Err(error)=access(&mut tx,&p,employee.as_str(),&config.office.employees[0].channels,"draft").await {
            tx.commit().await?;return Err(error);
        }
        policy::audit(&mut tx,&p,Some(employee.as_str()),None,"draft","replayed").await?;
        tx.commit().await?;
        return Ok((StatusCode::OK,Json(json!({"draft_id":body.draft_id,"employee_id":employee,"catalog_id":body.catalog_id,"expected_revision_id":body.expected_revision_id,"expected_lifecycle_epoch":saved.try_get::<i64,_>("employee_lifecycle_epoch")?,"action":if saved.try_get::<bool,_>("reenable")? {"reenable"}else if body.expected_revision_id.is_some(){"update"}else{"adopt"},"model":config.manifest.employee.runtime.model,"thinking":config.manifest.employee.runtime.options.get("reasoning_effort")}))));
    }
    let selected=sqlx::query("SELECT configuration FROM prepared_employee_catalog WHERE company_id=$1 AND id=$2 AND employee_id=$3 AND enabled FOR SHARE")
        .bind(p.scope.company_id()).bind(body.catalog_id).bind(employee.as_str()).fetch_optional(&mut *tx).await?;
    let Some(selected) = selected else {
        policy::audit(
            &mut tx,
            &p,
            Some(employee.as_str()),
            None,
            "draft",
            "not_found",
        )
        .await?;
        tx.commit().await?;
        return Err(ApiError::not_found());
    };
    let (revision, status, epoch) = baseline(&mut tx, &p, employee.as_str()).await?;
    let mut value: Value = selected.try_get("configuration")?;
    value["operation_key"] = json!(format!("management:{}", body.draft_id));
    value["mode"] = json!(if body.expected_revision_id.is_some()
        || status.as_deref() == Some("disabled")
    {
        "update"
    } else {
        "adopt"
    });
    value["dry_run"] = json!(false);
    let config = configuration(value.clone(), &p)?;
    if let Err(e) = access(
        &mut tx,
        &p,
        employee.as_str(),
        &config.office.employees[0].channels,
        "draft",
    )
    .await
    {
        tx.commit().await?;
        return Err(e);
    }
    if revision != body.expected_revision_id
        || body
            .expected_lifecycle_epoch
            .is_some_and(|expected| expected != epoch)
        || (status.as_deref() == Some("disabled") && body.expected_lifecycle_epoch != Some(epoch))
    {
        policy::audit(
            &mut tx,
            &p,
            Some(employee.as_str()),
            None,
            "draft",
            "conflict",
        )
        .await?;
        tx.commit().await?;
        return Err(ApiError(StatusCode::CONFLICT, "revision_changed"));
    }
    let hash = super::fingerprint(&value).map_err(|_| ApiError::invalid())?;
    let inserted=sqlx::query("INSERT INTO employee_configuration_drafts(company_id,id,employee_id,catalog_id,actor,expected_revision_id,configuration,fingerprint,employee_lifecycle_epoch,reenable) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10) ON CONFLICT DO NOTHING")
        .bind(p.scope.company_id()).bind(body.draft_id).bind(employee.as_str()).bind(body.catalog_id).bind(p.public_key.to_hex())
        .bind(revision).bind(&value).bind(&hash).bind(epoch).bind(status.as_deref()==Some("disabled")).execute(&mut *tx).await?.rows_affected()==1;
    let matches:bool=sqlx::query_scalar("SELECT fingerprint=$3 AND actor=$4 AND catalog_id=$5 AND employee_id=$6 FROM employee_configuration_drafts WHERE company_id=$1 AND id=$2")
        .bind(p.scope.company_id()).bind(body.draft_id).bind(hash).bind(p.public_key.to_hex()).bind(body.catalog_id).bind(employee.as_str()).fetch_one(&mut *tx).await?;
    if !matches {
        policy::audit(
            &mut tx,
            &p,
            Some(employee.as_str()),
            None,
            "draft",
            "conflict",
        )
        .await?;
        tx.commit().await?;
        return Err(ApiError(StatusCode::CONFLICT, "draft_changed"));
    }
    policy::audit(
        &mut tx,
        &p,
        Some(employee.as_str()),
        None,
        "draft",
        if inserted { "accepted" } else { "replayed" },
    )
    .await?;
    tx.commit().await?;
    Ok((
        if inserted {
            StatusCode::CREATED
        } else {
            StatusCode::OK
        },
        Json(
            json!({"draft_id":body.draft_id,"employee_id":employee,"catalog_id":body.catalog_id,"expected_revision_id":revision,"expected_lifecycle_epoch":epoch,"action":if status.as_deref()==Some("disabled"){"reenable"}else if revision.is_some(){"update"}else{"adopt"},"model":config.manifest.employee.runtime.model,"thinking":config.manifest.employee.runtime.options.get("reasoning_effort")}),
        ),
    ))
}

pub(super) async fn admit(
    State(state): State<ApiState>,
    Extension(p): Extension<Principal>,
    Path(employee): Path<EmployeeId>,
    Json(body): Json<CommandBody>,
) -> Result<(StatusCode, Json<Value>)> {
    capability(&state, &p).await?;
    if body.idempotency_key.is_empty()
        || body.idempotency_key.len() > 128
        || !body
            .idempotency_key
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_' | b':'))
    {
        return Err(ApiError::invalid());
    }
    let keyhash = super::fingerprint(&body).map_err(|_| ApiError::invalid())?;
    let mut tx = state.control.pool().begin().await?;
    if let Err(e) = access(&mut tx, &p, employee.as_str(), &[], body.action.name()).await {
        tx.commit().await?;
        return Err(e);
    }
    // Serialize admissions even before an employee row exists. This lock never
    // spans a runner or adapter call.
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))")
        .bind(format!(
            "ortak-management-admit:{}:{}",
            p.scope.company_id(),
            employee.as_str()
        ))
        .execute(&mut *tx)
        .await?;
    if let Some(row)=sqlx::query("SELECT id,actor,employee_id,request_fingerprint FROM employee_management_commands WHERE company_id=$1 AND idempotency_key=$2")
        .bind(p.scope.company_id()).bind(&body.idempotency_key).fetch_optional(&mut *tx).await? {
        let same=row.try_get::<Vec<u8>,_>("request_fingerprint")?==keyhash && row.try_get::<String,_>("actor")?==p.public_key.to_hex() && row.try_get::<String,_>("employee_id")?==employee.as_str();
        policy::audit(&mut tx,&p,Some(employee.as_str()),None,body.action.name(),if same{"replayed"}else{"conflict"}).await?;
        let id:Uuid=row.try_get("id")?;
        tx.commit().await?;
        return if same{Ok((StatusCode::ACCEPTED,Json(json!({"command_id":id,"employee_id":employee}))))}else{Err(ApiError(StatusCode::CONFLICT,"idempotency_conflict"))};
    }
    let selected = select_command(&mut tx, &p, &employee, &body).await;
    let (configuration, channels) = match selected {
        Ok(v) => v,
        Err(e) => {
            policy::audit(
                &mut tx,
                &p,
                Some(employee.as_str()),
                None,
                body.action.name(),
                if e.0 == StatusCode::FORBIDDEN {
                    "denied"
                } else {
                    "conflict"
                },
            )
            .await?;
            tx.commit().await?;
            return Err(e);
        }
    };
    if !policy::allowed_on(&mut tx, &p, employee.as_str(), &channels).await? {
        policy::audit(
            &mut tx,
            &p,
            Some(employee.as_str()),
            None,
            body.action.name(),
            "denied",
        )
        .await?;
        tx.commit().await?;
        return Err(ApiError(StatusCode::FORBIDDEN, "forbidden"));
    }
    let (revision, status, epoch) = baseline(&mut tx, &p, employee.as_str()).await?;
    if body.action != Action::Compensate
        && (revision != body.expected_revision_id
            || body
                .expected_lifecycle_epoch
                .is_some_and(|expected| expected != epoch)
            || (matches!(body.action, Action::Disable | Action::Reenable)
                && body.expected_lifecycle_epoch != Some(epoch))
            || (body.action == Action::Disable
                && !matches!(status.as_deref(), Some("active" | "paused")))
            || ((body.action == Action::Reenable) != (status.as_deref() == Some("disabled"))))
    {
        policy::audit(
            &mut tx,
            &p,
            Some(employee.as_str()),
            None,
            body.action.name(),
            "conflict",
        )
        .await?;
        tx.commit().await?;
        return Err(ApiError(StatusCode::CONFLICT, "revision_changed"));
    }
    let busy:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM employee_management_commands WHERE company_id=$1 AND employee_id=$2 AND status IN ('pending','running'))")
        .bind(p.scope.company_id()).bind(employee.as_str()).fetch_one(&mut *tx).await?;
    if busy {
        policy::audit(
            &mut tx,
            &p,
            Some(employee.as_str()),
            None,
            body.action.name(),
            "conflict",
        )
        .await?;
        tx.commit().await?;
        return Err(ApiError(StatusCode::CONFLICT, "employee_command_pending"));
    }
    let id = Uuid::new_v4();
    sqlx::query("INSERT INTO employee_management_commands(company_id,id,employee_id,actor,auth_event_id,policy_fingerprint,action,idempotency_key,request_fingerprint,draft_id,operation_id,expected_revision_id,configuration,channel_ids,policy_snapshot,employee_lifecycle_epoch) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16)")
        .bind(p.scope.company_id()).bind(id).bind(employee.as_str()).bind(p.public_key.to_hex()).bind(&p.auth_event_id).bind(policy::hash(&p.grant)?)
        .bind(body.action.name()).bind(&body.idempotency_key).bind(keyhash).bind(body.draft_id).bind(body.operation_id).bind(body.expected_revision_id).bind(configuration).bind(channels).bind(serde_json::to_value(&p.grant).map_err(|_|ApiError::unavailable())?).bind(epoch)
        .execute(&mut *tx).await?;
    policy::audit(
        &mut tx,
        &p,
        Some(employee.as_str()),
        Some(id),
        body.action.name(),
        "accepted",
    )
    .await?;
    tx.commit().await?;
    Ok((
        StatusCode::ACCEPTED,
        Json(json!({"command_id":id,"employee_id":employee})),
    ))
}

async fn select_command(
    c: &mut PgConnection,
    p: &Principal,
    employee: &EmployeeId,
    body: &CommandBody,
) -> Result<(Option<Value>, Vec<Uuid>)> {
    let invalid = || ApiError(StatusCode::CONFLICT, "command_not_available");
    let value = match body.action {
        Action::Disable => {
            if body.draft_id.is_some() || body.operation_id.is_some() {
                return Err(invalid());
            }
            return Ok((None, vec![]));
        }
        Action::Adopt | Action::Update | Action::Reenable if body.operation_id.is_none() => {
            if body.operation_id.is_some()
                || body.draft_id.is_none()
                || (body.action == Action::Adopt && body.expected_revision_id.is_some())
                || (body.action == Action::Update && body.expected_revision_id.is_none())
            {
                return Err(invalid());
            }
            let value:Option<Value>=sqlx::query_scalar("SELECT d.configuration FROM employee_configuration_drafts d JOIN prepared_employee_catalog c ON c.company_id=d.company_id AND c.id=d.catalog_id AND c.enabled WHERE d.company_id=$1 AND d.id=$2 AND d.employee_id=$3 AND d.expected_revision_id IS NOT DISTINCT FROM $4 AND d.employee_lifecycle_epoch=coalesce((SELECT lifecycle_epoch FROM employees WHERE company_id=d.company_id AND id=d.employee_id),0) AND d.reenable=$5 AND ($6::bigint IS NULL OR d.employee_lifecycle_epoch=$6) FOR SHARE OF c")
                .bind(p.scope.company_id()).bind(body.draft_id).bind(employee.as_str()).bind(body.expected_revision_id).bind(body.action==Action::Reenable).bind(body.expected_lifecycle_epoch).fetch_optional(&mut *c).await?;
            value.ok_or_else(invalid)?
        }
        Action::Retry | Action::Compensate | Action::Reenable => {
            if body.draft_id.is_some() || body.operation_id.is_none() {
                return Err(invalid());
            }
            let op=sqlx::query("SELECT status,mode,result_revision_id,employee_lifecycle_epoch,manifest->>'provisioning' AS resource_mode FROM provisioning_operations WHERE company_id=$1 AND id=$2 AND employee_id=$3")
                .bind(p.scope.company_id()).bind(body.operation_id).bind(employee.as_str()).fetch_optional(&mut *c).await?.ok_or_else(invalid)?;
            let status: String = op.try_get("status")?;
            if op
                .try_get::<Option<Uuid>, _>("result_revision_id")?
                .is_some()
                || op.try_get::<String, _>("mode")? == "create"
                || op.try_get::<Option<String>, _>("resource_mode")?.as_deref() != Some("adopt")
            {
                return Err(invalid());
            }
            if body.action == Action::Compensate {
                if !matches!(status.as_str(), "failed" | "compensating" | "compensated") {
                    return Err(invalid());
                }
                return Ok((None, vec![]));
            }
            if !matches!(status.as_str(), "pending" | "running" | "failed")
                || body.expected_lifecycle_epoch.is_some_and(|epoch| {
                    op.try_get::<i64, _>("employee_lifecycle_epoch").ok() != Some(epoch)
                })
            {
                return Err(invalid());
            }
            let current_epoch: i64 = sqlx::query_scalar(
                "SELECT coalesce((SELECT lifecycle_epoch FROM employees WHERE company_id=$1 AND id=$2),0)",
            )
            .bind(p.scope.company_id())
            .bind(employee.as_str())
            .fetch_one(&mut *c)
            .await?;
            if op.try_get::<i64, _>("employee_lifecycle_epoch")? != current_epoch {
                return Err(invalid());
            }
            let exhausted:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM provisioning_operation_steps WHERE company_id=$1 AND operation_id=$2 AND state NOT IN ('succeeded','skipped') AND attempt_count>=3) OR EXISTS(SELECT 1 FROM provisioning_runtime_probes WHERE company_id=$1 AND operation_id=$2 AND generation=20 AND (state='failed' OR (state='succeeded' AND deadline<=clock_timestamp())))")
                .bind(p.scope.company_id()).bind(body.operation_id).fetch_one(&mut *c).await?;
            if exhausted {
                return Err(ApiError(StatusCode::CONFLICT, "step_attempts_exhausted"));
            }
            sqlx::query_scalar::<_,Value>("SELECT configuration FROM employee_management_commands WHERE company_id=$1 AND operation_id=$2 AND configuration IS NOT NULL AND ((action='reenable')=$3) ORDER BY created_at,id LIMIT 1")
                .bind(p.scope.company_id()).bind(body.operation_id).bind(body.action==Action::Reenable).fetch_optional(&mut *c).await?.ok_or(ApiError(StatusCode::CONFLICT,"prepared_selection_not_retained"))?
        }
        _ => return Err(invalid()),
    };
    let config = configuration(value.clone(), p)?;
    if &config.manifest.employee.id != employee {
        return Err(invalid());
    }
    Ok((Some(value), config.office.employees[0].channels.clone()))
}

pub(super) async fn commands(
    State(state): State<ApiState>,
    Extension(p): Extension<Principal>,
    Path(employee): Path<EmployeeId>,
) -> Result<Json<Value>> {
    capability(&state, &p).await?;
    let mut tx = state.control.pool().begin().await?;
    if let Err(e) = access(&mut tx, &p, employee.as_str(), &[], "command").await {
        tx.commit().await?;
        return Err(e);
    }
    let rows=sqlx::query("SELECT c.id,c.action,c.status,c.attempts,c.operation_id,c.error_code,c.created_at,c.updated_at,o.status AS operation_status,c.expected_revision_id,c.employee_lifecycle_epoch,
        o.result_revision_id, NOT EXISTS(SELECT 1 FROM employee_management_commands x WHERE x.company_id=c.company_id AND x.employee_id=c.employee_id AND x.status IN ('pending','running')) AS idle,
        EXISTS(SELECT 1 FROM provisioning_operation_steps s WHERE s.company_id=c.company_id AND s.operation_id=c.operation_id AND s.state NOT IN ('succeeded','skipped') AND s.attempt_count>=3) OR EXISTS(SELECT 1 FROM provisioning_runtime_probes p WHERE p.company_id=c.company_id AND p.operation_id=c.operation_id AND p.generation=20 AND (p.state='failed' OR (p.state='succeeded' AND p.deadline<=clock_timestamp()))) AS exhausted,
        (SELECT jsonb_build_object('state',p.state,'generation',p.generation) FROM provisioning_runtime_probes p WHERE p.company_id=c.company_id AND p.operation_id=c.operation_id ORDER BY generation DESC LIMIT 1) AS runtime_probe
        FROM employee_management_commands c LEFT JOIN provisioning_operations o ON o.company_id=c.company_id AND o.id=c.operation_id
        WHERE c.company_id=$1 AND c.employee_id=$2 ORDER BY c.created_at DESC,c.id DESC LIMIT 25")
        .bind(p.scope.company_id()).bind(employee.as_str()).fetch_all(&mut *tx).await?;
    let (revision, status, epoch) = baseline(&mut tx, &p, employee.as_str()).await?;
    let mut commands = Vec::new();
    for row in rows {
        let operation_status: Option<String> = row.try_get("operation_status")?;
        let available = row.try_get::<bool, _>("idle")?
            && row
                .try_get::<Option<Uuid>, _>("result_revision_id")?
                .is_none();
        let can_retry = available
            && row.try_get::<i64, _>("employee_lifecycle_epoch")? == epoch
            && ((row.try_get::<String, _>("action")? == "reenable")
                == (status.as_deref() == Some("disabled")))
            && !row.try_get::<bool, _>("exhausted")?
            && matches!(
                operation_status.as_deref(),
                Some("pending" | "running" | "failed")
            );
        let can_compensate =
            available && matches!(operation_status.as_deref(), Some("failed" | "compensating"));
        commands.push(json!({"command_id":row.try_get::<Uuid,_>("id")?,"action":row.try_get::<String,_>("action")?,"status":row.try_get::<String,_>("status")?,"attempts":row.try_get::<i32,_>("attempts")?,"operation_id":row.try_get::<Option<Uuid>,_>("operation_id")?,"error_code":row.try_get::<Option<String>,_>("error_code")?,"created_at":row.try_get::<chrono::DateTime<chrono::Utc>,_>("created_at")?,"updated_at":row.try_get::<chrono::DateTime<chrono::Utc>,_>("updated_at")?,"can_retry":can_retry,"can_compensate":can_compensate,"runtime_probe":row.try_get::<Option<Value>,_>("runtime_probe")?}));
    }
    policy::audit(
        &mut tx,
        &p,
        Some(employee.as_str()),
        None,
        "command",
        "read",
    )
    .await?;
    let counts=sqlx::query("SELECT count(*) FILTER(WHERE r.status IN('queued','running','waiting')) AS active, count(*) FILTER(WHERE stop.state='pending') AS pending, count(*) FILTER(WHERE stop.state='failed') AS failed FROM runs r JOIN employees e ON e.company_id=r.company_id AND e.id=r.employee_id LEFT JOIN runtime_cancellations stop ON stop.company_id=r.company_id AND stop.run_id=r.id WHERE r.company_id=$1 AND r.employee_id=$2 AND r.employee_lifecycle_epoch<>e.lifecycle_epoch")
        .bind(p.scope.company_id()).bind(employee.as_str()).fetch_one(&mut *tx).await?;
    let busy:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM employee_management_commands WHERE company_id=$1 AND employee_id=$2 AND status IN('pending','running'))")
        .bind(p.scope.company_id()).bind(employee.as_str()).fetch_one(&mut *tx).await?;
    let lifecycle = json!({"can_disable":!busy && matches!(status.as_deref(),Some("active"|"paused")),"old_active_runs":counts.try_get::<i64,_>("active")?,"pending_stops":counts.try_get::<i64,_>("pending")?,"failed_stops":counts.try_get::<i64,_>("failed")?});
    tx.commit().await?;
    Ok(Json(
        json!({"employee_id":employee,"lifecycle":lifecycle,"commands":commands,"expected_revision_id":revision,"expected_lifecycle_epoch":epoch,"status":status,"lifecycle_supported":true}),
    ))
}
