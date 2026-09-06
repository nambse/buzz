//! Canonical delivery authorization inside the shared Office mutation fence.

use nostr::JsonUtil;
use ortak_control::inbox::InboxEvent;
use ortak_control::ports::Normalization;
use ortak_control::service::office_input_hash;
use ortak_control::{CompanyScope, ControlError, MessageId, PgControlPlane};
use ortak_domain::CredentialRef;
use sqlx::{PgConnection, Row};
use uuid::Uuid;

use super::{authorize, Provenance};
use crate::{FrozenSignedEvent, OfficePublishDraft, PgChannelNormalizer, Result};

pub(super) fn denied() -> crate::OfficeDeliveryError {
    ControlError::InvalidData("office delivery authority is no longer valid".to_owned()).into()
}

/// Acquire before run/outbox row locks; no network call may retain this tx.
pub(super) async fn lock(connection: &mut PgConnection, scope: &CompanyScope) -> Result<()> {
    sqlx::query("SELECT set_config('lock_timeout','500ms',true), set_config('statement_timeout','2s',true), set_config('idle_in_transaction_session_timeout','5s',true)")
        .execute(&mut *connection).await?;
    sqlx::query("SELECT ortak_lock_office_authority($1)")
        .bind(scope.company_id())
        .execute(connection)
        .await?;
    Ok(())
}

pub(super) async fn channel(
    connection: &mut PgConnection,
    scope: &CompanyScope,
    draft: &OfficePublishDraft,
    identity: &Provenance,
) -> Result<Uuid> {
    let row = sqlx::query(
        "SELECT cb.community_id,i.event_id,i.event_created_at,i.event_kind,
                i.author_pubkey,i.channel_id,r.root_message_id,r.delivery_intent,d.office_input_hash,
                r.routing_decision_id,j.run_id AS output_job_run_id,j.source_facts,
                j.draft_kind,j.draft_tags,j.draft_content
         FROM runs r
         JOIN companies c ON c.id=r.company_id AND c.status='active'
         JOIN office_company_bindings cb ON cb.company_id=c.id
         JOIN communities cm ON cm.id=cb.community_id
             AND cm.deletion_state='active' AND cm.deleted_at IS NULL
         JOIN employees e ON e.company_id=r.company_id AND e.id=r.employee_id
             AND e.status='active' AND e.lifecycle_epoch=r.employee_lifecycle_epoch
         JOIN employee_revisions current_rev ON current_rev.company_id=e.company_id
             AND current_rev.employee_id=e.id AND current_rev.id=e.active_revision_id
         JOIN routing_decisions d ON d.company_id=r.company_id AND d.id=r.routing_decision_id
             AND d.message_id=r.message_id AND d.root_message_id=r.root_message_id
         JOIN routing_recipients rr ON rr.company_id=r.company_id AND rr.routing_decision_id=d.id
             AND rr.employee_id=r.employee_id AND rr.employee_revision_id=r.employee_revision_id AND rr.action='wake'
         JOIN office_inbox i ON i.company_id=r.company_id AND i.event_id=r.message_id AND i.state='decided'
         LEFT JOIN runtime_office_outputs j ON j.company_id=r.company_id AND j.run_id=r.id
         WHERE r.company_id=$1 AND r.id=$2 AND r.work_item_id IS NULL
           AND lower(current_rev.manifest #>> '{office,public_key}')=$3
           AND current_rev.manifest #>> '{office,signer_ref}'=$4
           AND NOT EXISTS (SELECT 1 FROM runtime_cancellations x WHERE x.company_id=r.company_id AND x.run_id=r.id)
           AND NOT EXISTS (SELECT 1 FROM run_cancel_requests x WHERE x.company_id=r.company_id AND x.run_id=r.id)
           AND NOT EXISTS (SELECT 1 FROM users u WHERE u.community_id=cb.community_id
                            AND u.pubkey=$5 AND u.deactivated_at IS NOT NULL)",
    ).bind(scope.company_id()).bind(draft.run_id).bind(identity.public_key.to_hex())
        .bind(identity.signer_ref.as_str()).bind(identity.public_key.as_bytes().as_slice())
        .fetch_optional(&mut *connection).await?.ok_or_else(denied)?;
    let community_id: Uuid = row.try_get("community_id")?;
    if scope.community_id().is_some_and(|id| id != community_id) {
        return Err(denied());
    }
    let event_id: Vec<u8> = row.try_get("event_id")?;
    let author: Vec<u8> = row.try_get("author_pubkey")?;
    let inbox = InboxEvent {
        event_id: MessageId::try_from_slice(&event_id)?,
        event_created_at: row.try_get("event_created_at")?,
        event_kind: row.try_get("event_kind")?,
        author_pubkey: author.try_into().map_err(|_| denied())?,
        channel_id: row.try_get("channel_id")?,
    };
    let Normalization::Message(normalized) =
        PgChannelNormalizer::normalize_on(connection, scope, &inbox).await?
    else {
        return Err(denied());
    };
    if !normalized
        .eligible_employee_ids
        .contains(&identity.employee_id)
    {
        return Err(denied());
    }
    let hash = office_input_hash(
        &normalized.envelope,
        normalized.root_message_id,
        &normalized.eligible_employee_ids,
    );
    if row
        .try_get::<Option<Vec<u8>>, _>("office_input_hash")?
        .as_deref()
        != Some(hash.as_slice())
    {
        return Err(denied());
    }
    let root: Vec<u8> = row.try_get("root_message_id")?;
    if normalized.root_message_id.as_bytes().as_slice() != root {
        return Err(denied());
    }
    let channel = inbox.channel_id.ok_or_else(denied)?;
    let mut tags = vec![vec!["h".to_owned(), channel.to_string()]];
    let intent: String = row.try_get("delivery_intent")?;
    if row
        .try_get::<Option<Uuid>, _>("output_job_run_id")?
        .is_some()
    {
        let expected = serde_json::json!({
            "employee_id": identity.employee_id.as_str(),
            "employee_revision_id": identity.employee_revision_id.to_string(),
            "routing_decision_id": row.try_get::<Uuid, _>("routing_decision_id")?.to_string(),
            "message_id": inbox.event_id.to_hex(),
            "root_message_id": normalized.root_message_id.to_hex(),
            "delivery_intent": intent,
            "office_input_hash": hex::encode(hash),
        });
        if row
            .try_get::<Option<serde_json::Value>, _>("source_facts")?
            .as_ref()
            != Some(&expected)
            || row.try_get::<Option<i32>, _>("draft_kind")? != Some(i32::from(draft.kind))
            || row.try_get::<Option<serde_json::Value>, _>("draft_tags")?
                != Some(serde_json::json!(draft.tags))
            || row
                .try_get::<Option<String>, _>("draft_content")?
                .as_deref()
                != Some(draft.content.as_str())
        {
            return Err(denied());
        }
    }
    if intent == "reply" {
        let thread_root = super::reply_root_on(connection, scope, &inbox)
            .await?
            .ok_or_else(denied)?;
        if thread_root != inbox.event_id {
            tags.push(vec![
                "e".to_owned(),
                thread_root.to_hex(),
                String::new(),
                "root".to_owned(),
            ]);
        }
        tags.push(vec![
            "e".to_owned(),
            inbox.event_id.to_hex(),
            String::new(),
            "reply".to_owned(),
        ]);
    }
    if draft.kind != inbox.event_kind as u16 || draft.tags != tags {
        return Err(denied());
    }
    Ok(community_id)
}

pub(super) fn draft(event: &FrozenSignedEvent) -> Result<OfficePublishDraft> {
    let parsed = nostr::Event::from_json(event.signed_bytes()).map_err(|_| denied())?;
    Ok(OfficePublishDraft {
        company_id: event.company_id(),
        run_id: event.run_id(),
        kind: event.kind(),
        tags: parsed
            .tags
            .iter()
            .map(|tag| tag.as_slice().to_vec())
            .collect(),
        content: parsed.content,
    })
}

pub(super) fn matches(current: &crate::AuthorizedOfficePublish, event: &FrozenSignedEvent) -> bool {
    current.employee_id() == event.employee_id()
        && current.employee_revision_id() == event.employee_revision_id()
        && current.public_key() == event.public_key()
        && current.fingerprint() == event.fingerprint()
}

pub(crate) async fn before_publish(
    control: &PgControlPlane,
    scope: &CompanyScope,
    event: &FrozenSignedEvent,
    expected_host: &str,
) -> Result<(Uuid, CredentialRef)> {
    if event.company_id() != scope.company_id() {
        return Err(denied());
    }
    let mut tx = control.pool().begin().await?;
    lock(&mut tx, scope).await?;
    let current = authorize(&mut tx, scope, Uuid::nil(), &draft(event)?).await?;
    if !matches(&current, event) {
        return Err(denied());
    }
    let community_id: Option<Uuid> = sqlx::query_scalar(
        "SELECT b.community_id FROM outbox o JOIN office_company_bindings b ON b.company_id=o.company_id
         JOIN communities cm ON cm.id=b.community_id AND lower(cm.host)=$5
         WHERE o.company_id=$1 AND o.run_id=$2 AND o.kind='office_publish'
           AND o.signed_event_id=$3 AND o.signed_event_bytes=$4 AND o.state='pending'
           AND o.lease_token IS NOT NULL AND o.lease_expires_at>clock_timestamp()",
    ).bind(scope.company_id()).bind(event.run_id()).bind(event.event_id().as_bytes().as_slice())
        .bind(event.signed_bytes()).bind(expected_host).fetch_optional(&mut *tx).await?;
    let result = (
        community_id.ok_or_else(denied)?,
        current.signer_ref().clone(),
    );
    tx.commit().await?;
    Ok(result)
}
