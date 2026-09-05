//! Read-only projection of admitted context and durable write receipts.
//! Called only after the canonical run audience gate, under the HTTP Office fence.

use std::collections::BTreeSet;

use ortak_control::{
    memory::{MemoryRecall, MemoryScope, MemoryWriteReceipt},
    run_event::{strip_control_characters, RedactionPolicy},
};
use ortak_domain::EmployeeId;
use ortak_runtime::memory_context::{MAX_CONTEXT_BYTES, MAX_CONTEXT_RECORDS, MAX_SNAPSHOT_BYTES};
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use sqlx::Row;
use uuid::Uuid;

use crate::{
    auth::Principal,
    error::{ApiError, Result},
    routes::ApiState,
};

// Intentionally project only these fields. Runtime/model configuration,
// credential references and prompts must never be serialized by this surface.
#[derive(Deserialize)]
struct Snapshot {
    version: u8,
    company_id: Uuid,
    routing_decision_id: Uuid,
    message_id: String,
    root_message_id: String,
    event_kind: i32,
    recall: MemoryRecall,
    spec: SpecPin,
}
#[derive(Deserialize)]
struct SpecPin {
    run_id: Uuid,
    employee_id: EmployeeId,
    revision_id: Uuid,
    context: ContextPin,
}
#[derive(Deserialize)]
struct ContextPin {
    conversation_ref: Option<String>,
    reply_to_message_id: Option<String>,
    work_item_id: Option<Uuid>,
}

fn text(value: &str) -> Value {
    let clean = strip_control_characters(value);
    let text = RedactionPolicy::new().redact(&clean);
    let redacted = text != value || text.contains("[redacted]");
    json!({"text": text, "redacted": redacted, "truncated": false})
}

fn recalled(
    bytes: &[u8],
    hash: &[u8],
    company: Uuid,
    run: Uuid,
    row: &sqlx::postgres::PgRow,
) -> Result<Value> {
    if bytes.is_empty()
        || bytes.len() > MAX_SNAPSHOT_BYTES
        || Sha256::digest(bytes).as_slice() != hash
    {
        return Err(ApiError::unavailable());
    }
    let snapshot: Snapshot = serde_json::from_slice(bytes).map_err(|_| ApiError::unavailable())?;
    let employee: &str = row.try_get("employee_id")?;
    let message: Option<String> = row.try_get("message_hex")?;
    if snapshot.version != 1
        || snapshot.company_id != company
        || snapshot.spec.run_id != run
        || snapshot.spec.employee_id.as_str() != employee
        || snapshot.recall.records.len() > MAX_CONTEXT_RECORDS
        || Some(snapshot.routing_decision_id)
            != row.try_get::<Option<Uuid>, _>("routing_decision_id")?
        || Some(snapshot.message_id.as_str()) != message.as_deref()
        || Some(snapshot.root_message_id) != row.try_get::<Option<String>, _>("root_hex")?
        || Some(snapshot.event_kind) != row.try_get::<Option<i32>, _>("event_kind")?
        || snapshot.spec.revision_id != row.try_get::<Uuid, _>("employee_revision_id")?
        || snapshot.spec.context.conversation_ref
            != row
                .try_get::<Option<Uuid>, _>("channel_id")?
                .map(|id| id.to_string())
        || snapshot.spec.context.reply_to_message_id != message
        || snapshot.spec.context.work_item_id.is_some()
    {
        return Err(ApiError::unavailable());
    }
    let mut references = BTreeSet::new();
    let mut total = 0usize;
    let mut records = Vec::new();
    for record in snapshot.recall.records {
        total += record.content.len();
        if record.scope != (MemoryScope::RunScratch { run_id: run })
            || record.provenance.run_id != Some(run)
            || record.provenance.employee_id.as_str() != employee
            || record.record_ref.is_empty()
            || record.record_ref.len() > 256
            || !references.insert(record.record_ref.clone())
            || record.provenance.source.is_empty()
            || record.provenance.source.len() > 128
            || record.content.trim().is_empty()
            || record.content.len() > 4096
            || total > MAX_CONTEXT_BYTES
        {
            return Err(ApiError::unavailable());
        }
        records.push(json!({
            "record_ref": text(&record.record_ref)["text"],
            "content": text(&record.content),
            "source": text(&record.provenance.source)["text"],
            "recorded_at": record.provenance.recorded_at,
        }));
    }
    Ok(json!({"status": "prepared", "records": records, "truncated": snapshot.recall.truncated}))
}

impl ApiState {
    pub(crate) async fn run_memory(&self, principal: &Principal, run_id: Uuid) -> Result<Value> {
        let row = sqlx::query(
            "SELECT r.employee_id,r.employee_revision_id,r.routing_decision_id,
                    encode(r.message_id,'hex') AS message_hex,encode(r.root_message_id,'hex') AS root_hex,
                    i.channel_id,i.event_kind,s.spec_bytes,s.spec_hash,s.created_at AS prepared_at,
                    w.state,w.last_error_code,w.attempt_count,w.next_attempt_at,
                    w.content,w.recorded_at,w.signed_event_id,w.receipt,w.acknowledged_at,
                    (w.employee_id=r.employee_id AND w.employee_revision_id=r.employee_revision_id
                     AND w.channel_id=i.channel_id AND w.source_facts=jsonb_build_object(
                         'employee_id',r.employee_id,'employee_revision_id',r.employee_revision_id,
                         'routing_decision_id',r.routing_decision_id,'message_id',encode(r.message_id,'hex'),
                         'root_message_id',encode(r.root_message_id,'hex'),'delivery_intent',r.delivery_intent,
                         'office_input_hash',encode(d.office_input_hash,'hex'))) AS write_pinned
             FROM runs r
             LEFT JOIN office_inbox i ON i.company_id=r.company_id AND i.event_id=r.message_id
             LEFT JOIN routing_decisions d ON d.company_id=r.company_id AND d.id=r.routing_decision_id
             LEFT JOIN run_context_snapshots s ON s.company_id=r.company_id AND s.run_id=r.id
             LEFT JOIN runtime_memory_writes w ON w.company_id=r.company_id AND w.run_id=r.id
             WHERE r.company_id=$1 AND r.id=$2",
        ).bind(principal.scope.company_id()).bind(run_id).fetch_optional(self.control.pool()).await?
            .ok_or_else(ApiError::not_found)?;
        let recall = if let Some(bytes) = row.try_get::<Option<Vec<u8>>, _>("spec_bytes")? {
            let hash: Vec<u8> = row.try_get("spec_hash")?;
            let mut value = recalled(&bytes, &hash, principal.scope.company_id(), run_id, &row)?;
            value["prepared_at"] =
                json!(row.try_get::<chrono::DateTime<chrono::Utc>, _>("prepared_at")?);
            value
        } else {
            json!({"status": "not_prepared", "records": [], "truncated": false, "prepared_at": null})
        };
        let write = if let Some(state) = row.try_get::<Option<String>, _>("state")? {
            if row.try_get::<Option<bool>, _>("write_pinned")? != Some(true) {
                return Err(ApiError::unavailable());
            }
            if !matches!(state.as_str(), "pending" | "acknowledged" | "failed") {
                return Err(ApiError::unavailable());
            }
            let receipt = row
                .try_get::<Option<Value>, _>("receipt")?
                .map(serde_json::from_value::<MemoryWriteReceipt>)
                .transpose()
                .map_err(|_| ApiError::unavailable())?;
            if (state == "acknowledged") != receipt.is_some() {
                return Err(ApiError::unavailable());
            }
            let receipt = receipt.map(|receipt| {
                json!({
                    "reference": text(&receipt.receipt_ref)["text"], "written": receipt.written,
                })
            });
            let signed_id: Vec<u8> = row.try_get("signed_event_id")?;
            let source = signed_id
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>();
            json!({
                "status": state, "error_code": row.try_get::<Option<String>, _>("last_error_code")?,
                "attempts": row.try_get::<i32, _>("attempt_count")?,
                "next_attempt_at": if state == "pending" { row.try_get::<Option<chrono::DateTime<chrono::Utc>>, _>("next_attempt_at")? } else { None },
                "content": text(&row.try_get::<String, _>("content")?),
                "source": format!("office:{source}"),
                "recorded_at": row.try_get::<chrono::DateTime<chrono::Utc>, _>("recorded_at")?,
                "receipt": receipt, "acknowledged_at": row.try_get::<Option<chrono::DateTime<chrono::Utc>>, _>("acknowledged_at")?,
            })
        } else {
            Value::Null
        };
        Ok(json!({"scope": "run_scratch", "run_id": run_id, "recall": recall, "write": write}))
    }
}
