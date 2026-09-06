use super::*;
use chrono::{DateTime, SecondsFormat, Utc};
use sha2::{Digest, Sha256};

pub(super) fn digest(text: &str) -> String {
    hex::encode(Sha256::digest(text.as_bytes()))
}
pub(super) fn hash(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn timestamp(value: DateTime<Utc>) -> Result<String, MemoryError> {
    let micros = value.timestamp_subsec_micros();
    let value = DateTime::<Utc>::from_timestamp(value.timestamp(), micros * 1000)
        .ok_or_else(|| invalid("invalid reviewed expiry"))?;
    Ok(value.to_rfc3339_opts(
        if micros == 0 {
            SecondsFormat::Secs
        } else {
            SecondsFormat::Micros
        },
        true,
    ))
}

pub(super) fn publication(
    company: Uuid,
    audience: &ReviewedProjectScope,
    value: &ReviewedProjectPublication,
) -> Result<Value, MemoryError> {
    if value.record_id.is_nil()
        || value.approval_id.is_nil()
        || !config::key(&value.idempotency_key)
        || value.content.trim().is_empty()
        || value.content.len() > 4096
        || value
            .content
            .chars()
            .any(|c| c.is_control() && c != '\n' && c != '\t')
        || !hash(&value.source_hash)
        || !hash(&value.approved_by)
        || ortak_control::run_event::RedactionPolicy::new().redact(&value.content) != value.content
    {
        return Err(invalid("invalid reviewed publication"));
    }
    // No local future-time rejection: a delayed retry can legitimately be expired.
    Ok(
        json!({"company_id":company,"employee_id":audience.employee_id,
        "idempotency_key":value.idempotency_key,"content":value.content,"content_hash":digest(&value.content),
        "source_hash":value.source_hash,"approval_id":value.approval_id,"approved_by":value.approved_by,
        "expires_at":timestamp(value.expires_at)?}),
    )
}

pub(super) fn record(
    value: &ReviewedProjectRecord,
    company: Uuid,
    audience: &ReviewedProjectScope,
    binding_hash: &str,
    include_text: bool,
) -> Result<(), MemoryError> {
    let has_publication = value.provenance.is_some();
    if value.protocol != PROTOCOL
        || value.record_family != FAMILY
        || value.company_id != company
        || value.employee_id != audience.employee_id
        || value.project_id != audience.project_id
        || value.workspace_id != audience.binding.workspace
        || value.record_id.is_nil()
        || value.binding_hash != binding_hash
        || has_publication != value.content_hash.is_some()
        || has_publication != value.expires_at.is_some()
        || value.erased_from_reviewed_store != value.tombstone_at.is_some()
        || (value.status == ReviewedProjectStatus::Withdrawn && !value.erased_from_reviewed_store)
        || (!has_publication && value.status != ReviewedProjectStatus::Withdrawn)
        || value.content_hash.as_ref().is_some_and(|h| !hash(h))
    {
        return Err(rejected(
            "reviewed record identity or state is inconsistent",
        ));
    }
    if let Some(provenance) = &value.provenance {
        if provenance.approval_id.is_nil()
            || !hash(&provenance.approved_by)
            || !hash(&provenance.source_hash)
        {
            return Err(rejected("reviewed record provenance is invalid"));
        }
    }
    if value.status == ReviewedProjectStatus::Active {
        if value.erased_from_reviewed_store
            || value.expires_at.is_none_or(|expiry| expiry <= Utc::now())
            || (include_text != value.content.is_some())
        {
            return Err(rejected("reviewed active record is unavailable"));
        }
    } else if value.content.is_some() {
        return Err(rejected(
            "withdrawn or expired reviewed content was returned",
        ));
    }
    if let Some(content) = &value.content {
        if content.trim().is_empty()
            || content.len() > 4096
            || content
                .chars()
                .any(|c| c.is_control() && c != '\n' && c != '\t')
            || value.content_hash.as_ref() != Some(&digest(content))
            || ortak_control::run_event::RedactionPolicy::new().redact(content) != *content
        {
            return Err(rejected(
                "reviewed content differs from its approved digest or policy",
            ));
        }
    }
    Ok(())
}

pub(super) fn receipt(
    mut response: Value,
    body: &Value,
    action: &str,
    company: Uuid,
    audience: &ReviewedProjectScope,
    id: Uuid,
    binding_hash: &str,
    created: bool,
) -> Result<ReviewedProjectReceipt, MemoryError> {
    let response_hash = response
        .as_object_mut()
        .and_then(|r| r.remove("request_hash"))
        .and_then(|h| h.as_str().map(str::to_owned))
        .ok_or_else(|| rejected("reviewed receipt hash missing"))?;
    let mut request = body.clone();
    let fields = request
        .as_object_mut()
        .ok_or_else(|| invalid("invalid reviewed request"))?;
    fields.extend(serde_json::Map::from_iter([
        ("family".into(), json!(FAMILY)),
        ("workspace_id".into(), json!(audience.binding.workspace)),
        ("project_id".into(), json!(audience.project_id)),
        ("record_id".into(), json!(id)),
        ("action".into(), json!(action)),
    ]));
    if response_hash != wire::fingerprint(&request)? {
        return Err(rejected("reviewed receipt request hash mismatch"));
    }
    let value: ReviewedProjectRecord =
        serde_json::from_value(response).map_err(|_| rejected("invalid reviewed receipt"))?;
    record(&value, company, audience, binding_hash, false)?;
    if value.record_id != id {
        return Err(rejected("reviewed receipt record mismatch"));
    }
    if action == "publish" {
        let Some(provenance) = &value.provenance else {
            return Err(rejected("reviewed publication provenance missing"));
        };
        if json!(value.content_hash) != body["content_hash"]
            || json!(provenance.approval_id) != body["approval_id"]
            || json!(provenance.source_hash) != body["source_hash"]
            || json!(provenance.approved_by) != body["approved_by"]
            || value
                .expires_at
                .map(timestamp)
                .transpose()?
                .map(Value::String)
                .as_ref()
                != Some(&body["expires_at"])
        {
            return Err(rejected("reviewed publication differs from approval"));
        }
    } else if !value.erased_from_reviewed_store {
        return Err(rejected("reviewed erasure was not proven"));
    }
    Ok(ReviewedProjectReceipt {
        record: value,
        request_hash: response_hash,
        created,
    })
}
