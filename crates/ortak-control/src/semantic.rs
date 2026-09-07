//! Sealed local authority and cache identity for an out-of-transaction scorer.
//!
//! Only the inbox service constructs this input. Authority fields remain local:
//! an HTTP adapter serializes a bounded, redacted view of [`SemanticScoringInput::request`], not
//! this wrapper, a complete employee manifest, or a database snapshot.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use ortak_domain::{EmployeeId, MessageEnvelope};
use ortak_router::{SemanticCandidate, SemanticRoutingRequest};
use uuid::Uuid;

/// Transient remaining time for one scorer call, never routing authority or cache identity.
#[derive(Clone, Copy, Debug)]
pub struct ScoringBudget {
    deadline: tokio::time::Instant,
}

impl ScoringBudget {
    /// Makes an explicit bounded budget for adapter callers and transport tests.
    /// The production inbox service instead preserves its original shared deadline.
    pub fn for_duration(duration: std::time::Duration) -> Self {
        Self::until(tokio::time::Instant::now() + duration.min(std::time::Duration::from_secs(5)))
    }

    pub(crate) fn until(deadline: tokio::time::Instant) -> Self {
        Self { deadline }
    }

    /// Monotonic deadline shared by admission, network I/O and parsing.
    pub fn deadline(self) -> tokio::time::Instant {
        self.deadline
    }

    /// Time still available; an expired budget remains zero.
    pub fn remaining(self) -> std::time::Duration {
        self.deadline
            .saturating_duration_since(tokio::time::Instant::now())
    }
}

use crate::inbox::{InboxClaim, InboxState};
use crate::ports::RoutingSnapshot;
use crate::routing::CandidateRevision;
use crate::service::routing_input_hash;
use crate::{CompanyScope, ControlError, MessageId, Result};

/// Owned scoring request pinned to one server-resolved company and exact revisions.
///
/// This is evidence for an adapter/cache, not permission to dispatch. The routing
/// repository must still refresh canonical authority before committing a decision.
#[derive(Clone, PartialEq)]
pub struct SemanticScoringInput {
    request: SemanticRoutingRequest,
    company_id: Uuid,
    message_id: MessageId,
    candidates: Vec<CandidateRevision>,
    input_hash: [u8; 32],
}

impl SemanticScoringInput {
    pub(crate) fn new(
        scope: &CompanyScope,
        claim: &InboxClaim,
        snapshot: &RoutingSnapshot,
        envelope: &MessageEnvelope,
        eligible: &BTreeSet<EmployeeId>,
        request: SemanticRoutingRequest,
    ) -> Result<Self> {
        if claim.company_id != scope.company_id()
            || snapshot.office_authority.company_id() != scope.company_id()
            || snapshot.inbox.event.event_id != claim.message_id
            || snapshot.inbox.state != InboxState::Claimed
            || snapshot.inbox.claim_generation != claim.claim_generation
            || request.message_id() != claim.message_id.to_hex()
            || envelope.id != request.message_id()
            || envelope.body != request.body()
            || !envelope.origin.allows_semantic_routing()
            || request.policy_version() != snapshot.policy.version
            || request.policy_fingerprint() != snapshot.policy.fingerprint()
        {
            return Err(ControlError::InvalidProposal(
                "semantic input authority differs",
            ));
        }
        let mut roster = BTreeMap::new();
        for entry in &snapshot.roster {
            if roster.insert(&entry.record.id, entry).is_some() {
                return Err(ControlError::InvalidProposal(
                    "semantic roster contains duplicate identities",
                ));
            }
        }
        let mut candidates = BTreeMap::new();
        for candidate in request.candidates() {
            let employee_id = candidate.employee_id();
            let entry = roster
                .get(employee_id)
                .ok_or(ControlError::InvalidProposal(
                    "semantic candidate has no roster entry",
                ))?;
            let employee = entry
                .employee
                .as_ref()
                .ok_or(ControlError::InvalidProposal(
                    "semantic candidate has no active definition",
                ))?;
            let revision_id =
                entry
                    .record
                    .active_revision_id
                    .ok_or(ControlError::InvalidProposal(
                        "semantic candidate has no active revision",
                    ))?;
            if revision_id.is_nil()
                || !eligible.contains(employee_id)
                || !entry.record.accepts_routing()
                || employee.id != *employee_id
                || SemanticCandidate::from(employee) != *candidate
                || candidates
                    .insert(employee_id.clone(), revision_id)
                    .is_some()
            {
                return Err(ControlError::InvalidProposal(
                    "semantic candidate revision pins differ",
                ));
            }
        }
        if candidates.is_empty() || candidates.len() != request.candidates().len() {
            return Err(ControlError::InvalidProposal(
                "semantic candidate set is incomplete",
            ));
        }
        let candidates: Vec<_> = candidates
            .into_iter()
            .map(|(employee_id, revision_id)| CandidateRevision {
                employee_id,
                revision_id,
            })
            .collect();
        let input_hash = routing_input_hash(envelope, &candidates, eligible, &snapshot.policy);
        Ok(Self {
            request,
            company_id: scope.company_id(),
            message_id: claim.message_id,
            candidates,
            input_hash,
        })
    }

    /// Least-privilege routing fields; adapters must redact and bound them before egress.
    pub fn request(&self) -> &SemanticRoutingRequest {
        &self.request
    }

    /// Company resolved by the control layer; never selected by message content.
    pub fn company_id(&self) -> Uuid {
        self.company_id
    }

    /// Accepted Office message whose request this scores.
    pub fn message_id(&self) -> MessageId {
        self.message_id
    }

    /// Exact candidate/revision set, sorted by stable employee identity.
    pub fn candidates(&self) -> &[CandidateRevision] {
        &self.candidates
    }

    /// Human-readable company policy version, paired with its content fingerprint.
    pub fn policy_version(&self) -> &str {
        self.request.policy_version()
    }

    /// Canonical fingerprint of the complete company policy.
    pub fn policy_fingerprint(&self) -> &str {
        self.request.policy_fingerprint()
    }

    /// Canonical input hash including source, eligibility, revisions and policy.
    /// Company and adapter/deployment versions must additionally enter cache keys.
    pub fn input_hash(&self) -> [u8; 32] {
        self.input_hash
    }
}

impl fmt::Debug for SemanticScoringInput {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SemanticScoringInput")
            .field("company_id", &self.company_id)
            .field("message_id", &self.message_id)
            .field("candidate_count", &self.candidates.len())
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
pub(crate) mod tests;
