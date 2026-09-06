use axum::{
    body::{to_bytes, Body},
    extract::{Request, State},
    http::StatusCode,
    middleware::Next,
    response::Response,
};
use base64::{engine::general_purpose::STANDARD, Engine};
use buzz_auth::DEFAULT_REPLAY_TTL_SECS;
use nostr::{Event, PublicKey, TagKind};
use ortak_control::{ports::CompanyDirectory, CompanyScope};
use sqlx::Row;
use std::sync::Arc;

use crate::{
    config::HumanGrant,
    error::{ApiError, Result},
    routes::ApiState,
};

#[derive(Clone)]
pub(crate) struct Principal {
    pub(crate) scope: CompanyScope,
    pub(crate) public_key: PublicKey,
    pub(crate) auth_event_id: Vec<u8>,
    pub(crate) grant: HumanGrant,
}

// Ordinary Activity handlers borrow the existing middleware transaction so Work
// project ACL fences survive response construction without a third connection.
// Stream bodies own fresh short transactions after this one has committed.
#[derive(Clone)]
pub(crate) struct RequestAuthority(
    pub(crate) Arc<tokio::sync::Mutex<Option<sqlx::Transaction<'static, sqlx::Postgres>>>>,
);

pub(crate) async fn authenticate(
    State(state): State<ApiState>,
    request: Request,
    next: Next,
) -> Result<Response> {
    let unauthorized = || ApiError(StatusCode::UNAUTHORIZED, "authentication_required");
    if request.headers().get_all("host").iter().count() != 1
        || request.headers().get("host").and_then(|h| h.to_str().ok())
            != Some(state.config.authority())
        || request.headers().get_all("authorization").iter().count() != 1
    {
        return Err(unauthorized());
    }
    let auth = request
        .headers()
        .get("authorization")
        .and_then(|h| h.to_str().ok())
        .and_then(|h| h.strip_prefix("Nostr "))
        .filter(|h| h.len() <= 16_384)
        .ok_or_else(unauthorized)?;
    let decoded = STANDARD.decode(auth).map_err(|_| unauthorized())?;
    let json = std::str::from_utf8(&decoded).map_err(|_| unauthorized())?;
    let event: Event = serde_json::from_str(json).map_err(|_| unauthorized())?;
    let method = request.method().clone();
    let path = request
        .uri()
        .path_and_query()
        .ok_or_else(unauthorized)?
        .as_str();
    if path.len() > 4096 {
        return Err(ApiError::invalid());
    }
    let expected_url = format!("{}{path}", state.config.origin);
    // Employee review permits escaped 4KiB edited text in its exact POST DTOs.
    // Work retains 16KiB; other endpoints retain their existing 4KiB contract.
    let body_limit = if method == axum::http::Method::POST
        && crate::employee_memory::has_review_body(request.uri().path())
    {
        32_768
    } else if request.uri().path() == "/api/v1/projects"
        || request.uri().path().starts_with("/api/v1/projects/")
        || request.uri().path().starts_with("/api/v1/work-items/")
    {
        16_384
    } else {
        4096
    };
    let (parts, body) = request.into_parts();
    let body = to_bytes(body, body_limit)
        .await
        .map_err(|_| ApiError(StatusCode::PAYLOAD_TOO_LARGE, "body_too_large"))?;
    if (method != axum::http::Method::GET || !body.is_empty())
        && event
            .tags
            .iter()
            .filter(|tag| tag.kind() == TagKind::Payload)
            .count()
            != 1
    {
        return Err(unauthorized());
    }
    let public_key =
        buzz_auth::verify_nip98_event(json, &expected_url, method.as_str(), Some(&body))
            .map_err(|_| unauthorized())?;
    let scope = state
        .control
        .resolve_company_for_community(state.config.community_id)
        .await
        .map_err(|error| match error {
            // Purge intentionally retires this transient binding. Retained
            // product records cannot turn its absence into a retryable outage.
            ortak_control::ControlError::UnknownCompanyBinding { .. } => ApiError::not_found(),
            _ => ApiError::unavailable(),
        })?;
    if !state
        .replay
        .try_mark_in_scope(
            &format!("ortak-api:{}", scope.company_id()),
            &event.id,
            DEFAULT_REPLAY_TTL_SECS,
        )
        .await
        .map_err(|_| ApiError::unavailable())?
    {
        return Err(ApiError(
            StatusCode::UNAUTHORIZED,
            "authentication_replayed",
        ));
    }
    let grant = state
        .config
        .humans
        .iter()
        .find(|g| g.public_key == public_key.to_hex());
    // Fence before facts and keep it through response construction. Activity reads
    // use another pool connection; the shared fence prevents Office authority
    // changes across those statements without changing their transaction API.
    let mut authority = state.control.pool().begin().await?;
    lock_authority(&mut authority, &scope).await?;
    let allowed = human_allowed_on(
        &mut authority,
        &scope,
        state.config.community_id,
        &public_key,
    )
    .await?;
    let auth_event_id = event.id.to_bytes().to_vec();
    if !allowed || grant.is_none() {
        state
            .audit(
                &scope,
                &public_key,
                &auth_event_id,
                "access",
                "denied",
                None,
            )
            .await?;
        return Err(ApiError(StatusCode::FORBIDDEN, "forbidden"));
    }
    let grant = grant.cloned().ok_or_else(unauthorized)?;
    let authority = RequestAuthority(Arc::new(tokio::sync::Mutex::new(Some(authority))));
    let mut request = Request::from_parts(parts, Body::from(body));
    request.extensions_mut().insert(authority.clone());
    request.extensions_mut().insert(Principal {
        scope,
        public_key,
        auth_event_id,
        grant,
    });
    let response = next.run(request).await;
    authority
        .0
        .lock()
        .await
        .take()
        .ok_or_else(ApiError::unavailable)?
        .commit()
        .await?;
    Ok(response)
}

pub(crate) async fn lock_authority(
    connection: &mut sqlx::PgConnection,
    scope: &CompanyScope,
) -> Result<()> {
    // Must precede every run, inbox or outbox row lock (B1b lock order).
    sqlx::query("SELECT ortak_lock_office_authority($1)")
        .bind(scope.company_id())
        .execute(connection)
        .await?;
    Ok(())
}

pub(crate) async fn human_allowed_on(
    connection: &mut sqlx::PgConnection,
    scope: &CompanyScope,
    community: uuid::Uuid,
    public_key: &PublicKey,
) -> Result<bool> {
    // A configured human must also remain a live, non-automated Office identity.
    let facts = sqlx::query(
        "SELECT c.status = 'active' AND cm.deletion_state = 'active' AND cm.deleted_at IS NULL AS active,
                EXISTS (SELECT 1 FROM relay_members rm WHERE rm.community_id = $2 AND rm.pubkey = $3)
                OR EXISTS (SELECT 1 FROM channel_members m WHERE m.community_id = $2 AND m.pubkey = $4 AND m.removed_at IS NULL) AS member,
                EXISTS (SELECT 1 FROM users u WHERE u.community_id = $2 AND u.pubkey = $4
                         AND (u.deactivated_at IS NOT NULL OR u.agent_type IS NOT NULL OR u.agent_owner_pubkey IS NOT NULL))
                OR EXISTS (SELECT 1 FROM employee_office_bindings b WHERE b.company_id = $1 AND b.public_key = $4)
                OR EXISTS (SELECT 1 FROM channel_members m WHERE m.community_id = $2 AND m.pubkey = $4 AND m.role = 'bot') AS refused
           FROM companies c JOIN office_company_bindings b ON b.company_id = c.id AND b.community_id = $2 JOIN communities cm ON cm.id = b.community_id WHERE c.id = $1")
        .bind(scope.company_id()).bind(community).bind(public_key.to_hex())
        .bind(public_key.to_bytes().as_slice()).fetch_optional(connection).await?;
    let allowed = facts.as_ref().is_some_and(|r| {
        r.try_get::<bool, _>("active").unwrap_or(false)
            && r.try_get::<bool, _>("member").unwrap_or(false)
            && !r.try_get::<bool, _>("refused").unwrap_or(true)
    });
    Ok(allowed)
}
