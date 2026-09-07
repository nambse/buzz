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

mod employee;

// Intentionally project only these fields. Runtime/model configuration,
// credential references and prompts must never be serialized by this surface.
#[derive(Deserialize)]
struct Snapshot {
    version: u8,
    company_id: Uuid,
    routing_decision_id: Option<Uuid>,
    message_id: Option<String>,
    root_message_id: Option<String>,
    work_origin: Option<ortak_runtime::authority::WorkRunOrigin>,
    conversation: Option<ortak_runtime::memory_context::ReviewedConversationContext>,
    reviewed: Option<ortak_runtime::memory_context::ReviewedMemoryContext>,
    employee: Option<ortak_runtime::memory_context::ReviewedEmployeeContext>,
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
) -> Result<(
    Value,
    Option<ortak_runtime::memory_context::ReviewedEmployeeContext>,
)> {
    if bytes.is_empty()
        || bytes.len() > MAX_SNAPSHOT_BYTES
        || Sha256::digest(bytes).as_slice() != hash
    {
        return Err(ApiError::unavailable());
    }
    let snapshot: Snapshot = serde_json::from_slice(bytes).map_err(|_| ApiError::unavailable())?;
    let conversation = snapshot.conversation.is_some()
        || snapshot
            .employee
            .as_ref()
            .is_some_and(|value| value.conversation_origin.is_some());
    if (snapshot.version == 4) != snapshot.conversation.is_some()
        || (snapshot.version == 5) != snapshot.employee.is_some()
        || (snapshot.version == 5 && snapshot.reviewed.is_some())
        || conversation != row.try_get::<bool, _>("conversation")?
        || snapshot.employee.is_some() != row.try_get::<bool, _>("employee_memory")?
    {
        return Err(ApiError::unavailable());
    }
    let employee: &str = row.try_get("employee_id")?;
    let message: Option<String> = row.try_get("message_hex")?;
    let work: Option<Uuid> = row.try_get("work_item_id")?;
    let origin_valid = if let Some(item) = work {
        let pinned = snapshot.work_origin.as_ref();
        matches!(snapshot.version, 2..=5)
            && snapshot.routing_decision_id.is_none()
            && snapshot.message_id.is_none()
            && snapshot.root_message_id.is_none()
            && snapshot.event_kind == 0
            && snapshot.spec.context.conversation_ref.is_none()
            && snapshot.spec.context.reply_to_message_id.is_none()
            && snapshot.spec.context.work_item_id == Some(item)
            && row
                .try_get::<Option<Uuid>, _>("routing_decision_id")?
                .is_none()
            && message.is_none()
            && row.try_get::<Option<String>, _>("root_hex")?.is_none()
            && pinned.is_some_and(|origin| origin.run_id == run && origin.work_item_id == item)
            && pinned.map(|origin| origin.project_id)
                == row.try_get::<Option<Uuid>, _>("work_project_id")?
            && pinned.map(|origin| origin.execution_version)
                == row.try_get::<Option<i64>, _>("execution_version")?
            && pinned.map(|origin| origin.definition_hash.as_str())
                == row.try_get::<Option<&str>, _>("definition_hash")?
    } else {
        matches!(snapshot.version, 1 | 4 | 5)
            && snapshot.work_origin.is_none()
            && snapshot.routing_decision_id
                == row.try_get::<Option<Uuid>, _>("routing_decision_id")?
            && snapshot.routing_decision_id.is_some()
            && snapshot.message_id.as_deref() == message.as_deref()
            && snapshot.message_id.is_some()
            && snapshot.root_message_id == row.try_get::<Option<String>, _>("root_hex")?
            && snapshot.root_message_id.is_some()
            && Some(snapshot.event_kind) == row.try_get::<Option<i32>, _>("event_kind")?
            && snapshot.spec.context.conversation_ref
                == row
                    .try_get::<Option<Uuid>, _>("channel_id")?
                    .map(|id| id.to_string())
            && snapshot.spec.context.reply_to_message_id == message
            && snapshot.spec.context.work_item_id.is_none()
    };
    if !origin_valid
        || snapshot.company_id != company
        || snapshot.spec.run_id != run
        || snapshot.spec.employee_id.as_str() != employee
        || snapshot.recall.records.len() > MAX_CONTEXT_RECORDS
        || snapshot.spec.revision_id != row.try_get::<Uuid, _>("employee_revision_id")?
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
    Ok((
        json!({"status": "prepared", "records": records, "truncated": snapshot.recall.truncated}),
        snapshot.employee,
    ))
}

impl ApiState {
    pub(crate) async fn run_memory(&self, principal: &Principal, run_id: Uuid) -> Result<Value> {
        // The outer HTTP Office fence does not block project-grant changes.
        // Retain the sorted source/project locks through the entire projection,
        // including revoked metadata reads; this never renews run admission.
        let mut tx = self.control.pool().begin().await?;
        let current: bool = sqlx::query_scalar("SELECT ortak_lock_run_reviewed_memory($1,$2)")
            .bind(principal.scope.company_id())
            .bind(run_id)
            .fetch_one(&mut *tx)
            .await?;
        let row = sqlx::query(
            "SELECT r.employee_id,r.employee_revision_id,r.routing_decision_id,r.work_item_id,
                    EXISTS(SELECT 1 FROM run_reviewed_memory_uses u WHERE u.company_id=r.company_id
                        AND u.run_id=r.id AND u.conversation_audience_hash IS NOT NULL) AS conversation,
                    EXISTS(SELECT 1 FROM run_employee_reviewed_memory_uses u WHERE u.company_id=r.company_id
                        AND u.run_id=r.id) AS employee_memory,
                    x.project_id AS work_project_id,x.execution_version,encode(x.definition_hash,'hex') AS definition_hash,
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
             LEFT JOIN work_executions x ON x.company_id=r.company_id AND x.run_id=r.id
             LEFT JOIN office_inbox i ON i.company_id=r.company_id AND i.event_id=r.message_id
             LEFT JOIN routing_decisions d ON d.company_id=r.company_id AND d.id=r.routing_decision_id
             LEFT JOIN run_context_snapshots s ON s.company_id=r.company_id AND s.run_id=r.id
             LEFT JOIN runtime_memory_writes w ON w.company_id=r.company_id AND w.run_id=r.id
             WHERE r.company_id=$1 AND r.id=$2",
        ).bind(principal.scope.company_id()).bind(run_id).fetch_optional(&mut *tx).await?
            .ok_or_else(ApiError::not_found)?;
        let conversation: bool = row.try_get("conversation")?;
        let employee_memory: bool = row.try_get("employee_memory")?;
        let (mut recall, employee_context) = if let Some(bytes) =
            row.try_get::<Option<Vec<u8>>, _>("spec_bytes")?
        {
            let hash: Vec<u8> = row.try_get("spec_hash")?;
            let (mut value, employee) =
                recalled(&bytes, &hash, principal.scope.company_id(), run_id, &row)?;
            value["prepared_at"] =
                json!(row.try_get::<chrono::DateTime<chrono::Utc>, _>("prepared_at")?);
            (value, employee)
        } else {
            if employee_memory {
                return Err(ApiError::unavailable());
            }
            (
                json!({"status": "not_prepared", "records": [], "truncated": false, "prepared_at": null}),
                None,
            )
        };
        let mut current = current
            && employee_context
                .as_ref()
                .map(|context| {
                    context
                        .origin
                        .requester_public_key()
                        .map(|key| key == principal.public_key.to_hex())
                        .map_err(|_| ApiError::unavailable())
                })
                .transpose()?
                .unwrap_or(true);
        let mut write = if let Some(state) = row.try_get::<Option<String>, _>("state")? {
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
        let reviewed_rows=sqlx::query("SELECT u.*,f.content,f.audience_kind,
            a.audience_bytes,a.provenance_bytes,
            CASE WHEN f.audience_kind='conversation' THEN $3 AND
                ortak_conversation_runtime_eligible(u.company_id,u.run_id,u.fact_id,u.target_id,
                    u.conversation_authority_epoch,u.conversation_consumption_epoch)
            ELSE (NOT $4 OR $3) AND
                ortak_reviewed_runtime_eligible(u.company_id,u.fact_id,u.target_id,u.consumption_epoch)
            END AS current
            FROM run_reviewed_memory_uses u JOIN reviewed_memory_facts f ON f.company_id=u.company_id AND f.id=u.fact_id
            LEFT JOIN reviewed_memory_conversation_audiences a ON a.company_id=f.company_id AND a.fact_id=f.id
            WHERE u.company_id=$1 AND u.run_id=$2 ORDER BY u.ordinal LIMIT 8")
            .bind(principal.scope.company_id()).bind(run_id).bind(current).bind(conversation || employee_memory)
            .fetch_all(&mut *tx).await?;
        let mut reviewed = Vec::new();
        for row in &reviewed_rows {
            let current: bool = row.try_get("current")?;
            let mut record = json!({"fact_id":row.try_get::<Uuid,_>("fact_id")?,"approval_id":row.try_get::<Uuid,_>("approval_id")?,
                "approved_by":row.try_get::<String,_>("approved_by")?,"expires_at":row.try_get::<chrono::DateTime<chrono::Utc>,_>("expires_at")?,
                "current":current,"content":if current {text(&row.try_get::<String,_>("content")?)} else {Value::Null}});
            if conversation || employee_memory {
                record["audience_kind"] = json!(row.try_get::<String, _>("audience_kind")?);
                record["audience"] = if current {
                    row.try_get::<Option<Vec<u8>>, _>("audience_bytes")?
                        .map(|bytes| serde_json::from_slice::<Value>(&bytes))
                        .transpose()
                        .map_err(|_| ApiError::unavailable())?
                        .unwrap_or(Value::Null)
                } else {
                    Value::Null
                };
            }
            reviewed.push(record);
        }
        if let Some(context) = &employee_context {
            reviewed = employee::project(
                &mut tx,
                employee::Input {
                    principal,
                    community: self.config.community_id,
                    run: run_id,
                    context,
                    legacy_rows: &reviewed_rows,
                    legacy_views: reviewed,
                    current,
                    run_row: &row,
                },
            )
            .await?;
            // Locks fence mutations, but clock-only expiry still needs a final
            // current-use check after all bounded projection reads.
            current &=
                sqlx::query_scalar::<_, bool>("SELECT ortak_run_reviewed_memory_current($1,$2)")
                    .bind(principal.scope.company_id())
                    .bind(run_id)
                    .fetch_one(&mut *tx)
                    .await?;
            if !current {
                for record in &mut reviewed {
                    record["current"] = json!(false);
                    record["content"] = Value::Null;
                    record["audience"] = Value::Null;
                }
            }
        }
        if (conversation || employee_memory) && !current {
            // A provider may have incorporated the retired fact into scratch or
            // output. Keep only receipt/retry attribution, never derived text.
            recall["records"] = json!([]);
            recall["withheld"] = json!(true);
            if !write.is_null() {
                write["content"] = json!({"text":"","redacted":true,"truncated":false});
                write["withheld"] = json!(true);
            }
        }
        Ok(
            json!({"scope": if employee_memory {"run_scratch_and_reviewed_employee"}
                else if conversation {"run_scratch_and_reviewed_conversation"}
                else if reviewed.is_empty(){"run_scratch"}else{"run_scratch_and_reviewed_project"},
            "run_id": run_id, "recall": recall, "reviewed":reviewed, "write": write}),
        )
    }
}
