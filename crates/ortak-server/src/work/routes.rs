use axum::{
    extract::{
        rejection::{JsonRejection, QueryRejection},
        Path, Query, State,
    },
    http::StatusCode,
    routing::{get, post},
    Extension, Json, Router,
};
use ortak_work::{WorkListCursor, WorkListQuery, WorkMutation};
use serde_json::{json, Value};
use sqlx::Row;
use uuid::Uuid;

use super::{authorized, dto::*, projection};
use crate::{
    auth::Principal,
    error::{ApiError, Result},
    routes::ApiState,
};

type Body<T> = std::result::Result<Json<T>, JsonRejection>;
type Params<T> = std::result::Result<Query<T>, QueryRejection>;

pub(crate) fn router() -> Router<ApiState> {
    Router::new()
        .route(
            "/api/v1/employees/{employee_id}/work-items",
            get(super::queue::employee_queue),
        )
        .route("/api/v1/projects", get(projects).post(create_project))
        .route("/api/v1/projects/{project_id}", get(project))
        .route(
            "/api/v1/projects/{project_id}/conversation-memory",
            get(super::conversation_memory::list).post(super::conversation_memory::approve),
        )
        .route(
            "/api/v1/projects/{project_id}/conversation-memory/preview",
            post(super::conversation_memory::preview),
        )
        .route(
            "/api/v1/projects/{project_id}/conversation-memory/{fact_id}/stop",
            post(super::conversation_memory::revoke),
        )
        .route(
            "/api/v1/projects/{project_id}/conversation-memory/{fact_id}/publish",
            post(super::reviewed_exports::publish_conversation),
        )
        .route(
            "/api/v1/projects/{project_id}/conversation-memory/{fact_id}/exports/{action}/retry",
            post(super::reviewed_exports::retry_conversation),
        )
        .route(
            "/api/v1/projects/{project_id}/reviewed-memory",
            get(super::facts::list).post(super::facts::approve),
        )
        .route(
            "/api/v1/projects/{project_id}/reviewed-memory/recall",
            post(super::facts::recall),
        )
        .route(
            "/api/v1/projects/{project_id}/reviewed-memory/{fact_id}/stop",
            post(super::facts::revoke),
        )
        .route(
            "/api/v1/projects/{project_id}/reviewed-memory/{fact_id}/publish",
            post(super::reviewed_exports::publish),
        )
        .route(
            "/api/v1/projects/{project_id}/reviewed-memory/{fact_id}/exports/{action}/retry",
            post(super::reviewed_exports::retry),
        )
        .route(
            "/api/v1/projects/{project_id}/work-items",
            get(work_list).post(create_work),
        )
        .route("/api/v1/projects/{project_id}/promotions", post(promote))
        .route("/api/v1/work-items/{item_id}", get(work_detail))
        .route(
            "/api/v1/work-items/{item_id}/decomposition",
            get(super::decomposition::list),
        )
        .route(
            "/api/v1/work-items/{item_id}/children",
            post(super::decomposition::create),
        )
        .route(
            "/api/v1/work-items/{item_id}/dependencies",
            get(super::dependencies::list).post(super::dependencies::add),
        )
        .route(
            "/api/v1/work-items/{item_id}/dependencies/{dependency_id}/remove",
            post(super::dependencies::remove),
        )
        .route(
            "/api/v1/work-items/{item_id}/executions",
            get(super::execution::list).post(super::execution::start),
        )
        .route(
            "/api/v1/work-items/{item_id}/artifacts/{artifact_id}",
            get(super::execution::artifact),
        )
        .route(
            "/api/v1/work-items/{item_id}/definition",
            post(super::definition::edit),
        )
        .route("/api/v1/work-items/{item_id}/assignments", post(assign))
        .route(
            "/api/v1/work-items/{item_id}/assignments/{employee_id}/release",
            post(super::assignment::release),
        )
        .route(
            "/api/v1/work-items/{item_id}/assignments/{employee_id}/reassign",
            post(super::assignment::reassign),
        )
        .route("/api/v1/work-items/{item_id}/transitions", post(transition))
        .route(
            "/api/v1/work-items/{item_id}/criteria/{criterion_id}/satisfy",
            post(satisfy),
        )
        .route(
            "/api/v1/work-items/{item_id}/approvals/{approval_id}/resolve",
            post(resolve),
        )
}

async fn projects(
    State(state): State<ApiState>,
    Extension(p): Extension<Principal>,
    query: Params<PageQuery>,
) -> Result<Json<Value>> {
    let Query(query) = query.map_err(|_| ApiError::invalid())?;
    let page = authorized(&state, &p)?
        .list_projects(
            query.cursor.as_deref(),
            query.limit.unwrap_or(25).clamp(1, 25),
        )
        .await?;
    let items: Vec<_> = page
        .items
        .iter()
        .map(|project| projection::project(project, false, &p))
        .collect();
    let channels = creation_channels(&state, &p).await?;
    projection::bounded(json!({"projects": items, "next_cursor": page.next_cursor,
        "can_create_projects": !channels.is_empty(), "create_channels": channels}))
}

async fn project(
    State(state): State<ApiState>,
    Extension(p): Extension<Principal>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>> {
    let result = authorized(&state, &p)?.project(id).await?;
    projection::bounded(json!({"project": projection::project(&result, true, &p)}))
}

// The outer authentication transaction keeps current Office facts stable while
// this bounded list supplies names for a channel selector. No client supplies
// an audience, and public channels still require actual membership for Work.
async fn creation_channels(state: &ApiState, p: &Principal) -> Result<Vec<Value>> {
    if p.grant.role != crate::Role::Operator || !p.grant.can_create_projects {
        return Ok(Vec::new());
    }
    let rows = sqlx::query(
        "SELECT c.id,c.name FROM channels c JOIN channel_members m
         ON m.community_id=c.community_id AND m.channel_id=c.id
         WHERE c.community_id=$1 AND c.id=ANY($2) AND c.channel_type::text='stream'
           AND c.deleted_at IS NULL AND m.pubkey=$3 AND m.removed_at IS NULL
         ORDER BY c.name,c.id LIMIT 64",
    )
    .bind(state.config.community_id)
    .bind(&p.grant.channel_ids)
    .bind(p.public_key.to_bytes().as_slice())
    .fetch_all(state.control.pool())
    .await?;
    rows.iter()
        .map(|row| {
            Ok(projection::channel(
                row.try_get("id")?,
                row.try_get("name")?,
            ))
        })
        .collect()
}

async fn create_project(
    State(state): State<ApiState>,
    Extension(p): Extension<Principal>,
    body: Body<CreateProject>,
) -> Result<(StatusCode, Json<Value>)> {
    let Json(body) = body.map_err(|_| ApiError::invalid())?;
    let result = authorized(&state, &p)?
        .create_project(body.operation_id, body.channel_id, body.project)
        .await?;
    let status = if result.created {
        StatusCode::CREATED
    } else {
        StatusCode::OK
    };
    Ok((
        status,
        projection::bounded(
            json!({"project": projection::project(&result.project, true, &p), "created": result.created}),
        )?,
    ))
}

async fn work_list(
    State(state): State<ApiState>,
    Extension(p): Extension<Principal>,
    Path(id): Path<Uuid>,
    query: Params<WorkQuery>,
) -> Result<Json<Value>> {
    let Query(query) = query.map_err(|_| ApiError::invalid())?;
    let page = authorized(&state, &p)?
        .list_project_work(
            id,
            &WorkListQuery {
                cursor: query
                    .cursor
                    .as_deref()
                    .map(|value| {
                        let (project, cursor) =
                            value.split_once('/').ok_or_else(ApiError::invalid)?;
                        if Uuid::parse_str(project).ok() != Some(id) {
                            return Err(ApiError::invalid());
                        }
                        WorkListCursor::decode(cursor).map_err(ApiError::from)
                    })
                    .transpose()?,
                limit: Some(query.limit.unwrap_or(25).clamp(1, 25)),
                states: query.state.into_iter().collect(),
            },
        )
        .await?;
    let items: Vec<_> = page.items.iter().map(projection::summary).collect();
    projection::bounded(
        json!({"work_items": items, "next_cursor": page.next_cursor.map(|c| format!("{id}/{}", c.encode()))}),
    )
}

async fn work_detail(
    State(state): State<ApiState>,
    Extension(p): Extension<Principal>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>> {
    let result = authorized(&state, &p)?.work_item(id).await?;
    projection::bounded(json!({"work_item": projection::item(&result, &p)}))
}

async fn create_work(
    State(state): State<ApiState>,
    Extension(p): Extension<Principal>,
    Path(id): Path<Uuid>,
    body: Body<CreateWork>,
) -> Result<(StatusCode, Json<Value>)> {
    create(&state, &p, id, body, false).await
}

async fn promote(
    State(state): State<ApiState>,
    Extension(p): Extension<Principal>,
    Path(id): Path<Uuid>,
    body: Body<CreateWork>,
) -> Result<(StatusCode, Json<Value>)> {
    create(&state, &p, id, body, true).await
}

async fn create(
    state: &ApiState,
    p: &Principal,
    project: Uuid,
    body: Body<CreateWork>,
    promotion: bool,
) -> Result<(StatusCode, Json<Value>)> {
    let Json(body) = body.map_err(|_| ApiError::invalid())?;
    let (operation, input) = body.input(project, promotion)?;
    let result = authorized(state, p)?
        .create_work_item(operation, input)
        .await?;
    let status = if result.created {
        StatusCode::CREATED
    } else {
        StatusCode::OK
    };
    Ok((
        status,
        projection::bounded(
            json!({"work_item": projection::item(&result.item, p), "created": result.created}),
        )?,
    ))
}

async fn assign(
    State(state): State<ApiState>,
    Extension(p): Extension<Principal>,
    Path(id): Path<Uuid>,
    body: Body<Assignment>,
) -> Result<Json<Value>> {
    let Json(body) = body.map_err(|_| ApiError::invalid())?;
    mutate(
        &state,
        &p,
        body.operation_id,
        id,
        body.expected_version,
        WorkMutation::Assign {
            employee_id: body.employee_id,
            role: body.role,
        },
    )
    .await
}

async fn transition(
    State(state): State<ApiState>,
    Extension(p): Extension<Principal>,
    Path(id): Path<Uuid>,
    body: Body<Transition>,
) -> Result<Json<Value>> {
    let Json(body) = body.map_err(|_| ApiError::invalid())?;
    mutate(
        &state,
        &p,
        body.operation_id,
        id,
        body.expected_version,
        WorkMutation::Transition {
            target: body.target,
            reason: body.reason,
        },
    )
    .await
}

async fn satisfy(
    State(state): State<ApiState>,
    Extension(p): Extension<Principal>,
    Path((id, criterion_id)): Path<(Uuid, Uuid)>,
    body: Body<Criterion>,
) -> Result<Json<Value>> {
    let Json(body) = body.map_err(|_| ApiError::invalid())?;
    mutate(
        &state,
        &p,
        body.operation_id,
        id,
        body.expected_version,
        WorkMutation::SatisfyCriterion { criterion_id },
    )
    .await
}

async fn resolve(
    State(state): State<ApiState>,
    Extension(p): Extension<Principal>,
    Path((id, approval_id)): Path<(Uuid, Uuid)>,
    body: Body<Approval>,
) -> Result<Json<Value>> {
    let Json(body) = body.map_err(|_| ApiError::invalid())?;
    mutate(
        &state,
        &p,
        body.operation_id,
        id,
        body.expected_version,
        WorkMutation::ResolveApproval {
            approval_id,
            decision: body.decision,
            reason: body.reason,
        },
    )
    .await
}

pub(super) async fn mutate(
    state: &ApiState,
    p: &Principal,
    operation: Uuid,
    id: Uuid,
    version: i64,
    mutation: WorkMutation,
) -> Result<Json<Value>> {
    let result = authorized(state, p)?
        .mutate(operation, id, version, mutation)
        .await?;
    projection::bounded(json!({"work_item": projection::item(&result, p)}))
}
