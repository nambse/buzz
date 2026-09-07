//! Explicit lexical field order matches the candidate SQL byte recipe.
use super::*;
use serde::Serialize;

pub(super) fn approval(employee: &EmployeeId, operation: Uuid, draft: &Draft) -> Result<Vec<u8>> {
    #[derive(Serialize)]
    struct Wire<'a> {
        action: &'static str,
        content: &'a str,
        destination_channel_id: Uuid,
        employee_id: &'a EmployeeId,
        expected_audience_hash: &'a str,
        expires_at: String,
        format: &'static str,
        human_public_key: &'a Option<String>,
        kind: Kind,
        operation_id: Uuid,
        reviewed: bool,
        source_event_created_at: String,
        source_event_id: &'a str,
    }
    encode(&Wire {
        action: "approve",
        content: &draft.content,
        destination_channel_id: draft.destination_channel_id,
        employee_id: employee,
        expected_audience_hash: &draft.expected_audience_hash,
        expires_at: timestamp(draft.expires_at)?,
        format: "ortak-reviewed-employee-command/1",
        human_public_key: &draft.human_public_key,
        kind: draft.kind,
        operation_id: operation,
        reviewed: draft.reviewed,
        source_event_created_at: timestamp(draft.source_event_created_at)?,
        source_event_id: &draft.source_event_id,
    })
}
pub(super) fn stop(operation: Uuid, fact: Uuid, version: i32) -> Result<Vec<u8>> {
    #[derive(Serialize)]
    struct Wire {
        action: &'static str,
        expected_version: i32,
        fact_id: Uuid,
        format: &'static str,
        operation_id: Uuid,
    }
    encode(&Wire {
        action: "stop",
        expected_version: version,
        fact_id: fact,
        format: "ortak-reviewed-employee-command/1",
        operation_id: operation,
    })
}
#[derive(Serialize)]
pub(super) struct SourceWire {
    author_public_key: String,
    channel_id: Uuid,
    community_id: Uuid,
    event_created_at: String,
    event_id: String,
    evidence_hash: String,
}
pub(super) fn source(value: &EmployeeMemorySourceV1) -> Result<SourceWire> {
    Ok(SourceWire {
        author_public_key: value.author_public_key().to_hex(),
        channel_id: value.channel_id(),
        community_id: value.community_id(),
        event_created_at: timestamp(value.event_created_at())?,
        event_id: value.event_id().to_hex(),
        evidence_hash: value.evidence_hash().to_hex(),
    })
}
pub(super) fn source_hash(
    audience: &EmployeeMemoryAudienceV1,
    value: &EmployeeMemorySourceV1,
) -> Result<EmployeeMemoryDigest> {
    #[derive(Serialize)]
    struct Wire {
        audience_hash: String,
        format: &'static str,
        source: SourceWire,
    }
    Ok(digest(&encode(&Wire {
        audience_hash: audience
            .audience_hash()
            .map_err(|_| ApiError::unavailable())?
            .to_hex(),
        format: EMPLOYEE_MEMORY_SOURCE_FORMAT_V1,
        source: source(value)?,
    })?))
}
fn encode(value: &impl Serialize) -> Result<Vec<u8>> {
    let bytes = serde_json::to_vec(value).map_err(|_| ApiError::invalid())?;
    if bytes.len() > 32768 {
        return Err(ApiError::invalid());
    }
    Ok(bytes)
}
