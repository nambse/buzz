use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{DomainError, EmployeeId};

/// Absolute process-safety ceiling for recipients in one decision.
pub const HARD_MAX_RECIPIENTS: usize = 16;
/// Absolute process-safety ceiling for successful dispatch batches in a chain.
pub const HARD_MAX_CHAIN_HOPS: u8 = 8;
/// Absolute process-safety ceiling for unique employee wakes in a chain.
pub const HARD_MAX_CHAIN_WAKES: usize = 64;
/// Absolute process-safety ceiling for message bytes admitted to routing.
pub const HARD_MAX_MESSAGE_BYTES: usize = 64 * 1024;

/// Company-wide policy limits applied to every routing decision.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RoutingPolicy {
    /// Version recorded with each durable decision.
    pub version: String,
    /// Company-wide semantic score floor.
    pub semantic_threshold: f32,
    /// Maximum recipients for one routing decision.
    pub max_recipients: usize,
    /// Maximum successful dispatch batches, including the initial root batch.
    pub max_hops: u8,
    /// Maximum total employee wakes in a delivery chain.
    pub max_chain_wakes: usize,
    /// Maximum message bytes exposed to parsing or semantic scoring.
    pub max_message_bytes: usize,
}

impl RoutingPolicy {
    /// Validates bounded policy values.
    pub fn validate(&self) -> Result<(), DomainError> {
        if self.version.is_empty()
            || self.version.len() > 64
            || !self.version.chars().all(|character| {
                character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.')
            })
        {
            return Err(DomainError::EmptyField {
                field: "routing.version",
            });
        }
        if !self.semantic_threshold.is_finite() || !(0.0..=1.0).contains(&self.semantic_threshold) {
            return Err(DomainError::InvalidScore {
                field: "routing.semantic_threshold",
            });
        }
        if self.max_recipients == 0 || self.max_recipients > HARD_MAX_RECIPIENTS {
            return Err(DomainError::InvalidRoutingPolicy(
                "max_recipients is outside the hard safety range",
            ));
        }
        if self.max_chain_wakes == 0 || self.max_chain_wakes > HARD_MAX_CHAIN_WAKES {
            return Err(DomainError::InvalidRoutingPolicy(
                "max_chain_wakes is outside the hard safety range",
            ));
        }
        if self.max_hops == 0 || self.max_hops > HARD_MAX_CHAIN_HOPS {
            return Err(DomainError::InvalidRoutingPolicy(
                "max_hops is outside the hard safety range",
            ));
        }
        if self.max_message_bytes == 0 || self.max_message_bytes > HARD_MAX_MESSAGE_BYTES {
            return Err(DomainError::InvalidRoutingPolicy(
                "max_message_bytes is outside the hard safety range",
            ));
        }
        Ok(())
    }

    /// Canonical content hash persisted with every decision.
    pub fn fingerprint(&self) -> String {
        let mut hasher = Sha256::new();
        hasher.update(b"ortak-routing-policy-v0\0");
        hasher.update((self.version.len() as u64).to_be_bytes());
        hasher.update(self.version.as_bytes());
        hasher.update(self.semantic_threshold.to_bits().to_be_bytes());
        hasher.update((self.max_recipients as u64).to_be_bytes());
        hasher.update([self.max_hops]);
        hasher.update((self.max_chain_wakes as u64).to_be_bytes());
        hasher.update((self.max_message_bytes as u64).to_be_bytes());
        format!("sha256:{}", hex::encode(hasher.finalize()))
    }
}

impl Default for RoutingPolicy {
    fn default() -> Self {
        Self {
            version: "routing-v0".to_owned(),
            semantic_threshold: 0.72,
            max_recipients: 2,
            max_hops: 2,
            max_chain_wakes: 4,
            max_message_bytes: 32 * 1024,
        }
    }
}

/// High-level path used to reach a decision.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RoutingMode {
    /// A hard rule resolved the target set.
    Deterministic,
    /// The injected semantic scorer ranked eligible employees.
    Semantic,
    /// No employee should be woken.
    Silent,
}

/// Final action for a candidate or explicitly targeted employee.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RecipientAction {
    /// Create one idempotent run dispatch.
    Wake,
    /// Do not create work for this employee.
    Drop,
}

/// Stable, audit-friendly explanation for a routing outcome.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RoutingReason {
    /// Envelope identity/content invariants failed before target resolution.
    InvalidMessage,
    /// Direct conversation participant.
    DirectMessage,
    /// Authorized structured dispatch target.
    StructuredDispatch,
    /// Editor/event-level structured mention.
    StructuredMention,
    /// Direct reply to an employee-authored message.
    ReplyToEmployee,
    /// Unique explicit alias in human-authored text.
    ExplicitAlias,
    /// Structured Work assignment target.
    WorkAssignment,
    /// Candidate passed semantic scoring and policy thresholds.
    SemanticMatch,
    /// Event type cannot create employee work.
    NonRoutableMessage,
    /// Message exceeds the bounded routing/scoring input size.
    MessageTooLarge,
    /// The requested target does not exist.
    UnknownTarget,
    /// Employee cannot receive work in its current lifecycle state.
    EmployeeInactive,
    /// Employee has routing disabled in its active revision.
    RoutingDisabled,
    /// Employee authored the input and cannot wake itself.
    SelfOrigin,
    /// Employee was already visited in the delivery chain.
    AlreadyVisited,
    /// Delivery chain reached its explicit hop limit.
    HopLimitReached,
    /// Delivery chain exhausted its total wake budget.
    WakeBudgetExhausted,
    /// Semantic score did not meet the effective threshold.
    BelowSemanticThreshold,
    /// Candidate passed the score floor but exceeded the recipient cap.
    RecipientLimitReached,
    /// Semantic scorer failed or did not return a usable result.
    SemanticScorerUnavailable,
    /// The control-layer semantic scoring deadline elapsed.
    SemanticScorerTimedOut,
    /// The control layer cancelled semantic scoring before completion.
    SemanticScoringCancelled,
    /// The message origin is not allowed to trigger semantic fan-out.
    OriginCannotFanOut,
    /// No eligible employee remained after policy guards.
    NoEligibleEmployee,
    /// No deterministic target or semantic match was found.
    NoRelevantEmployee,
}

/// Stable non-sensitive taxonomy label supplied by a semantic scorer.
#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(try_from = "String", into = "String")]
pub struct EvidenceLabel(String);

impl EvidenceLabel {
    /// Accepts only a compact ASCII code grammar, never arbitrary prose.
    pub fn parse(value: impl AsRef<str>) -> Result<Self, DomainError> {
        let value = value.as_ref();
        let valid = !value.is_empty()
            && value.len() <= 64
            && value
                .chars()
                .next()
                .is_some_and(|character| character.is_ascii_lowercase())
            && value.chars().all(|character| {
                character.is_ascii_lowercase()
                    || character.is_ascii_digit()
                    || matches!(character, '_' | '-' | '.' | ':')
            });

        if valid {
            Ok(Self(value.to_owned()))
        } else {
            Err(DomainError::InvalidSemanticEvidence)
        }
    }

    /// Returns the stable evidence code.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for EvidenceLabel {
    type Error = DomainError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::parse(value)
    }
}

impl From<EvidenceLabel> for String {
    fn from(value: EvidenceLabel) -> Self {
        value.0
    }
}

/// One semantic scorer result before authoritative policy is applied.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SemanticScore {
    /// Candidate employee.
    pub employee_id: EmployeeId,
    /// Relevance score in the inclusive zero-to-one range.
    pub score: f32,
    /// Bounded, non-sensitive evidence labels.
    pub evidence: Vec<EvidenceLabel>,
}

impl SemanticScore {
    /// Validates a scorer result before the router trusts its shape.
    pub fn validate(&self) -> Result<(), DomainError> {
        if !self.score.is_finite() || !(0.0..=1.0).contains(&self.score) {
            return Err(DomainError::InvalidScore {
                field: "semantic_score.score",
            });
        }
        if self.evidence.len() > 8 {
            return Err(DomainError::InvalidSemanticEvidence);
        }
        Ok(())
    }
}

/// Explainable action for one employee in a routing decision.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RecipientDecision {
    /// Employee affected by this row.
    pub employee_id: EmployeeId,
    /// Wake or drop action.
    pub action: RecipientAction,
    /// Authoritative policy explanation.
    pub reason: RoutingReason,
    /// Optional semantic score.
    pub score: Option<f32>,
    /// Bounded, non-sensitive evidence labels.
    pub evidence: Vec<EvidenceLabel>,
}

/// One complete routing outcome for one accepted input message.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RoutingDecision {
    /// Input message identifier.
    pub message_id: String,
    /// Deterministic, semantic, or silent path.
    pub mode: RoutingMode,
    /// High-level reason shown when no recipient detail is needed.
    pub summary_reason: RoutingReason,
    /// Policy version used for this decision.
    pub policy_version: String,
    /// Canonical hash of the complete policy contents used for this decision.
    pub policy_fingerprint: String,
    /// Target and candidate decisions in stable order.
    pub recipients: Vec<RecipientDecision>,
}

impl RoutingDecision {
    /// Iterates through employees selected for a wake dispatch.
    pub fn woken_employee_ids(&self) -> impl Iterator<Item = &EmployeeId> {
        self.recipients.iter().filter_map(|recipient| {
            (recipient.action == RecipientAction::Wake).then_some(&recipient.employee_id)
        })
    }

    /// Returns the number of employee wakes created by this decision.
    pub fn wake_count(&self) -> usize {
        self.woken_employee_ids().count()
    }
}

#[cfg(test)]
mod tests {
    use super::{
        EvidenceLabel, RoutingPolicy, HARD_MAX_CHAIN_HOPS, HARD_MAX_CHAIN_WAKES,
        HARD_MAX_MESSAGE_BYTES, HARD_MAX_RECIPIENTS,
    };

    #[test]
    fn policy_hard_limits_reject_unbounded_configuration() {
        let cases = [
            RoutingPolicy {
                max_recipients: HARD_MAX_RECIPIENTS + 1,
                ..RoutingPolicy::default()
            },
            RoutingPolicy {
                max_hops: HARD_MAX_CHAIN_HOPS + 1,
                ..RoutingPolicy::default()
            },
            RoutingPolicy {
                max_chain_wakes: HARD_MAX_CHAIN_WAKES + 1,
                ..RoutingPolicy::default()
            },
            RoutingPolicy {
                max_message_bytes: HARD_MAX_MESSAGE_BYTES + 1,
                ..RoutingPolicy::default()
            },
        ];

        assert!(cases.into_iter().all(|policy| policy.validate().is_err()));
    }

    #[test]
    fn policy_fingerprint_changes_when_contents_change_under_the_same_version() {
        let baseline = RoutingPolicy::default();
        let changed = RoutingPolicy {
            semantic_threshold: 0.91,
            ..baseline.clone()
        };

        assert_eq!(baseline.version, changed.version);
        assert_ne!(baseline.fingerprint(), changed.fingerprint());
    }

    #[test]
    fn semantic_evidence_accepts_codes_and_rejects_prose_or_controls() {
        assert!(EvidenceLabel::parse("domain:fitness_apps").is_ok());
        assert!(EvidenceLabel::parse("Because this looks relevant").is_err());
        assert!(EvidenceLabel::parse("domain:\u{202e}secret").is_err());
        assert!(EvidenceLabel::parse("domain:fitness\nsecret").is_err());
    }
}
