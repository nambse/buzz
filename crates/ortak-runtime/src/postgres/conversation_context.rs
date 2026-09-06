//! Ordinary Office source selection under the shared mutation fence.

use chrono::{DateTime, Utc};
use ortak_control::adapter::truncate_at_char_boundary;
use ortak_control::conversation_context::{
    ContextEmployee, ContextMessage, ContextSelection, ConversationContext, MAX_CONTEXT_BYTES,
    MAX_HISTORY_BYTES, MAX_MESSAGES, MAX_MESSAGE_BYTES, MAX_TEAMMATES,
};
use ortak_control::run_event::strip_control_characters;
use ortak_control::CompanyScope;
use ortak_domain::{Employee, EmployeeId};
use sqlx::{PgConnection, Row};
use uuid::Uuid;

use super::invalid;
use crate::authority::DispatchAuthority;
use crate::Result;

fn bounded(value: &str, maximum: usize) -> String {
    let value = strip_control_characters(value);
    truncate_at_char_boundary(&value, maximum).to_owned()
}

fn employee(value: Employee, revision: Uuid) -> ContextEmployee {
    ContextEmployee {
        employee_id: value.id,
        revision_id: revision,
        name: bounded(&value.name, 200),
        title: bounded(&value.title, 200),
        biography: bounded(&value.biography, 4096),
        responsibilities: value
            .responsibilities
            .iter()
            .take(32)
            .map(|v| bounded(v, 512))
            .collect(),
        domains: value
            .domains
            .iter()
            .take(32)
            .map(|v| bounded(v, 128))
            .collect(),
    }
}

/// Freeze-time only. Retry loads the committed snapshot instead of reselecting.
pub(super) async fn select(
    connection: &mut PgConnection,
    scope: &CompanyScope,
    authority: &DispatchAuthority,
    run: Uuid,
) -> Result<Option<ConversationContext>> {
    if authority.work_origin().is_some() {
        return Ok(None);
    }
    let trigger = authority
        .message_id()
        .ok_or_else(|| invalid("context trigger missing".into()))?;
    let channel = authority
        .input()
        .channel_id
        .ok_or_else(|| invalid("context channel missing".into()))?;
    let row = sqlx::query(
        "SELECT b.community_id, ev.created_at, ev.received_at,
                encode(tm.parent_event_id,'hex') AS parent_id,
                encode(tm.root_event_id,'hex') AS thread_id, rev.manifest
           FROM office_company_bindings b
           JOIN office_inbox i ON i.company_id=b.company_id AND i.event_id=$2
           JOIN events ev ON ev.community_id=b.community_id AND ev.id=i.event_id AND ev.created_at=i.event_created_at
           LEFT JOIN thread_metadata tm ON tm.community_id=ev.community_id AND tm.event_id=ev.id AND tm.event_created_at=ev.created_at
           JOIN employee_revisions rev ON rev.company_id=b.company_id AND rev.employee_id=$4 AND rev.id=$5
          WHERE b.company_id=$1 AND ev.channel_id=$3 AND ev.kind IN(9,40002) AND ev.deleted_at IS NULL",
    )
    .bind(scope.company_id()).bind(trigger.as_bytes().as_slice()).bind(channel)
    .bind(authority.employee_id().as_str()).bind(authority.employee_revision_id())
    .fetch_one(&mut *connection).await?;
    let community: Uuid = row.try_get("community_id")?;
    let parent: Option<String> = row.try_get("parent_id")?;
    let thread: Option<String> = if parent.is_some() {
        row.try_get("thread_id")?
    } else {
        None
    };
    if parent.is_some() && thread.is_none() {
        return Err(invalid("context canonical thread missing".into()));
    }
    let cutoff: DateTime<Utc> = row.try_get("received_at")?;
    let created: DateTime<Utc> = row.try_get("created_at")?;
    let manifest: Employee = serde_json::from_value(row.try_get("manifest")?)
        .map_err(|_| invalid("context employee manifest invalid".into()))?;
    let eligible = ortak_office::normalizer::channel_eligible_employees(
        &mut *connection,
        scope.company_id(),
        community,
        channel,
    )
    .await?;
    if !eligible.contains(authority.employee_id()) {
        return Err(invalid("context employee no longer eligible".into()));
    }
    let ids: Vec<&str> = eligible.iter().map(EmployeeId::as_str).collect();
    let roster = sqlx::query(
        "SELECT e.id,e.active_revision_id,r.manifest FROM employees e
         JOIN employee_revisions r ON r.company_id=e.company_id AND r.employee_id=e.id AND r.id=e.active_revision_id
         WHERE e.company_id=$1 AND e.status='active' AND e.id=ANY($2) AND e.id<>$3 ORDER BY e.id LIMIT $4",
    ).bind(scope.company_id()).bind(ids).bind(authority.employee_id().as_str())
        .bind(MAX_TEAMMATES as i64).fetch_all(&mut *connection).await?;
    let mut teammates = Vec::new();
    for row in roster {
        let value: Employee = serde_json::from_value(row.try_get("manifest")?)
            .map_err(|_| invalid("context roster manifest invalid".into()))?;
        teammates.push(employee(value, row.try_get("active_revision_id")?));
    }
    let rows = sqlx::query(include_str!("conversation_context/select.sql"))
        .bind(community)
        .bind(channel)
        .bind(created)
        .bind(cutoff)
        .bind(trigger.as_bytes().as_slice())
        .bind(&parent)
        .bind(&thread)
        .bind(scope.company_id())
        .bind((MAX_MESSAGES + 1) as i64)
        .fetch_all(&mut *connection)
        .await?;
    let mut context = ConversationContext {
        version: 1,
        snapshot_id: run,
        channel_id: channel,
        trigger_message_id: trigger.to_hex(),
        thread_root_message_id: thread.clone(),
        cutoff_received_at: cutoff,
        employee: employee(manifest, authority.employee_revision_id()),
        teammates,
        messages: Vec::new(),
        omitted_history: rows.len() > MAX_MESSAGES,
    };
    let mut total = 0;
    for row in rows.into_iter().take(MAX_MESSAGES) {
        let id: String = row.try_get("message_id")?;
        let text: String = row.try_get("content")?;
        let content = bounded(&text, MAX_MESSAGE_BYTES.min(MAX_HISTORY_BYTES - total));
        if content.trim().is_empty() {
            if parent.as_ref() == Some(&id) || thread.as_ref() == Some(&id) {
                return Err(invalid("context reply source has no usable text".into()));
            }
            context.omitted_history = true;
            continue;
        }
        total += content.len();
        let selection = if parent.as_ref() == Some(&id) {
            ContextSelection::ReplyParent
        } else if thread.as_ref() == Some(&id) {
            ContextSelection::ThreadRoot
        } else if thread.is_some() {
            ContextSelection::ThreadRecent
        } else {
            ContextSelection::ChannelRecent
        };
        context.messages.push(ContextMessage {
            message_id: id,
            created_at: row.try_get("created_at")?,
            author_public_key: row.try_get("author_public_key")?,
            author_employee_id: row
                .try_get::<Option<String>, _>("author_employee_id")?
                .map(EmployeeId::parse)
                .transpose()
                .map_err(|_| invalid("context author invalid".into()))?,
            author_name: bounded(&row.try_get::<String, _>("author_name")?, 200),
            parent_message_id: row.try_get("parent_id")?,
            thread_root_message_id: row.try_get("thread_id")?,
            truncated: content != text,
            content,
            source_content_hash: row.try_get("content_hash")?,
            selection,
        });
    }
    for required in [parent.as_ref(), thread.as_ref()].into_iter().flatten() {
        if !context.messages.iter().any(|m| &m.message_id == required) {
            return Err(invalid("context canonical reply source unavailable".into()));
        }
    }
    context
        .messages
        .sort_by(|a, b| (a.created_at, &a.message_id).cmp(&(b.created_at, &b.message_id)));
    // Preserve the selected transcript and receiver. Oversized visible rosters
    // are shortened, never allowed to overflow the encoded transport budget.
    while serde_json::to_vec(&context)
        .map_err(|_| invalid("context encoding failed".into()))?
        .len()
        > MAX_CONTEXT_BYTES
    {
        if context.teammates.pop().is_none() {
            return Err(invalid("context metadata exceeds budget".into()));
        }
    }
    if !context.valid_for(
        run,
        authority.employee_id(),
        authority.employee_revision_id(),
    ) {
        return Err(invalid("selected conversation context invalid".into()));
    }
    Ok(Some(context))
}
