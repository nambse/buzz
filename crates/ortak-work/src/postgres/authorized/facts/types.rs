//! Human-approved project context; it is not a Honcho deletion receipt.
use super::*;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Canonical evidence reviewed by the approving human.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ReviewedFactSource {
    /// A visible, decided message in the project's bound Office channel.
    Conversation {
        /// Canonical event identifier; its author, partition and channel are checked.
        message_id: String,
    },
    /// A retained complete text artifact from this project and employee.
    Artifact {
        /// Server-generated artifact identifier.
        artifact_id: Uuid,
    },
}

/// An explicit human review, with no runtime/model-authored scope or approval.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewedFactDraft {
    /// Exactly one configured employee may use the project context.
    pub employee_id: EmployeeId,
    /// Evidence reviewed by the human.
    pub source: ReviewedFactSource,
    /// Edited and redacted fact, never copied automatically from source output.
    pub content: String,
    /// Explicit end of permitted use; this does not erase storage or backups.
    pub expires_at: DateTime<Utc>,
    /// The human confirms reviewing the fact and its audience.
    pub reviewed: bool,
}
impl ReviewedFactDraft {
    pub(super) fn validate(&self) -> Result<()> {
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
                "reviewed fact text or confirmation is invalid",
            ));
        }
        match &self.source {
            ReviewedFactSource::Conversation { message_id } => {
                MessageId::parse_hex(message_id)?;
            }
            ReviewedFactSource::Artifact { artifact_id } if artifact_id.is_nil() => {
                return Err(WorkError::InvalidQuery("artifact id must not be nil"));
            }
            ReviewedFactSource::Artifact { .. } => {}
        }
        Ok(())
    }
}

/// A retained reviewed fact, including current permitted-use state.
#[derive(Clone, Debug, Serialize)]
pub struct ReviewedFact {
    /// Stable fact identifier.
    pub id: Uuid,
    /// Its exclusive project boundary.
    pub project_id: Uuid,
    /// Its exclusive employee boundary.
    pub employee_id: EmployeeId,
    /// Canonical evidence; never an arbitrary URL or private path.
    pub source: Option<ReviewedFactSource>,
    /// Human-edited text. Retained after expiry/revocation for inspection.
    pub content: Option<String>,
    /// False withholds content/evidence while preserving authorized stop-use recovery.
    pub source_visible: bool,
    /// Initial approval is version one; revocation advances it once.
    pub version: i64,
    /// Current active, expired or revoked state, evaluated in the read transaction.
    pub status: String,
    /// Human that approved the audience and edited content.
    pub approved_by: String,
    /// Stable approval time.
    pub approved_at: DateTime<Utc>,
    /// End of permitted use, not a promise of physical erasure.
    pub expires_at: DateTime<Utc>,
    /// Human that stopped use, when applicable.
    pub revoked_by: Option<String>,
    /// Retained revocation time.
    pub revoked_at: Option<DateTime<Utc>>,
    /// Explicit bounded reason for stopping use.
    pub revoke_reason: Option<String>,
    /// Current worker target exists; publication still requires an explicit review action.
    pub publication_available: bool,
    /// Hash-only remote publication/cleanup progress; absent for preview-only facts.
    pub export: Option<crate::reviewed_exports::ReviewedExportView>,
}

/// One finite inspection page; refresh the first page to see later insertions.
#[derive(Clone, Debug, Serialize)]
pub struct ReviewedFactPage {
    /// At most 25 currently visible facts, including expired/revoked evidence.
    pub facts: Vec<ReviewedFact>,
    /// Continue in stable UUID order within the same project/employee scope.
    pub next_after: Option<Uuid>,
}

/// An atomic mutation receipt and its current authorized record.
#[derive(Clone, Debug, Serialize)]
pub struct ReviewedFactReceipt {
    /// Current authorized fact. A replay may show a later revocation.
    pub fact: ReviewedFact,
    /// True only when this operation committed for the first time.
    pub created: bool,
}

/// A bounded preview of reviewed project context; never raw Honcho contents.
#[derive(Clone, Debug, Serialize)]
pub struct ReviewedFactRecall {
    /// Only active, unexpired, currently authorized facts for one employee.
    pub facts: Vec<ReviewedFact>,
    /// True when the eight-record or eight-KiB budget omitted matches.
    pub truncated: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reviewed_fact_rejects_unconfirmed_secret_or_oversized_text() {
        let mut draft = ReviewedFactDraft {
            employee_id: EmployeeId::parse("cem").unwrap(),
            source: ReviewedFactSource::Artifact {
                artifact_id: Uuid::new_v4(),
            },
            content: "Human reviewed project fact".into(),
            expires_at: Utc::now(),
            reviewed: true,
        };
        assert!(draft.validate().is_ok());
        draft.reviewed = false;
        assert!(draft.validate().is_err());
        draft.reviewed = true;
        draft.content = "é".repeat(2049);
        assert!(draft.validate().is_err());
        draft.content = "value\0hidden".into();
        assert!(draft.validate().is_err());
        draft.content = "api_key=must-not-be-stored".into();
        assert!(draft.validate().is_err());
    }
}
