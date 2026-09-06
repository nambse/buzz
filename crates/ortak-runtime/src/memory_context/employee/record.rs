use super::*;
use chrono::{DateTime, Utc};
use ortak_control::memory::employee::{EmployeeMemoryDigest, EmployeeMemoryProvenanceV1};

/// Exact original employee approval, namespace and current epoch commitments.
/// This is a separate pin shape; it is never a legacy project pin.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewedEmployeePin {
    /// Immutable employee fact and remote record identity.
    pub fact_id: Uuid,
    /// Retained owned namespace/destination target.
    pub target_id: Uuid,
    /// Exact active approved version, one.
    pub fact_version: i64,
    /// Exact human-edited UTF-8 SHA-256.
    pub content_hash: String,
    /// Domain-separated audience/source hash.
    pub source_hash: String,
    /// SHA-256 of the exact canonical sharing provenance.
    pub sharing_hash: String,
    /// Hash of the explicit employee-owned audience.
    pub audience_hash: String,
    /// Original reviewed-employee protocol binding hash.
    pub binding_hash: String,
    /// Stable company/employee namespace hash.
    pub namespace_hash: String,
    /// Original explicit sharing-review operation.
    pub approval_id: Uuid,
    /// Original approving human's canonical public key.
    pub approved_by: String,
    /// Original fixed permitted-use expiry.
    pub expires_at: DateTime<Utc>,
    /// Source channel authority epoch observed during this selection.
    pub source_authority_epoch: i64,
    /// Destination channel authority epoch observed during this selection.
    pub destination_authority_epoch: i64,
    /// Independent selected target consumption epoch.
    pub consumption_epoch: i64,
}

/// Remote approved text with exact immutable employee sharing provenance.
#[derive(Clone, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewedEmployeeRecord {
    /// Separate employee namespace and approval commitments.
    pub pin: ReviewedEmployeePin,
    /// Exact remote UTF-8 text; deliberately excluded from Debug.
    pub content: String,
    /// Exact canonical v1 employee provenance JSON string.
    pub provenance: String,
}

impl ReviewedEmployeeRecord {
    pub(crate) fn validate(&self) -> Result<EmployeeMemoryProvenanceV1> {
        let p = &self.pin;
        if p.source_authority_epoch < 0
            || p.destination_authority_epoch < 0
            || [&p.sharing_hash, &p.audience_hash, &p.namespace_hash]
                .iter()
                .any(|v| EmployeeMemoryDigest::parse_hex(v).is_err())
        {
            return Err(rejected());
        }
        // Reuse only the legacy content/UUID/digest checks. Its temporary value
        // is never serialized, persisted or admitted as project authority.
        ReviewedMemoryContext {
            records: vec![ReviewedMemoryRecord {
                pin: ReviewedMemoryPin {
                    fact_id: p.fact_id,
                    target_id: p.target_id,
                    fact_version: p.fact_version,
                    consumption_epoch: p.consumption_epoch,
                    content_hash: p.content_hash.clone(),
                    source_hash: p.source_hash.clone(),
                    binding_hash: p.binding_hash.clone(),
                    approval_id: p.approval_id,
                    approved_by: p.approved_by.clone(),
                    expires_at: p.expires_at,
                },
                content: self.content.clone(),
            }],
            truncated: false,
        }
        .validate()?;
        let parsed = EmployeeMemoryProvenanceV1::from_canonical_bytes(self.provenance.as_bytes())
            .map_err(|_| rejected())?;
        let a = parsed.approval();
        if parsed
            .audience()
            .audience_hash()
            .map_err(|_| rejected())?
            .to_hex()
            != p.audience_hash
            || parsed.source_hash().map_err(|_| rejected())?.to_hex() != p.source_hash
            || parsed.sharing_hash().map_err(|_| rejected())?.to_hex() != p.sharing_hash
            || a.content_hash().to_hex() != p.content_hash
            || a.approval_id() != p.approval_id
            || a.approved_by().to_hex() != p.approved_by
            || a.expires_at() != p.expires_at
            || parsed.source().author_public_key() != a.approved_by()
        {
            return Err(rejected());
        }
        Ok(parsed)
    }
}

/// Ordered v5 union. Existing project and conversation record bytes are unchanged.
#[derive(Clone, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "scope", rename_all = "snake_case", deny_unknown_fields)]
pub enum EmployeeContextRecord {
    /// Existing reviewed project record; requires promoted Work authority.
    Project {
        /// Unchanged legacy record shape.
        record: ReviewedMemoryRecord,
    },
    /// Existing reviewed conversation record and exact conversation origin.
    Conversation {
        /// Unchanged v4 record shape.
        record: ReviewedConversationRecord,
    },
    /// Explicit destination-scoped employee or exact-human relationship record.
    Employee {
        /// Employee-owned record, without a project placeholder.
        record: ReviewedEmployeeRecord,
    },
}

impl EmployeeContextRecord {
    /// Fact identity inside its distinct table namespace.
    pub fn fact_id(&self) -> Uuid {
        match self {
            Self::Project { record } => record.pin.fact_id,
            Self::Conversation { record } => record.pin.fact_id,
            Self::Employee { record } => record.pin.fact_id,
        }
    }
    /// Exact text for combined budgeting.
    pub fn content(&self) -> &str {
        match self {
            Self::Project { record } => &record.content,
            Self::Conversation { record } => &record.content,
            Self::Employee { record } => &record.content,
        }
    }
    pub(crate) fn legacy(&self) -> Option<ReviewedContextRecord> {
        match self {
            Self::Project { record } => Some(ReviewedContextRecord::Project {
                record: record.clone(),
            }),
            Self::Conversation { record } => Some(ReviewedContextRecord::Conversation {
                record: record.clone(),
            }),
            Self::Employee { .. } => None,
        }
    }
    pub(crate) fn rendered(&self) -> Result<String> {
        let value = match self {
            Self::Employee { record } => {
                record.validate()?;
                serde_json::to_string(&serde_json::json!({"type":"reviewed_employee_memory","trust":"untrusted_data","record":record}))
                    .map_err(|_| rejected())?
            }
            _ => self.legacy().ok_or_else(rejected)?.rendered()?,
        };
        if value.len() > MAX_RENDERED_CONTEXT_BYTES {
            return Err(rejected());
        }
        Ok(value)
    }
}
