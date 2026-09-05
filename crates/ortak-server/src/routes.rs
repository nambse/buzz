use std::{sync::Arc, time::Duration};

use axum::{
    extract::{Path, Query, State},
    http::{header, HeaderValue, Method, StatusCode},
    middleware,
    routing::{get, post},
    Extension, Json, Router,
};
use buzz_auth::Nip98ReplayGuard;
use ortak_control::PgControlPlane;
use ortak_domain::EmployeeId;
use ortak_observability::{
    ActivityQueries, RunEventsQuery, RunListCursor, RunListPage, RunListQuery, RunStatus,
};
use serde::Deserialize;
use tower_http::{cors::CorsLayer, set_header::SetResponseHeaderLayer, timeout::TimeoutLayer};
use uuid::Uuid;

use crate::{
    auth::{authenticate, Principal},
    config::ApiConfig,
    error::{ApiError, Result},
};

#[derive(Clone)]
pub(crate) struct ApiState {
    pub(crate) control: PgControlPlane,
    pub(crate) config: Arc<ApiConfig>,
    pub(crate) replay: Arc<dyn Nip98ReplayGuard>,
}

/// Constructs the actual PostgreSQL product API with a shared replay fence.
/// The supplied configuration is validated before routes are made available.
pub fn product_router(
    control: PgControlPlane,
    config: ApiConfig,
    replay: Arc<dyn Nip98ReplayGuard>,
) -> std::result::Result<Router, &'static str> {
    let connections = control.pool().options().get_max_connections();
    if connections < 2 {
        return Err("API pool requires at least two connections");
    }
    let concurrency = (connections / 2).clamp(1, 16) as usize;
    let config = config.validate()?;
    let origins = config
        .allowed_web_origins
        .iter()
        .map(|origin| HeaderValue::from_str(origin).map_err(|_| "invalid web origin header"))
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let cors = CorsLayer::new()
        .allow_origin(origins)
        .allow_methods([Method::GET, Method::POST])
        .allow_headers([header::AUTHORIZATION, header::CONTENT_TYPE])
        .max_age(Duration::from_secs(300));
    let state = ApiState {
        control,
        config: Arc::new(config),
        replay,
    };
    Ok(Router::new()
        .route("/api/v1/employees", get(crate::employees::list))
        .route(
            "/api/v1/employees/{employee_id}",
            get(crate::employees::detail),
        )
        .route("/api/v1/runs", get(list_runs))
        .route("/api/v1/runs/{run_id}", get(run_detail))
        .route("/api/v1/runs/{run_id}/events", get(run_events))
        .route("/api/v1/runs/{run_id}/cancel", post(crate::cancel::request))
        .merge(crate::work::router())
        .route_layer(middleware::from_fn_with_state(state.clone(), authenticate))
        .with_state(state)
        .layer(cors)
        .layer(tower::limit::ConcurrencyLimitLayer::new(concurrency))
        .layer(TimeoutLayer::with_status_code(
            StatusCode::REQUEST_TIMEOUT,
            Duration::from_secs(15),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            header::CACHE_CONTROL,
            HeaderValue::from_static("no-store"),
        )))
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ListQuery {
    cursor: Option<String>,
    limit: Option<u32>,
    employee_id: Option<EmployeeId>,
    status: Option<RunStatus>,
}

async fn list_runs(
    State(state): State<ApiState>,
    Extension(principal): Extension<Principal>,
    Query(query): Query<ListQuery>,
) -> Result<Json<RunListPage>> {
    if query
        .employee_id
        .as_ref()
        .is_some_and(|id| !principal.grant.employee_ids.contains(id))
    {
        state
            .audit_principal(&principal, "read_runs", "denied", None)
            .await?;
        return Err(ApiError::not_found());
    }
    let query = RunListQuery {
        cursor: query
            .cursor
            .as_deref()
            .map(RunListCursor::decode)
            .transpose()?,
        limit: Some(query.limit.unwrap_or(25).clamp(1, 25)),
        employee_id: query.employee_id,
        statuses: query.status.into_iter().collect(),
        ..RunListQuery::default()
    };
    Ok(Json(state.visible_runs(&principal, &query).await?))
}

async fn run_detail(
    State(state): State<ApiState>,
    Extension(principal): Extension<Principal>,
    Path(run_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>> {
    state.require_run(&principal, run_id, "read_run").await?;
    let detail = state.control.run_detail(&principal.scope, run_id).await?;
    let cancellation = state.cancellation(&principal, run_id).await?;
    let office_delivery = state.office_delivery(&principal, run_id).await?;
    let memory = state.run_memory(&principal, run_id).await?;
    let can_request_cancel = principal.grant.role == crate::Role::Operator
        && !detail.run.status.is_terminal()
        && cancellation.is_none();
    Ok(Json(
        serde_json::json!({"detail": detail, "cancellation": cancellation, "can_request_cancel": can_request_cancel, "office_delivery": office_delivery, "memory": memory}),
    ))
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct EventsQuery {
    after_sequence: Option<i64>,
    limit: Option<u32>,
}

async fn run_events(
    State(state): State<ApiState>,
    Extension(principal): Extension<Principal>,
    Path(run_id): Path<Uuid>,
    Query(query): Query<EventsQuery>,
) -> Result<Json<ortak_observability::RunEventPage>> {
    state.require_run(&principal, run_id, "read_events").await?;
    let query = RunEventsQuery {
        after_sequence: query.after_sequence,
        limit: Some(query.limit.unwrap_or(100).clamp(1, 100)),
        include_raw: false,
    };
    Ok(Json(
        state
            .control
            .run_events(&principal.scope, run_id, &query)
            .await?,
    ))
}
