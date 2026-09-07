//! Submitted values are separate from current canonical resolver observations.
use super::*;
use chrono::{Datelike, SecondsFormat};
use ortak_control::memory::conversation::{ConversationAudienceKind, ConversationMemoryDigest};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Version of the immutable submitted-field preimage; legacy fact hashes differ.
pub const REVIEWED_CONVERSATION_DRAFT_FORMAT_V1: &str = "ortak-reviewed-conversation-draft/1";

/// Explicit audience choice. A caller cannot supply a root, partition or project scope.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ConversationMemoryAudience {
    /// Only the canonical thread containing the selected source message.
    Thread {},
    /// The whole bound stream channel; never selected as an automatic fallback.
    Channel {},
}

impl ConversationMemoryAudience {
    pub(super) fn kind(self) -> ConversationAudienceKind {
        match self {
            Self::Thread {} => ConversationAudienceKind::Thread,
            Self::Channel {} => ConversationAudienceKind::Channel,
        }
    }
}

/// Strict preview input. Project and human authority come from the signed handler.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewedConversationPreviewRequest {
    /// One employee in the server-configured grant ceiling.
    pub employee_id: EmployeeId,
    /// Exact lowercase event ID; the server resolves partition and canonical ancestry.
    pub source_message_id: String,
    /// Required explicit choice, with no default or fallback.
    pub audience: ConversationMemoryAudience,
}

impl ReviewedConversationPreviewRequest {
    /// Validate submitted syntax only; existence and current authority require the resolver.
    pub fn validate(&self) -> Result<()> {
        source_id(&self.source_message_id)?;
        Ok(())
    }
}

/// Current metadata for human review. Contains no source text or proposed fact text.
/// It is not retained approval, scoped epoch, publication or runtime authority.
#[derive(Clone, Debug, Serialize)]
pub struct ReviewedConversationPreview {
    /// Exact canonical v1 audience object, including explicit null channel root fields.
    pub audience: serde_json::Value,
    /// Digest of that canonical audience, independent of the source anchor.
    pub audience_hash: String,
    /// Canonical v1 source locator and server evidence hashes; no source body or author key.
    pub provenance: serde_json::Value,
    /// Database observation time; changes can invalidate the observation immediately.
    pub observed_at: DateTime<Utc>,
    /// Earliest current Office/channel expiry; not a promise of validity until this time.
    pub valid_before: Option<DateTime<Utc>>,
    /// Maximum prospective fact expiry: 90 days or the earlier current deadline.
    pub max_expires_at: DateTime<Utc>,
}

/// Future approval input, kept separate from legacy project fact definitions.
/// These fields do not authorize insertion, publication or recall by themselves.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewedConversationFactDraft {
    /// Sole selected employee.
    pub employee_id: EmployeeId,
    /// Source ID only, without a client-supplied root, partition or evidence hash.
    pub source_message_id: String,
    /// Explicit audience selected during review.
    pub audience: ConversationMemoryAudience,
    /// Exact canonical audience digest shown by a preceding authorized preview.
    pub expected_audience_hash: String,
    /// Human-edited bounded text; never copied automatically from the source.
    pub content: String,
    /// Explicit end of allowed use, with lossless PostgreSQL microsecond precision.
    pub expires_at: DateTime<Utc>,
    /// Human confirmation of the edited text and displayed audience.
    pub reviewed: bool,
}

impl ReviewedConversationFactDraft {
    /// Validate immutable submitted fields only, without consulting time or current ACL.
    /// Later receipt recovery must compare this fingerprint before new-admission expiry.
    pub fn validate(&self) -> Result<()> {
        source_id(&self.source_message_id)?;
        ConversationMemoryDigest::parse_hex(&self.expected_audience_hash)
            .map_err(|_| WorkError::InvalidQuery("invalid expected conversation audience hash"))?;
        valid_time(self.expires_at)?;
        if !self.reviewed
            || self.content.trim().is_empty()
            || self.content.len() > 4096
            || self
                .content
                .chars()
                .any(|c| c.is_control() && !matches!(c, '\n' | '\t'))
            || ortak_control::run_event::RedactionPolicy::new().redact(&self.content)
                != self.content
        {
            return Err(WorkError::InvalidQuery(
                "reviewed conversation text or confirmation is invalid",
            ));
        }
        Ok(())
    }

    /// Hash only the immutable submitted fields and selected project under a new version.
    /// No observed time, source evidence, resolved root or current epoch enters this value.
    /// This is a future replay key, never a durable receipt or authorization claim.
    pub fn submitted_fingerprint(&self, project_id: Uuid) -> Result<[u8; 32]> {
        self.validate()?;
        if project_id.is_nil() {
            return Err(WorkError::InvalidQuery("project id must not be nil"));
        }
        // Declaration order is the canonical recursively lexical JSON order.
        #[derive(Serialize)]
        struct Submitted<'a> {
            audience: ConversationMemoryAudience,
            content: &'a str,
            employee_id: &'a EmployeeId,
            expected_audience_hash: &'a str,
            expires_at: String,
            format: &'static str,
            project_id: Uuid,
            reviewed: bool,
            source_message_id: &'a str,
        }
        let bytes = serde_json::to_vec(&Submitted {
            audience: self.audience,
            content: &self.content,
            employee_id: &self.employee_id,
            expected_audience_hash: &self.expected_audience_hash,
            expires_at: self.expires_at.to_rfc3339_opts(SecondsFormat::Micros, true),
            format: REVIEWED_CONVERSATION_DRAFT_FORMAT_V1,
            project_id,
            reviewed: self.reviewed,
            source_message_id: &self.source_message_id,
        })
        .map_err(|_| WorkError::InvalidQuery("invalid conversation draft wire value"))?;
        Ok(Sha256::digest(bytes).into())
    }

    /// Validate expiry for a new approval against fresh database time and current deadline.
    /// Receipt replay must not use this method to erase recovery after an ordinary expiry.
    pub fn validate_expiry(
        &self,
        observed_at: DateTime<Utc>,
        valid_before: Option<DateTime<Utc>>,
    ) -> Result<()> {
        self.validate()?;
        if self.expires_at <= observed_at
            || self.expires_at > expiry_limit(observed_at, valid_before)?
        {
            return Err(WorkError::InvalidQuery(
                "conversation fact expiry exceeds its current allowance",
            ));
        }
        Ok(())
    }
}

pub(super) fn source_id(value: &str) -> Result<MessageId> {
    // Validate before MessageId's legacy parser, whose diagnostic can echo input.
    ConversationMemoryDigest::parse_hex(value)
        .map(|hash| MessageId::from_bytes(*hash.as_bytes()))
        .map_err(|_| WorkError::InvalidQuery("invalid conversation source id"))
}

fn valid_time(value: DateTime<Utc>) -> Result<()> {
    if !(1970..=9999).contains(&value.year())
        || value.timestamp_subsec_nanos() >= 1_000_000_000
        || !value.timestamp_subsec_nanos().is_multiple_of(1000)
    {
        return Err(WorkError::InvalidQuery("invalid conversation timestamp"));
    }
    Ok(())
}

pub(super) fn expiry_limit(
    observed_at: DateTime<Utc>,
    valid_before: Option<DateTime<Utc>>,
) -> Result<DateTime<Utc>> {
    valid_time(observed_at)?;
    let maximum = observed_at
        .checked_add_signed(chrono::Duration::days(90))
        .ok_or(WorkError::InvalidQuery(
            "invalid conversation expiry allowance",
        ))?;
    valid_time(maximum)?;
    if let Some(deadline) = valid_before {
        valid_time(deadline)?;
        if deadline <= observed_at {
            return Err(WorkError::OperationTimedOut);
        }
        Ok(maximum.min(deadline))
    } else {
        Ok(maximum)
    }
}
