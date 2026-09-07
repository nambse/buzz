use axum::Json;
use ortak_control::run_event::RedactionPolicy;
use ortak_domain::{WorkActor, WorkEvent};
use ortak_work::{ApiProject, ProjectRole, WorkItemAggregate, WorkSummary};
use serde_json::{json, Value};

use crate::{
    auth::Principal,
    error::{ApiError, Result},
};

// Only explicitly selected product fields cross this boundary. Aggregate
// attachments, dependency targets, runtime/decision IDs and raw event payloads
// require the later execution audience contract and are not API DTO fields.
fn text(value: &str) -> String {
    let cleaned: String = value
        .chars()
        .filter(|ch| !ch.is_control() || matches!(ch, '\n' | '\t'))
        .collect();
    RedactionPolicy::new().redact(&cleaned)
}

fn actor(value: &WorkActor, principal: &Principal) -> Value {
    match value {
        WorkActor::Human(id) if nostr::PublicKey::from_hex(id).is_ok() => {
            json!({"type": "human", "public_key": id})
        }
        WorkActor::Employee(id) if principal.grant.employee_ids.contains(id) => {
            json!({"type": "employee", "employee_id": id})
        }
        _ => json!({"type": value.type_str()}),
    }
}

pub(super) fn project(value: &ApiProject, detail: bool, principal: &Principal) -> Value {
    let p = &value.record.project;
    let mut result = json!({
        "id": p.id, "slug": p.slug, "name": text(&p.name),
        "status": p.status, "version": p.version,
        "channel_id": value.channel_id, "role": value.role,
        "can_contribute": principal.grant.role == crate::Role::Operator
            && matches!(value.role, ProjectRole::Owner | ProjectRole::Contributor),
        "can_review": principal.grant.role == crate::Role::Operator
            && matches!(value.role, ProjectRole::Owner | ProjectRole::Reviewer),
        "created_at": value.record.created_at, "updated_at": value.record.updated_at,
    });
    if detail {
        result["description"] = json!(text(&p.description));
    }
    result
}

pub(super) fn channel(id: uuid::Uuid, name: &str) -> Value {
    json!({"id": id, "name": text(name)})
}

pub(super) fn summary(value: &WorkSummary) -> Value {
    json!({
        "id": value.id, "project_id": value.project_id, "title": text(&value.title),
        "priority": value.priority, "state": value.state, "version": value.version,
        "source_message_id": value.source_message_id,
        "created_at": value.created_at, "updated_at": value.updated_at,
    })
}

pub(super) fn item(value: &WorkItemAggregate, principal: &Principal) -> Value {
    let item = &value.item;
    let criteria: Vec<_> = item
        .criteria
        .iter()
        .map(|c| {
            json!({
                "id": c.id, "position": c.position, "text": text(&c.text),
                "status": c.status,
                "satisfied_by": c.satisfied_by.as_ref().map(|a| actor(a, principal)),
            })
        })
        .collect();
    let approvals: Vec<_> = item
        .approvals
        .iter()
        .map(|a| {
            json!({
                "id": a.id, "gate": a.gate, "required": a.required, "status": a.status,
                "resolved_by": a.resolved_by.as_ref().map(|a| actor(a, principal)),
                "reason": a.reason.as_deref().map(text),
            })
        })
        .collect();
    let assignments: Vec<_> = item
        .assignments
        .iter()
        .filter(|a| principal.grant.employee_ids.contains(&a.employee_id))
        .map(|a| json!({"employee_id": a.employee_id, "role": a.role, "status": a.status}))
        .collect();
    let history: Vec<_> = value
        .history
        .iter()
        .filter(|entry| {
            !matches!(
                entry.event,
                WorkEvent::Attached { .. }
                    | WorkEvent::DependencyAdded { .. }
                    | WorkEvent::DependencyRemoved { .. }
                    | WorkEvent::ChildCreated { .. }
            )
        })
        .map(|entry| {
            let mut result = json!({
                "sequence": entry.sequence, "version": entry.version,
                "event_type": entry.event.event_type(),
                "actor": actor(&entry.actor, principal), "recorded_at": entry.recorded_at,
            });
            if let Some((from, to)) = entry.event.state_change() {
                result["from"] = json!(from);
                result["to"] = json!(to);
            }
            result
        })
        .collect();
    json!({
        "id": item.id, "project_id": item.project_id, "title": text(&item.title),
        "description": text(&item.description), "priority": item.priority,
        "state": item.state, "version": item.version,
        "source_message_id": item.source_message_id,
        "criteria": criteria, "approvals": approvals, "assignments": assignments,
        "created_by": actor(&value.created_by, principal),
        "created_at": value.created_at, "updated_at": value.updated_at,
        "completed_at": value.completed_at, "cancelled_at": value.cancelled_at,
        "history_omitted": history.len() != value.history.len(),
        "history_truncated": value.history_truncated, "history": history,
        "execution_available": principal.grant.role == crate::Role::Operator
            && matches!(item.state,ortak_domain::WorkState::Ready|ortak_domain::WorkState::InProgress)
            && item.definition_editable() && item.assignments.iter().any(|a|
                principal.grant.employee_ids.contains(&a.employee_id) && a.status==ortak_domain::AssignmentStatus::Active
                && matches!(a.role,ortak_domain::AssignmentRole::Owner|ortak_domain::AssignmentRole::Contributor)),
    })
}

pub(super) fn bounded(value: Value) -> Result<Json<Value>> {
    let bytes = serde_json::to_vec(&value).map_err(|_| ApiError::unavailable())?;
    if bytes.len() > 262_144 {
        return Err(ApiError::unavailable());
    }
    Ok(Json(value))
}
