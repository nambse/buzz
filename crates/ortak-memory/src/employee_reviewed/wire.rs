use super::*;

pub(super) fn timestamps(value: &serde_json::Value, keys: &[&str]) -> Result<(), MemoryError> {
    use chrono::{DateTime, Datelike, SecondsFormat, Utc};
    for key in keys {
        if let Some(text) = value.get(*key).and_then(serde_json::Value::as_str) {
            let parsed = DateTime::parse_from_rfc3339(text)
                .map_err(|_| rejected("invalid employee receipt time"))?
                .with_timezone(&Utc);
            if !(1970..=9999).contains(&parsed.year())
                || parsed.to_rfc3339_opts(SecondsFormat::Micros, true) != text
            {
                return Err(rejected("noncanonical employee receipt time"));
            }
        }
    }
    Ok(())
}
use serde_json::Value;
use sha2::{Digest, Sha256};

pub(super) fn hash(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}
pub(super) fn is_hash(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
pub(super) fn text(value: &str) -> bool {
    !value.trim().is_empty()
        && value.len() <= 4096
        && !value
            .chars()
            .any(|c| c.is_control() && !matches!(c, '\n' | '\t'))
        && ortak_control::run_event::RedactionPolicy::new().redact(value) == value
}
pub(super) fn common(namespace: &ReviewedEmployeeNamespace) -> Value {
    let r = &namespace.original;
    json!({"company_id":r.company_id,"employee_id":r.employee_id,"deployment_id":r.deployment_id,
        "binding":r.binding,"ownership":{"request_hash":r.request_hash,"native_ids":r.native_ids}})
}
pub(super) fn path(namespace: &ReviewedEmployeeNamespace) -> String {
    format!(
        "/v3/ortak/workspaces/{}/reviewed-employees/{}",
        namespace.original.binding.workspace, namespace.original.employee_id
    )
}
pub(super) fn extend(mut value: Value, extra: Value) -> Result<Value, MemoryError> {
    let target = value
        .as_object_mut()
        .ok_or_else(|| invalid("invalid employee request"))?;
    target.extend(
        extra
            .as_object()
            .ok_or_else(|| invalid("invalid employee fields"))?
            .clone(),
    );
    if serde_json::to_vec(&value)
        .map_err(|_| invalid("invalid employee wire"))?
        .len()
        > 32768
    {
        return Err(invalid("employee request exceeds byte limit"));
    }
    Ok(value)
}
pub(super) fn bounded_response(value: &Value) -> Result<(), MemoryError> {
    if serde_json::to_vec(value)
        .map_err(|_| rejected("invalid employee response"))?
        .len()
        > 65536
    {
        return Err(rejected("employee response exceeds byte limit"));
    }
    Ok(())
}
pub(super) fn diagnostic(request: &EmployeeNamespaceDiagnostic) -> Result<(), MemoryError> {
    if request.operation_id.is_nil()
        || request.employee_revision_id.is_nil()
        || request.employee_lifecycle_epoch < 0
        || !is_hash(&request.challenge)
    {
        return Err(invalid("invalid explicit namespace diagnostic"));
    }
    Ok(())
}
pub(super) fn diagnostic_hash(
    namespace: &ReviewedEmployeeNamespace,
    request: &EmployeeNamespaceDiagnostic,
    withdraw: bool,
) -> Result<String, MemoryError> {
    let mut value = json!({"format":if withdraw {"ortak-reviewed-employee-diagnostic-withdraw/1"}else{"ortak-reviewed-employee-diagnostic/1"},
        "operation_id":request.operation_id,"namespace_hash":namespace.namespace_hash,"binding_hash":namespace.binding_hash,
        "employee_revision_id":request.employee_revision_id,"employee_lifecycle_epoch":request.employee_lifecycle_epoch});
    if withdraw {
        value["challenge_hash"] = json!(hash(request.challenge.as_bytes()));
    } else {
        value["challenge"] = json!(request.challenge);
    }
    crate::wire::fingerprint(&value)
}
pub(super) fn commitment(value: &ReviewedEmployeeCommitment) -> Result<(), MemoryError> {
    if value.target_id.is_nil()
        || value.fact_id.is_nil()
        || value.destination_channel_id.is_nil()
        || ![&value.content_hash, &value.source_hash, &value.sharing_hash]
            .iter()
            .all(|v| is_hash(v))
    {
        return Err(invalid("invalid employee export commitment"));
    }
    Ok(())
}

/// Exact counterpart of candidate SQL's typed remote request commitment.
pub fn employee_reviewed_request_hash(
    namespace_hash: &str,
    binding_hash: &str,
    company: Uuid,
    employee: &EmployeeId,
    value: &ReviewedEmployeeCommitment,
    withdraw: bool,
) -> Result<String, MemoryError> {
    commitment(value)?;
    if company.is_nil() || !is_hash(namespace_hash) || !is_hash(binding_hash) {
        return Err(invalid("invalid employee namespace commitment"));
    }
    crate::wire::fingerprint(
        &json!({"format":"ortak-reviewed-employee-remote-request/1","action":if withdraw {"withdraw"}else{"publish"},
        "company_id":company,"employee_id":employee,"fact_id":value.fact_id,"target_id":value.target_id,
        "namespace_hash":namespace_hash,"binding_hash":binding_hash,"content_hash":value.content_hash,"source_hash":value.source_hash,"sharing_hash":value.sharing_hash}),
    )
}
