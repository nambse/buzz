use super::*;
use chrono::{Datelike, SecondsFormat};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

// Declaration order is deliberately lexicographic, including the nested
// audience. Serialize structs directly so serde_json map feature flags cannot
// change canonical bytes. New fields/normalization require a new format.
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct AudienceWire {
    channel_id: Uuid,
    community_id: Uuid,
    company_id: Uuid,
    employee_id: EmployeeId,
    format: String,
    kind: String,
    project_id: Uuid,
    thread_root_event_created_at: Option<String>,
    thread_root_event_id: Option<String>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ProvenanceWire {
    audience: AudienceWire,
    audience_hash: String,
    format: String,
    source_event_created_at: String,
    source_event_id: String,
    source_evidence_hash: String,
    source_hash: String,
}

#[derive(Serialize)]
struct SourceWire {
    audience_hash: String,
    format: &'static str,
    source_evidence_hash: String,
}

pub(super) fn timestamp(value: DateTime<Utc>) -> Result<String, ConversationMemoryError> {
    if !(1970..=9999).contains(&value.year())
        || value.timestamp_subsec_nanos() >= 1_000_000_000
        || !value.timestamp_subsec_nanos().is_multiple_of(1000)
    {
        return Err(ConversationMemoryError::InvalidTimestamp);
    }
    Ok(value.to_rfc3339_opts(SecondsFormat::Micros, true))
}

fn event(id: &str, created_at: &str) -> Result<ConversationEventIdentity, ConversationMemoryError> {
    // Use the closed digest parser rather than MessageId's legacy error that
    // includes its rejected input. The wire never accepts uppercase IDs.
    let id = ConversationMemoryDigest::parse_hex(id)?;
    let parsed = DateTime::parse_from_rfc3339(created_at)
        .map_err(|_| ConversationMemoryError::InvalidTimestamp)?
        .with_timezone(&Utc);
    if timestamp(parsed)? != created_at {
        return Err(ConversationMemoryError::InvalidTimestamp);
    }
    ConversationEventIdentity::new(MessageId::from_bytes(*id.as_bytes()), parsed)
}

impl AudienceWire {
    fn from_value(value: &ConversationAudienceV1) -> Result<Self, ConversationMemoryError> {
        Ok(Self {
            channel_id: value.channel_id,
            community_id: value.community_id,
            company_id: value.company_id,
            employee_id: value.employee_id.clone(),
            format: CONVERSATION_AUDIENCE_FORMAT_V1.into(),
            kind: if value.thread_root.is_some() {
                "thread"
            } else {
                "channel"
            }
            .into(),
            project_id: value.project_id,
            thread_root_event_created_at: value
                .thread_root
                .as_ref()
                .map(|root| timestamp(root.created_at))
                .transpose()?,
            thread_root_event_id: value
                .thread_root
                .as_ref()
                .map(|root| root.event_id.to_hex()),
        })
    }

    fn into_value(self) -> Result<ConversationAudienceV1, ConversationMemoryError> {
        if self.format != CONVERSATION_AUDIENCE_FORMAT_V1 {
            return Err(ConversationMemoryError::InvalidWire);
        }
        let root = match (
            self.kind.as_str(),
            self.thread_root_event_id.as_deref(),
            self.thread_root_event_created_at.as_deref(),
        ) {
            ("channel", None, None) => None,
            ("thread", Some(id), Some(at)) => Some(event(id, at)?),
            _ => return Err(ConversationMemoryError::InvalidWire),
        };
        let mut value = ConversationAudienceV1::channel(
            self.company_id,
            self.community_id,
            self.project_id,
            self.employee_id,
            self.channel_id,
        )?;
        value.thread_root = root;
        Ok(value)
    }
}

pub(super) fn digest(bytes: &[u8]) -> ConversationMemoryDigest {
    ConversationMemoryDigest::from_bytes(Sha256::digest(bytes).into())
}

fn encode(value: &impl Serialize) -> Result<Vec<u8>, ConversationMemoryError> {
    serde_json::to_vec(value).map_err(|_| ConversationMemoryError::InvalidWire)
}

pub(super) fn audience_bytes(
    value: &ConversationAudienceV1,
) -> Result<Vec<u8>, ConversationMemoryError> {
    encode(&AudienceWire::from_value(value)?)
}

pub(super) fn source_hash(
    value: &ConversationProvenanceV1,
) -> Result<ConversationMemoryDigest, ConversationMemoryError> {
    Ok(digest(&encode(&SourceWire {
        audience_hash: value.audience.audience_hash()?.to_hex(),
        format: CONVERSATION_SOURCE_FORMAT_V1,
        source_evidence_hash: value.source_evidence_hash.to_hex(),
    })?))
}

pub(super) fn provenance_bytes(
    value: &ConversationProvenanceV1,
) -> Result<Vec<u8>, ConversationMemoryError> {
    encode(&ProvenanceWire {
        audience: AudienceWire::from_value(&value.audience)?,
        audience_hash: value.audience.audience_hash()?.to_hex(),
        format: CONVERSATION_PROVENANCE_FORMAT_V1.into(),
        source_event_created_at: timestamp(value.source.created_at)?,
        source_event_id: value.source.event_id.to_hex(),
        source_evidence_hash: value.source_evidence_hash.to_hex(),
        source_hash: value.source_hash()?.to_hex(),
    })
}

pub(super) fn parse_audience(
    bytes: &[u8],
) -> Result<ConversationAudienceV1, ConversationMemoryError> {
    if bytes.is_empty() || bytes.len() > MAX_CONVERSATION_AUDIENCE_BYTES {
        return Err(ConversationMemoryError::InvalidWire);
    }
    let wire: AudienceWire =
        serde_json::from_slice(bytes).map_err(|_| ConversationMemoryError::InvalidWire)?;
    let value = wire.into_value()?;
    if value.canonical_bytes()? != bytes {
        return Err(ConversationMemoryError::InvalidWire);
    }
    Ok(value)
}

pub(super) fn parse_provenance(
    bytes: &[u8],
) -> Result<ConversationProvenanceV1, ConversationMemoryError> {
    if bytes.is_empty() || bytes.len() > MAX_CONVERSATION_PROVENANCE_BYTES {
        return Err(ConversationMemoryError::InvalidWire);
    }
    let wire: ProvenanceWire =
        serde_json::from_slice(bytes).map_err(|_| ConversationMemoryError::InvalidWire)?;
    if wire.format != CONVERSATION_PROVENANCE_FORMAT_V1 {
        return Err(ConversationMemoryError::InvalidWire);
    }
    let expected_audience = ConversationMemoryDigest::parse_hex(&wire.audience_hash)?;
    let expected_source = ConversationMemoryDigest::parse_hex(&wire.source_hash)?;
    let value = ConversationProvenanceV1::new(
        wire.audience.into_value()?,
        event(&wire.source_event_id, &wire.source_event_created_at)?,
        ConversationMemoryDigest::parse_hex(&wire.source_evidence_hash)?,
    )?;
    if value.audience.audience_hash()? != expected_audience
        || value.source_hash()? != expected_source
    {
        return Err(ConversationMemoryError::InconsistentProvenance);
    }
    if value.canonical_bytes()? != bytes {
        return Err(ConversationMemoryError::InvalidWire);
    }
    Ok(value)
}
