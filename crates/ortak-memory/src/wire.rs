use std::collections::{BTreeMap, BTreeSet};

use chrono::{DateTime, SecondsFormat, Utc};
use ortak_control::memory::{
    MemoryProvenance, MemoryRecall, MemoryRecord, MemoryScope, MemoryWriteRequest,
};
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::{config, invalid, rejected, HonchoEmployeeBinding, MemoryError, PROTOCOL};

pub(crate) fn canonical(value: &Value) -> Result<Vec<u8>, MemoryError> {
    fn sorted(value: &Value) -> Value {
        match value {
            Value::Object(map) => {
                let values: BTreeMap<_, _> =
                    map.iter().map(|(k, v)| (k.clone(), sorted(v))).collect();
                Value::Object(values.into_iter().collect())
            }
            Value::Array(values) => Value::Array(values.iter().map(sorted).collect()),
            other => other.clone(),
        }
    }
    serde_json::to_vec(&sorted(value)).map_err(|_| invalid("invalid memory canonical encoding"))
}

pub(crate) fn fingerprint(value: &Value) -> Result<String, MemoryError> {
    Ok(hex::encode(Sha256::digest(canonical(value)?)))
}

pub(crate) fn scope_value(scope: &MemoryScope) -> Result<Value, MemoryError> {
    serde_json::to_value(scope).map_err(|_| invalid("invalid memory scope"))
}

pub(crate) fn context(
    company: Uuid,
    allowed: &HonchoEmployeeBinding,
    scope: &MemoryScope,
) -> Result<Value, MemoryError> {
    Ok(
        json!({"protocol":PROTOCOL,"company_id":company,"employee_id":allowed.employee_id,"scope":scope_value(scope)?}),
    )
}

pub(crate) fn session(
    company: Uuid,
    allowed: &HonchoEmployeeBinding,
    scope: &MemoryScope,
) -> Result<String, MemoryError> {
    Ok(format!(
        "ortak_{}",
        fingerprint(&context(company, allowed, scope)?)?
    ))
}

pub(crate) fn check_scope(
    allowed: &HonchoEmployeeBinding,
    scope: &MemoryScope,
) -> Result<(), MemoryError> {
    let valid = match scope {
        MemoryScope::CompanyTruth => allowed.allow_company_truth,
        MemoryScope::ProjectContext { project_id } => allowed.allowed_projects.contains(project_id),
        MemoryScope::RunScratch { run_id } => !run_id.is_nil(),
        MemoryScope::EmployeeExperience | MemoryScope::Relationship => true,
    };
    if valid {
        Ok(())
    } else {
        Err(invalid("memory scope is outside the authorized binding"))
    }
}

pub(crate) fn provenance(value: &MemoryProvenance) -> Result<Value, MemoryError> {
    if value.source.trim().is_empty()
        || value.source.len() > 128
        || value.source.chars().any(char::is_control)
    {
        return Err(invalid("memory provenance source is invalid"));
    }
    // Match Pydantic's normalized UTC datetime spelling used by receipt hashing.
    let micros = value.recorded_at.timestamp_subsec_micros();
    let recorded_at = DateTime::<Utc>::from_timestamp(value.recorded_at.timestamp(), micros * 1000)
        .ok_or_else(|| invalid("memory provenance timestamp is invalid"))?;
    let format = if micros == 0 {
        SecondsFormat::Secs
    } else {
        SecondsFormat::Micros
    };
    let mut data = json!({"employee_id":value.employee_id,"source":value.source,"recorded_at":recorded_at.to_rfc3339_opts(format,true)});
    if let Some(id) = value.run_id {
        if id.is_nil() {
            return Err(invalid("memory provenance run is invalid"));
        }
        data["run_id"] = json!(id);
    }
    Ok(data)
}

pub(crate) fn check_record(
    record: &MemoryRecord,
    allowed: &HonchoEmployeeBinding,
    scope: &MemoryScope,
) -> Result<(), MemoryError> {
    if record.scope != *scope
        || record.provenance.employee_id != allowed.employee_id
        || !config::name(&record.record_ref)
        || record.content.trim().is_empty()
        || record.content.len() > 16 * 1024
        || record.content.contains('\0')
        || matches!(scope, MemoryScope::RunScratch {run_id} if record.provenance.run_id != Some(*run_id))
    {
        return Err(rejected(
            "memory record violates scope, provenance or content bounds",
        ));
    }
    provenance(&record.provenance).map_err(|_| rejected("memory record provenance is invalid"))?;
    Ok(())
}

pub(crate) fn write_body(
    company: Uuid,
    allowed: &HonchoEmployeeBinding,
    request: &MemoryWriteRequest,
) -> Result<Value, MemoryError> {
    request.validate()?;
    check_scope(allowed, &request.scope)?;
    if !config::key(&request.idempotency_key) {
        return Err(invalid("memory write key is invalid"));
    }
    let mut facts = Vec::with_capacity(request.facts.len());
    for fact in &request.facts {
        if fact.content.contains('\0')
            || fact.provenance.employee_id != allowed.employee_id
            || matches!(&request.scope, MemoryScope::RunScratch {run_id} if fact.provenance.run_id != Some(*run_id))
        {
            return Err(invalid(
                "memory fact provenance does not match authorized employee or run",
            ));
        }
        facts.push(json!({"content":fact.content,"provenance":provenance(&fact.provenance)?}));
    }
    Ok(
        json!({"idempotency_key":request.idempotency_key,"company_id":company,"employee_id":allowed.employee_id,"scope":scope_value(&request.scope)?,"facts":facts}),
    )
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WriteResponse {
    protocol: String,
    workspace_id: String,
    session_id: String,
    request_hash: String,
    record_refs: Vec<String>,
    records: Vec<WrittenRecord>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WrittenRecord {
    record_ref: String,
    content: String,
    scope: MemoryScope,
    provenance: MemoryProvenance,
    metadata: Value,
}

pub(crate) fn validate_write(
    company: Uuid,
    allowed: &HonchoEmployeeBinding,
    request: &MemoryWriteRequest,
    session: &str,
    body: &Value,
    response: Value,
) -> Result<(String, Vec<MemoryRecord>), MemoryError> {
    let mut hashed = body.clone();
    hashed["workspace_id"] = json!(allowed.binding.workspace);
    hashed["session_id"] = json!(session);
    let expected_hash = fingerprint(&hashed)?;
    let response: WriteResponse =
        serde_json::from_value(response).map_err(|_| rejected("invalid memory write receipt"))?;
    if response.protocol != PROTOCOL
        || response.workspace_id != allowed.binding.workspace
        || response.session_id != session
        || response.request_hash != expected_hash
        || response.records.len() != request.facts.len()
        || response.record_refs.len() != request.facts.len()
    {
        return Err(rejected("memory write receipt does not match request"));
    }
    let mut unique = BTreeSet::new();
    let mut records = Vec::with_capacity(response.records.len());
    for (index, written) in response.records.into_iter().enumerate() {
        let mut envelope = context(company, allowed, &request.scope)?;
        envelope["write_key"] = json!(request.idempotency_key);
        envelope["request_hash"] = json!(expected_hash);
        envelope["fact_index"] = json!(index);
        envelope["provenance"] = body["facts"][index]["provenance"].clone();
        let record = MemoryRecord {
            record_ref: written.record_ref,
            content: written.content,
            scope: written.scope,
            provenance: written.provenance,
        };
        check_record(&record, allowed, &request.scope)?;
        if !unique.insert(record.record_ref.clone())
            || response.record_refs[index] != record.record_ref
            || record.content != request.facts[index].content
            || provenance(&record.provenance)? != body["facts"][index]["provenance"]
            || written.metadata != json!({"ortak":envelope})
        {
            return Err(rejected(
                "memory write receipt content or provenance differs",
            ));
        }
        records.push(record);
    }
    Ok((
        format!(
            "honcho:{}:{}:{}",
            allowed.binding.workspace, session, expected_hash
        ),
        records,
    ))
}

pub(crate) fn validate_recall(
    allowed: &HonchoEmployeeBinding,
    scope: &MemoryScope,
    max_records: usize,
    max_bytes: usize,
    value: Value,
) -> Result<MemoryRecall, MemoryError> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct RecallResponse {
        records: Vec<MemoryRecord>,
        truncated: bool,
    }
    let response: RecallResponse =
        serde_json::from_value(value).map_err(|_| rejected("invalid memory recall response"))?;
    if response.records.len() > max_records {
        return Err(rejected("memory recall exceeds record budget"));
    }
    let mut total = 0usize;
    let mut ids = BTreeSet::new();
    for record in &response.records {
        check_record(record, allowed, scope)?;
        total = total.saturating_add(record.content.len());
        if total > max_bytes || !ids.insert(&record.record_ref) {
            return Err(rejected("memory recall exceeds budget or repeats records"));
        }
    }
    Ok(MemoryRecall {
        records: response.records,
        truncated: response.truncated,
    })
}
