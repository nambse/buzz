//! A message-scoped read of persisted routing, including decisions with no run.
use axum::{
    extract::{Path, State},
    Extension, Json,
};
use nostr::EventId;
use serde_json::{json, Value};
use sqlx::Row;
use uuid::Uuid;

use crate::{
    auth::{Principal, RequestAuthority},
    error::{ApiError, Result},
    routes::ApiState,
};

#[path = "routing_read/projection.rs"]
mod projection;

pub(super) async fn detail(
    State(state): State<ApiState>,
    Extension(principal): Extension<Principal>,
    Extension(authority): Extension<RequestAuthority>,
    Path((channel, message)): Path<(Uuid, String)>,
) -> Result<Json<Value>> {
    let message = EventId::from_hex(&message).map_err(|_| ApiError::invalid())?;
    let mut held = authority.0.lock().await;
    let connection = held.as_mut().ok_or_else(ApiError::unavailable)?;
    Ok(Json(current_on(&state, &principal, channel, message, connection).await?))
}

/// Current canonical audience, borrowed from a caller-held Office authority fence.
pub(super) async fn visible_on(
    state: &ApiState,
    principal: &Principal,
    channel: Uuid,
    message: EventId,
    connection: &mut sqlx::PgConnection,
) -> Result<Option<ortak_domain::EmployeeId>> {
    // Reuse the middleware's Office shared fence through the entire projection.
    // Canonical source/audience comes first, even when no inbox/decision exists.
    let channel_type: Option<String> = sqlx::query_scalar(
        "SELECT c.channel_type::text FROM office_company_bindings b
        JOIN events e ON e.community_id=b.community_id AND e.id=$3 AND e.channel_id=$4
        JOIN channels c ON c.community_id=e.community_id AND c.id=e.channel_id
        WHERE b.company_id=$1 AND b.community_id=$2 AND e.deleted_at IS NULL
          AND e.kind IN(9,40002) AND c.deleted_at IS NULL AND e.channel_id=ANY($5)
          AND ((c.channel_type::text='dm' AND c.visibility::text='private')
            OR (c.channel_type::text='stream' AND (c.visibility::text='open' OR EXISTS(
            SELECT 1 FROM channel_members m WHERE m.community_id=b.community_id
              AND m.channel_id=c.id AND m.pubkey=$6 AND m.removed_at IS NULL))))",
    )
    .bind(principal.scope.company_id())
    .bind(state.config.community_id)
    .bind(message.to_bytes().as_slice())
    .bind(channel)
    .bind(&principal.grant.channel_ids)
    .bind(principal.public_key.to_bytes().as_slice())
    .fetch_optional(&mut *connection)
    .await?;
    let mut direct_employee = None;
    let visible = match channel_type.as_deref() {
        Some("stream") => true,
        Some("dm") => {
            let direct = ortak_control::postgres::direct_channel_on(
                &mut *connection,
                principal.scope.company_id(),
                Some(state.config.community_id),
                channel,
            )
            .await
            .map_err(|_| ApiError::unavailable())?;
            if let Some(direct) = direct.filter(|direct| {
                direct.visible_to(&principal.public_key.to_bytes())
                    && principal.grant.employee_ids.contains(&direct.employee_id)
            }) {
                direct_employee = Some(direct.employee_id);
                true
            } else {
                false
            }
        }
        _ => false,
    };
    if !visible {
        crate::store::audit_on(
            &mut *connection,
            &principal.scope,
            &principal.public_key,
            &principal.auth_event_id,
            "access",
            "not_found",
            None,
        )
        .await?;
        return Err(ApiError::not_found());
    }
    Ok(direct_employee)
}

/// One bounded projection for HTTP and SSE; never uses a cached audience grant.
pub(super) async fn current_on(
    state: &ApiState,
    principal: &Principal,
    channel: Uuid,
    message: EventId,
    connection: &mut sqlx::PgConnection,
) -> Result<Value> {
    let direct_employee = visible_on(state, principal, channel, message, &mut *connection).await?;
    // Select only safe scalar metadata. Never load raw prompt/input, excluded
    // targets, candidate revision sets or arbitrary scorer usage into this API.
    let row = sqlx::query("SELECT d.id,d.mode,d.summary_reason,d.policy_version,d.decided_at,
        left(d.scorer_adapter,129) AS scorer_adapter,left(d.scorer_model,129) AS scorer_model,
        left(d.scorer_prompt_version,129) AS scorer_prompt_version,left(d.scorer_version,129) AS scorer_version,d.scorer_latency_ms,
        to_jsonb(left(d.scorer_usage->>'reasoning_effort',129)) AS effort,
        CASE WHEN jsonb_typeof(d.scorer_usage->'cache_hit')='boolean' THEN d.scorer_usage->'cache_hit' END AS cached,
        to_jsonb(left(d.scorer_usage->>'failure_code',129)) AS failure_code,
        CASE WHEN jsonb_typeof(d.scorer_usage->'prompt_tokens')='number' THEN left(d.scorer_usage->>'prompt_tokens',16) END AS input_tokens,
        CASE WHEN jsonb_typeof(d.scorer_usage->'completion_tokens')='number' THEN left(d.scorer_usage->>'completion_tokens',16) END AS output_tokens,
        CASE WHEN jsonb_typeof(d.scorer_usage->'total_tokens')='number' THEN left(d.scorer_usage->>'total_tokens',16) END AS total_tokens
        FROM routing_decisions d JOIN office_inbox i ON i.company_id=d.company_id AND i.event_id=d.message_id
        JOIN events e ON e.community_id=$2 AND e.id=i.event_id AND e.created_at=i.event_created_at
          AND e.channel_id=i.channel_id AND e.kind=i.event_kind AND e.pubkey=i.author_pubkey
        WHERE d.company_id=$1 AND d.message_id=$3 AND i.channel_id=$4 AND e.deleted_at IS NULL
          AND i.state='decided'")
        .bind(principal.scope.company_id()).bind(state.config.community_id)
        .bind(message.to_bytes().as_slice()).bind(channel).fetch_optional(&mut *connection).await?;
    let decision = if let Some(row) = row {
        let id: Uuid = row.try_get("id")?;
        let granted = principal
            .grant
            .employee_ids
            .iter()
            .filter(|id| {
                direct_employee
                    .as_ref()
                    .is_none_or(|employee| *id == employee)
            })
            .map(|id| id.as_str())
            .collect::<Vec<_>>();
        let recipients = sqlx::query(
            "SELECT employee_id,action,reason,score,
            coalesce((SELECT jsonb_agg(label) FROM jsonb_array_elements_text(evidence) label
                WHERE label IN('responsibility_match','domain_match','role_match','insufficient_context','no_match')),'[]'::jsonb) AS evidence
            FROM routing_recipients WHERE company_id=$1 AND routing_decision_id=$2
              AND employee_id=ANY($3) ORDER BY position LIMIT 33",
        )
        .bind(principal.scope.company_id())
        .bind(id)
        .bind(granted)
        .fetch_all(&mut *connection)
        .await?;
        Some(projection::decision(&row, &recipients)?)
    } else {
        // Absence is a product claim. A retained decision with broken source
        // pins must remain an unavailable read, not masquerade as no decision.
        // A concurrent first commit may also yield a retryable unavailable here.
        let persisted: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM routing_decisions WHERE company_id=$1 AND message_id=$2)",
        )
        .bind(principal.scope.company_id())
        .bind(message.to_bytes().as_slice())
        .fetch_one(&mut *connection)
        .await?;
        if persisted {
            return Err(ApiError::unavailable());
        }
        None
    };
    Ok(json!({"message_id":message.to_hex(),"channel_id":channel,"decision":decision}))
}
