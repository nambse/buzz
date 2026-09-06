use super::*;
use crate::MessageId;
use chrono::{DateTime, Datelike, SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

// Struct declaration order is lexicographic at every object level. Never use
// JSONB::text or map insertion order as an alternative canonical encoding.
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct AudienceWire {
    company_id: Uuid,
    destination_channel_id: Uuid,
    destination_community_id: Uuid,
    employee_id: EmployeeId,
    format: String,
    human_public_key: Option<String>,
    kind: String,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct OfficeSourceWire {
    author_public_key: String,
    channel_id: Uuid,
    community_id: Uuid,
    event_created_at: String,
    event_id: String,
    evidence_hash: String,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ApprovalWire {
    approval_id: Uuid,
    approved_by: String,
    content_hash: String,
    expires_at: String,
    format: String,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ProvenanceWire {
    approval: ApprovalWire,
    audience: AudienceWire,
    audience_hash: String,
    format: String,
    source: OfficeSourceWire,
    source_hash: String,
}

#[derive(Serialize)]
struct SourceBindingWire {
    audience_hash: String,
    format: &'static str,
    source: OfficeSourceWire,
}

pub(super) fn timestamp(value: DateTime<Utc>) -> Result<String, EmployeeMemoryError> {
    if !(1970..=9999).contains(&value.year())
        || value.timestamp_subsec_nanos() >= 1_000_000_000
        || !value.timestamp_subsec_nanos().is_multiple_of(1000)
    {
        return Err(EmployeeMemoryError::InvalidTimestamp);
    }
    Ok(value.to_rfc3339_opts(SecondsFormat::Micros, true))
}

fn parse_timestamp(value: &str) -> Result<DateTime<Utc>, EmployeeMemoryError> {
    let parsed = DateTime::parse_from_rfc3339(value)
        .map_err(|_| EmployeeMemoryError::InvalidTimestamp)?
        .with_timezone(&Utc);
    if timestamp(parsed)? != value {
        return Err(EmployeeMemoryError::InvalidTimestamp);
    }
    Ok(parsed)
}

fn public_key(value: &str) -> Result<OfficePublicKey, EmployeeMemoryError> {
    EmployeeMemoryDigest::parse_hex(value)?;
    OfficePublicKey::parse_hex(value).map_err(|_| EmployeeMemoryError::InvalidDigest)
}

impl AudienceWire {
    fn from_value(value: &EmployeeMemoryAudienceV1) -> Self {
        Self {
            company_id: value.company_id,
            destination_channel_id: value.destination_channel_id,
            destination_community_id: value.destination_community_id,
            employee_id: value.employee_id.clone(),
            format: EMPLOYEE_MEMORY_AUDIENCE_FORMAT_V1.into(),
            human_public_key: value.human_public_key.map(|key| key.to_hex()),
            kind: match value.kind() {
                EmployeeMemoryKind::Experience => "experience",
                EmployeeMemoryKind::Relationship => "relationship",
            }
            .into(),
        }
    }

    fn into_value(self) -> Result<EmployeeMemoryAudienceV1, EmployeeMemoryError> {
        if self.format != EMPLOYEE_MEMORY_AUDIENCE_FORMAT_V1 {
            return Err(EmployeeMemoryError::InvalidWire);
        }
        match (self.kind.as_str(), self.human_public_key.as_deref()) {
            ("experience", None) => EmployeeMemoryAudienceV1::experience(
                self.company_id,
                self.employee_id,
                self.destination_community_id,
                self.destination_channel_id,
            ),
            ("relationship", Some(human)) => EmployeeMemoryAudienceV1::relationship(
                self.company_id,
                self.employee_id,
                self.destination_community_id,
                self.destination_channel_id,
                public_key(human)?,
            ),
            _ => Err(EmployeeMemoryError::InvalidWire),
        }
    }
}

impl OfficeSourceWire {
    fn from_value(value: &EmployeeMemorySourceV1) -> Result<Self, EmployeeMemoryError> {
        Ok(Self {
            author_public_key: value.author_public_key().to_hex(),
            channel_id: value.channel_id(),
            community_id: value.community_id(),
            event_created_at: timestamp(value.event_created_at())?,
            event_id: value.event_id().to_hex(),
            evidence_hash: value.evidence_hash().to_hex(),
        })
    }

    fn into_value(self) -> Result<EmployeeMemorySourceV1, EmployeeMemoryError> {
        EmployeeMemorySourceV1::new(
            self.community_id,
            self.channel_id,
            MessageId::from_bytes(*EmployeeMemoryDigest::parse_hex(&self.event_id)?.as_bytes()),
            parse_timestamp(&self.event_created_at)?,
            public_key(&self.author_public_key)?,
            EmployeeMemoryDigest::parse_hex(&self.evidence_hash)?,
        )
    }
}

impl ApprovalWire {
    fn from_value(value: &EmployeeSharingApprovalV1) -> Result<Self, EmployeeMemoryError> {
        Ok(Self {
            approval_id: value.approval_id(),
            approved_by: value.approved_by().to_hex(),
            content_hash: value.content_hash().to_hex(),
            expires_at: timestamp(value.expires_at())?,
            format: EMPLOYEE_MEMORY_SHARING_FORMAT_V1.into(),
        })
    }

    fn into_value(self) -> Result<EmployeeSharingApprovalV1, EmployeeMemoryError> {
        if self.format != EMPLOYEE_MEMORY_SHARING_FORMAT_V1 {
            return Err(EmployeeMemoryError::InvalidWire);
        }
        EmployeeSharingApprovalV1::new(
            self.approval_id,
            public_key(&self.approved_by)?,
            EmployeeMemoryDigest::parse_hex(&self.content_hash)?,
            parse_timestamp(&self.expires_at)?,
        )
    }
}

pub(super) fn digest(bytes: &[u8]) -> EmployeeMemoryDigest {
    EmployeeMemoryDigest::from_bytes(Sha256::digest(bytes).into())
}

fn encode(value: &impl Serialize, max: usize) -> Result<Vec<u8>, EmployeeMemoryError> {
    let bytes = serde_json::to_vec(value).map_err(|_| EmployeeMemoryError::InvalidWire)?;
    if bytes.len() > max {
        return Err(EmployeeMemoryError::InvalidWire);
    }
    Ok(bytes)
}

pub(super) fn audience_bytes(
    value: &EmployeeMemoryAudienceV1,
) -> Result<Vec<u8>, EmployeeMemoryError> {
    encode(
        &AudienceWire::from_value(value),
        MAX_EMPLOYEE_MEMORY_AUDIENCE_BYTES,
    )
}

pub(super) fn source_hash(
    value: &EmployeeMemoryProvenanceV1,
) -> Result<EmployeeMemoryDigest, EmployeeMemoryError> {
    Ok(digest(&encode(
        &SourceBindingWire {
            audience_hash: value.audience.audience_hash()?.to_hex(),
            format: EMPLOYEE_MEMORY_SOURCE_FORMAT_V1,
            source: OfficeSourceWire::from_value(&value.source)?,
        },
        MAX_EMPLOYEE_MEMORY_PROVENANCE_BYTES,
    )?))
}

pub(super) fn provenance_bytes(
    value: &EmployeeMemoryProvenanceV1,
) -> Result<Vec<u8>, EmployeeMemoryError> {
    encode(
        &ProvenanceWire {
            approval: ApprovalWire::from_value(&value.approval)?,
            audience: AudienceWire::from_value(&value.audience),
            audience_hash: value.audience.audience_hash()?.to_hex(),
            format: EMPLOYEE_MEMORY_PROVENANCE_FORMAT_V1.into(),
            source: OfficeSourceWire::from_value(&value.source)?,
            source_hash: value.source_hash()?.to_hex(),
        },
        MAX_EMPLOYEE_MEMORY_PROVENANCE_BYTES,
    )
}

pub(super) fn parse_audience(
    bytes: &[u8],
) -> Result<EmployeeMemoryAudienceV1, EmployeeMemoryError> {
    if bytes.is_empty() || bytes.len() > MAX_EMPLOYEE_MEMORY_AUDIENCE_BYTES {
        return Err(EmployeeMemoryError::InvalidWire);
    }
    let wire: AudienceWire =
        serde_json::from_slice(bytes).map_err(|_| EmployeeMemoryError::InvalidWire)?;
    let value = wire.into_value()?;
    if value.canonical_bytes()? != bytes {
        return Err(EmployeeMemoryError::InvalidWire);
    }
    Ok(value)
}

pub(super) fn parse_provenance(
    bytes: &[u8],
) -> Result<EmployeeMemoryProvenanceV1, EmployeeMemoryError> {
    if bytes.is_empty() || bytes.len() > MAX_EMPLOYEE_MEMORY_PROVENANCE_BYTES {
        return Err(EmployeeMemoryError::InvalidWire);
    }
    let wire: ProvenanceWire =
        serde_json::from_slice(bytes).map_err(|_| EmployeeMemoryError::InvalidWire)?;
    if wire.format != EMPLOYEE_MEMORY_PROVENANCE_FORMAT_V1 {
        return Err(EmployeeMemoryError::InvalidWire);
    }
    let expected_audience = EmployeeMemoryDigest::parse_hex(&wire.audience_hash)?;
    let expected_source = EmployeeMemoryDigest::parse_hex(&wire.source_hash)?;
    let value = EmployeeMemoryProvenanceV1::new(
        wire.audience.into_value()?,
        wire.source.into_value()?,
        wire.approval.into_value()?,
    )?;
    if value.audience().audience_hash()? != expected_audience
        || value.source_hash()? != expected_source
    {
        return Err(EmployeeMemoryError::InconsistentProvenance);
    }
    if value.canonical_bytes()? != bytes {
        return Err(EmployeeMemoryError::InvalidWire);
    }
    Ok(value)
}
