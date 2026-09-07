use super::*;
use chrono::{DateTime, Utc};
use ortak_control::memory::conversation::ConversationProvenanceV1;

/// Explicit v4 conversation approval/export pins; legacy project pins are unchanged.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewedConversationPin {
    /// Durable fact and selected remote record identity.
    pub fact_id: Uuid,
    /// Exact retained owned publication target.
    pub target_id: Uuid,
    /// Active approval version, exactly one.
    pub fact_version: i64,
    /// Legacy project-consumption sentinel, exactly zero for conversation use.
    pub consumption_epoch: i64,
    /// Hash of the human-approved UTF-8 content bytes.
    pub content_hash: String,
    /// Audience-bound canonical conversation source hash.
    pub source_hash: String,
    /// Exact retained memory binding/ownership hash.
    pub binding_hash: String,
    /// Original human approval operation.
    pub approval_id: Uuid,
    /// Approving human's lowercase hexadecimal public key.
    pub approved_by: String,
    /// Original permitted-use expiry; current time is checked by the repository.
    pub expires_at: DateTime<Utc>,
    /// Canonical fact audience hash, independent of the source anchor.
    pub conversation_audience_hash: String,
    /// Monotonic selected project/channel authority epoch.
    pub conversation_authority_epoch: i64,
    /// Independent conversation target consumption epoch.
    pub conversation_consumption_epoch: i64,
}

impl ReviewedConversationPin {
    fn common(&self) -> ReviewedMemoryPin {
        ReviewedMemoryPin {
            fact_id: self.fact_id,
            target_id: self.target_id,
            fact_version: self.fact_version,
            consumption_epoch: self.consumption_epoch,
            content_hash: self.content_hash.clone(),
            source_hash: self.source_hash.clone(),
            binding_hash: self.binding_hash.clone(),
            approval_id: self.approval_id,
            approved_by: self.approved_by.clone(),
            expires_at: self.expires_at,
        }
    }
}

/// Selected remote bytes with original approval and canonical source attribution.
#[derive(Clone, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewedConversationRecord {
    /// Separate explicit conversation pins; no flattened legacy fallback.
    pub pin: ReviewedConversationPin,
    /// Exact human-approved remote content, never substituted from a local row.
    pub content: String,
    /// Exact canonical v1 fact provenance JSON string.
    pub provenance: String,
}

impl ReviewedConversationRecord {
    pub(crate) fn validate(&self) -> Result<ConversationProvenanceV1> {
        let p = &self.pin;
        if p.consumption_epoch != 0
            || p.conversation_authority_epoch < 0
            || p.conversation_consumption_epoch < 0
        {
            return Err(rejected());
        }
        // Reuse unchanged common content/hash/identifier checks without
        // serializing the conversation pin as a legacy project pin.
        ReviewedMemoryContext {
            records: vec![ReviewedMemoryRecord {
                pin: p.common(),
                content: self.content.clone(),
            }],
            truncated: false,
        }
        .validate()?;
        let parsed = ConversationProvenanceV1::from_canonical_bytes(self.provenance.as_bytes())
            .map_err(|_| rejected())?;
        if parsed
            .audience()
            .audience_hash()
            .map_err(|_| rejected())?
            .to_hex()
            != p.conversation_audience_hash
            || parsed.source_hash().map_err(|_| rejected())?.to_hex() != p.source_hash
        {
            return Err(rejected());
        }
        Ok(parsed)
    }
}

/// One ordered v4 reviewed record. The scope tag is not part of rendered context.
#[derive(Clone, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "scope", rename_all = "snake_case", deny_unknown_fields)]
pub enum ReviewedContextRecord {
    /// Unchanged legacy project record, permitted only on promoted Work.
    Project {
        /// Exact legacy record and pin shape.
        record: ReviewedMemoryRecord,
    },
    /// Fact scoped to the canonical run thread or its containing channel.
    Conversation {
        /// Conversation record with its separate provenance and epoch pins.
        record: ReviewedConversationRecord,
    },
}

impl ReviewedContextRecord {
    /// Fact identity shared by the duplicate guard across both variants.
    pub fn fact_id(&self) -> Uuid {
        match self {
            Self::Project { record } => record.pin.fact_id,
            Self::Conversation { record } => record.pin.fact_id,
        }
    }

    /// Exact content bytes for combined context budgeting.
    pub fn content(&self) -> &str {
        match self {
            Self::Project { record } => &record.content,
            Self::Conversation { record } => &record.content,
        }
    }

    /// Bounded provider-facing JSON, with one untrusted-data wrapper per record.
    pub(crate) fn rendered(&self) -> Result<String> {
        let rendered = match self {
            Self::Project { record } => ReviewedMemoryContext {
                records: vec![record.clone()],
                truncated: false,
            }
            .rendered()?
            .into_iter()
            .next()
            .ok_or_else(rejected)?,
            Self::Conversation { record } => {
                record.validate()?;
                serde_json::to_string(&serde_json::json!({
                    "type":"reviewed_conversation_memory","trust":"untrusted_data","record":record
                }))
                .map_err(|_| rejected())?
            }
        };
        if rendered.len() > MAX_RENDERED_CONTEXT_BYTES {
            return Err(rejected());
        }
        Ok(rendered)
    }
}
