//! Routing proposals, in-transaction revalidation, and commit outcomes.
//!
//! The pure functions in this module are the single guard seam used inside
//! the authoritative PostgreSQL transaction. They are exercised directly by
//! unit tests and indirectly by the Postgres integration tests.

use std::collections::{BTreeMap, BTreeSet};

use chrono::{DateTime, Utc};
use ortak_domain::{
    DeliveryChain, EmployeeId, EmployeeStatus, MessageOrigin, RecipientAction, RecipientDecision,
    RoutingDecision, RoutingMode, RoutingPolicy, RoutingReason,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::{ControlError, Result};
use crate::ids::{ClaimGeneration, MessageId};
use crate::inbox::InboxState;
use crate::outbox::DispatchTicket;

/// One candidate employee and the immutable revision it was evaluated with.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CandidateRevision {
    /// Candidate employee.
    pub employee_id: EmployeeId,
    /// Active revision id at snapshot time.
    pub revision_id: Uuid,
}

/// Which roster the proposal's candidate set describes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RosterScope {
    /// Only the explicitly targeted employees were evaluated.
    Targets,
    /// Every eligible employee was scored; a newly eligible employee changes the input.
    EligibleRoster,
}

/// Pinned semantic scorer provenance persisted with a decision.
#[derive(Clone, Debug, PartialEq)]
pub struct ScorerMetadata {
    /// Scorer adapter name.
    pub adapter: String,
    /// Model reference, when applicable.
    pub model: Option<String>,
    /// Prompt version, when applicable.
    pub prompt_version: Option<String>,
    /// Scorer version.
    pub version: String,
    /// Remote latency in milliseconds.
    pub latency_ms: Option<i32>,
    /// Bounded, secret-free usage metadata.
    pub usage: Option<serde_json::Value>,
}

/// A requested target that resolved to no employee row.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ExcludedTarget {
    /// Requested target as written in the decision.
    pub target: String,
    /// Why it was excluded.
    pub reason: RoutingReason,
}

/// Out-of-transaction routing result submitted for authoritative commit.
///
/// The proposal is bound to the company of the inbox claim it was prepared
/// from; the commit rejects it under any other [`crate::CompanyScope`].
#[derive(Clone, Debug, PartialEq)]
pub struct RoutingProposal {
    /// Company whose inbox claim this proposal decides.
    pub company_id: Uuid,
    /// Message being decided.
    pub message_id: MessageId,
    /// Root of the delivery chain the message belongs to.
    pub root_message_id: MessageId,
    /// Inbox claim generation the worker holds.
    pub claim_generation: ClaimGeneration,
    /// Authenticated message origin.
    pub origin: MessageOrigin,
    /// SHA-256 of the bounded router/scorer input.
    pub input_hash: [u8; 32],
    /// Candidate employees and the revisions they were evaluated with.
    pub candidates: Vec<CandidateRevision>,
    /// Which roster the candidate set describes.
    pub roster_scope: RosterScope,
    /// Pure router decision computed outside the transaction.
    pub decision: RoutingDecision,
    /// Scorer provenance when semantic scoring ran.
    pub scorer: Option<ScorerMetadata>,
}

/// Authoritative delivery-chain state as read under the root row lock.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChainState {
    /// Root message.
    pub root_message_id: MessageId,
    /// Policy version pinned when the root was first locked.
    pub policy_version: String,
    /// Policy fingerprint pinned when the root was first locked.
    pub policy_fingerprint: String,
    /// Pinned hop ceiling.
    pub max_hops: u8,
    /// Pinned wake ceiling.
    pub max_wakes: usize,
    /// Committed dispatch batches that reserved at least one wake.
    pub hop_count: u8,
    /// Reserved employee visits.
    pub wake_count: usize,
    /// Employees holding a visit reservation.
    pub visited: BTreeSet<EmployeeId>,
}

impl ChainState {
    /// Describes a root that has no durable row yet, pinned to the current policy.
    pub fn fresh(root_message_id: MessageId, policy: &RoutingPolicy) -> Self {
        Self {
            root_message_id,
            policy_version: policy.version.clone(),
            policy_fingerprint: policy.fingerprint(),
            max_hops: policy.max_hops,
            max_wakes: policy.max_chain_wakes,
            hop_count: 0,
            wake_count: 0,
            visited: BTreeSet::new(),
        }
    }

    /// Materializes the pure snapshot value for early rejection by the router.
    ///
    /// The snapshot is defense in depth only; the durable row remains the
    /// authority for spending budgets.
    pub fn snapshot(&self) -> Result<DeliveryChain> {
        let root_hex = self.root_message_id.to_hex();
        let hops = usize::from(self.hop_count);
        let visited = self.visited.iter().collect::<Vec<_>>();
        if hops == 0 {
            if !visited.is_empty() {
                return Err(ControlError::InvalidData(format!(
                    "delivery chain {root_hex} has visits but no committed hop"
                )));
            }
            return Ok(DeliveryChain::root(root_hex));
        }
        if visited.len() < hops || visited.len() != self.wake_count {
            return Err(ControlError::InvalidData(format!(
                "delivery chain {root_hex} counters disagree with its visits"
            )));
        }

        let mut chain = DeliveryChain::root(root_hex);
        for employee_id in &visited[..hops - 1] {
            chain = chain.advance_for_dispatch([*employee_id])?;
        }
        chain = chain.advance_for_dispatch(visited[hops - 1..].iter().copied())?;
        Ok(chain)
    }
}

/// Lifecycle facts about one employee as read inside the transaction.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EmployeeRecord {
    /// Employee id.
    pub id: EmployeeId,
    /// Durable lifecycle status column.
    pub status: EmployeeStatus,
    /// Revision currently in effect.
    pub active_revision_id: Option<Uuid>,
    /// Routing participation from the active revision; false without one.
    pub routing_enabled: bool,
}

impl EmployeeRecord {
    /// Returns whether the employee may receive work before per-message guards.
    pub fn accepts_routing(&self) -> bool {
        self.status == EmployeeStatus::Active && self.routing_enabled
    }
}

/// Why the refreshed inputs invalidated the proposal.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RevalidationFailure {
    /// The company policy version or canonical fingerprint changed.
    PolicyChanged {
        /// Version/fingerprint pinned by the proposal.
        expected: (String, String),
        /// Version/fingerprint currently in effect.
        observed: (String, String),
    },
    /// A candidate's active revision changed after it was evaluated.
    CandidateRevisionChanged {
        /// Affected employee.
        employee_id: EmployeeId,
    },
    /// An employee not in the scored candidate set became eligible.
    RosterChanged {
        /// Newly eligible employee.
        employee_id: EmployeeId,
    },
}

/// Chain counters after a commit, for explanation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ChainCounters {
    /// Committed hops.
    pub hop_count: u8,
    /// Reserved wakes.
    pub wake_count: usize,
    /// Pinned hop ceiling.
    pub max_hops: u8,
    /// Pinned wake ceiling.
    pub max_wakes: usize,
}

/// A committed routing decision and the dispatch work it created.
#[derive(Clone, Debug, PartialEq)]
pub struct CommittedDecision {
    /// Durable decision id.
    pub decision_id: Uuid,
    /// Final mode after refreshed guards.
    pub mode: RoutingMode,
    /// Final summary reason after refreshed guards.
    pub summary_reason: RoutingReason,
    /// Persisted recipient rows in stable order.
    pub recipients: Vec<RecipientDecision>,
    /// Targets that resolved to no employee row.
    pub excluded_targets: Vec<ExcludedTarget>,
    /// Employees newly reserved and woken by this commit.
    pub wake_count: usize,
    /// Whether the batch consumed a hop.
    pub hop_consumed: bool,
    /// Chain counters after this commit.
    pub chain: ChainCounters,
    /// Run-dispatch outbox rows written in the same commit.
    pub dispatches: Vec<DispatchTicket>,
    /// True when refreshed chain state dropped at least one proposed wake.
    pub refreshed: bool,
}

/// Result of the authoritative routing transaction.
#[derive(Clone, Debug, PartialEq)]
pub enum RoutingCommitOutcome {
    /// The decision, reservations, counters, and outbox rows were committed.
    Committed(CommittedDecision),
    /// The message already has its one dispatching decision; nothing was written.
    AlreadyDecided {
        /// Existing decision id.
        decision_id: Uuid,
    },
    /// The inbox claim generation no longer belongs to this worker; nothing was written.
    StaleClaim {
        /// Inbox state observed under lock.
        observed_state: InboxState,
        /// Claim generation observed under lock.
        observed_generation: ClaimGeneration,
    },
    /// An input that affected scoring changed; the transaction rolled back.
    InputsChanged(RevalidationFailure),
}

/// Durable decision as read back for audit and revalidation.
#[derive(Clone, Debug, PartialEq)]
pub struct StoredDecision {
    /// Decision id.
    pub id: Uuid,
    /// Decided message.
    pub message_id: MessageId,
    /// Chain root.
    pub root_message_id: MessageId,
    /// Inbox claim generation fenced by the commit.
    pub inbox_claim_generation: ClaimGeneration,
    /// Path used.
    pub mode: RoutingMode,
    /// Summary reason.
    pub summary_reason: RoutingReason,
    /// Policy version pinned.
    pub policy_version: String,
    /// Policy fingerprint pinned.
    pub policy_fingerprint: String,
    /// Bounded input hash pinned.
    pub input_hash: [u8; 32],
    /// Candidate revisions pinned, in stable order.
    pub candidate_revision_ids: Vec<Uuid>,
    /// Employees newly woken.
    pub wake_count: usize,
    /// Whether a hop was consumed.
    pub hop_consumed: bool,
    /// Scorer adapter, when semantic scoring ran.
    pub scorer_adapter: Option<String>,
    /// Recipient rows in stable order.
    pub recipients: Vec<RecipientDecision>,
    /// Commit time.
    pub decided_at: DateTime<Utc>,
}

/// Recipient row after refreshed guards, with the revision it pins.
#[derive(Clone, Debug, PartialEq)]
pub struct GuardedRecipient {
    /// Final recipient decision.
    pub decision: RecipientDecision,
    /// Active revision at commit time; `None` only when the employee had none.
    pub revision_id: Option<Uuid>,
}

/// Decision after guards were reapplied against refreshed authoritative state.
#[derive(Clone, Debug, PartialEq)]
pub struct GuardedDecision {
    /// Final mode.
    pub mode: RoutingMode,
    /// Final summary reason.
    pub summary_reason: RoutingReason,
    /// Recipient rows that reference real employees, in stable order.
    pub recipients: Vec<GuardedRecipient>,
    /// Requested targets with no employee row.
    pub excluded_targets: Vec<ExcludedTarget>,
    /// Employees newly reserved by this decision.
    pub wake_count: usize,
    /// True when at least one proposed wake was dropped by refreshed state.
    pub refreshed: bool,
}

impl GuardedDecision {
    /// Iterates through employees that will be reserved and woken.
    pub fn woken_employee_ids(&self) -> impl Iterator<Item = &EmployeeId> {
        self.recipients.iter().filter_map(|recipient| {
            (recipient.decision.action == RecipientAction::Wake)
                .then_some(&recipient.decision.employee_id)
        })
    }
}

/// Checks whether inputs that affected scoring changed since the snapshot.
///
/// Chain counters and visit reservations are deliberately not inputs here:
/// they are reapplied by [`reapply_guards`] and may drop recipients without
/// forcing a re-score.
pub fn revalidate_inputs(
    proposal: &RoutingProposal,
    policy: &RoutingPolicy,
    employees: &BTreeMap<EmployeeId, EmployeeRecord>,
    chain: &ChainState,
) -> Option<RevalidationFailure> {
    let observed = (policy.version.clone(), policy.fingerprint());
    let expected = (
        proposal.decision.policy_version.clone(),
        proposal.decision.policy_fingerprint.clone(),
    );
    if observed != expected {
        return Some(RevalidationFailure::PolicyChanged { expected, observed });
    }

    for candidate in &proposal.candidates {
        let current = employees
            .get(&candidate.employee_id)
            .and_then(|record| record.active_revision_id);
        if current != Some(candidate.revision_id) {
            return Some(RevalidationFailure::CandidateRevisionChanged {
                employee_id: candidate.employee_id.clone(),
            });
        }
    }

    if proposal.roster_scope == RosterScope::EligibleRoster {
        let scored = proposal
            .candidates
            .iter()
            .map(|candidate| &candidate.employee_id)
            .collect::<BTreeSet<_>>();
        let self_origin = proposal.origin.employee_id();
        let newly_eligible = employees.values().find(|record| {
            record.accepts_routing()
                && self_origin != Some(&record.id)
                && !chain.visited.contains(&record.id)
                && !scored.contains(&record.id)
        });
        if let Some(record) = newly_eligible {
            return Some(RevalidationFailure::RosterChanged {
                employee_id: record.id.clone(),
            });
        }
    }

    None
}

/// Reapplies eligibility, visited, hop, wake, and recipient-cap guards against
/// the refreshed chain row and employee state.
///
/// The pure decision is evidence: a proposed wake survives only if the locked
/// chain still has a hop and enough wake budget and the employee is still
/// eligible and unvisited. Proposed drops are preserved verbatim.
pub fn reapply_guards(
    decision: &RoutingDecision,
    origin: &MessageOrigin,
    chain: &ChainState,
    max_recipients: usize,
    employees: &BTreeMap<EmployeeId, EmployeeRecord>,
) -> GuardedDecision {
    let mut recipients = Vec::with_capacity(decision.recipients.len());
    let mut excluded_targets = Vec::new();
    let mut wakes = 0usize;
    let mut first_refreshed_reason = None;
    let mut seen = BTreeSet::new();

    for recipient in &decision.recipients {
        if !seen.insert(recipient.employee_id.clone()) {
            continue;
        }
        let Some(record) = employees.get(&recipient.employee_id) else {
            excluded_targets.push(ExcludedTarget {
                target: recipient.employee_id.to_string(),
                reason: RoutingReason::UnknownTarget,
            });
            continue;
        };

        if recipient.action == RecipientAction::Drop {
            recipients.push(GuardedRecipient {
                decision: recipient.clone(),
                revision_id: record.active_revision_id,
            });
            continue;
        }

        let refreshed_reason = if origin.employee_id() == Some(&record.id) {
            Some(RoutingReason::SelfOrigin)
        } else if chain.visited.contains(&record.id) {
            Some(RoutingReason::AlreadyVisited)
        } else if record.status != EmployeeStatus::Active {
            Some(RoutingReason::EmployeeInactive)
        } else if !record.routing_enabled {
            Some(RoutingReason::RoutingDisabled)
        } else if chain.hop_count >= chain.max_hops {
            Some(RoutingReason::HopLimitReached)
        } else if chain.wake_count + wakes >= chain.max_wakes {
            Some(RoutingReason::WakeBudgetExhausted)
        } else if wakes >= max_recipients {
            Some(RoutingReason::RecipientLimitReached)
        } else {
            None
        };

        let final_decision = match refreshed_reason {
            Some(reason) => {
                first_refreshed_reason.get_or_insert(reason);
                RecipientDecision {
                    employee_id: recipient.employee_id.clone(),
                    action: RecipientAction::Drop,
                    reason,
                    score: recipient.score,
                    evidence: recipient.evidence.clone(),
                }
            }
            None => {
                wakes += 1;
                recipient.clone()
            }
        };
        recipients.push(GuardedRecipient {
            decision: final_decision,
            revision_id: record.active_revision_id,
        });
    }

    let (mode, summary_reason) = if wakes > 0 {
        let mode = if decision.mode == RoutingMode::Silent {
            RoutingMode::Deterministic
        } else {
            decision.mode
        };
        (mode, decision.summary_reason)
    } else {
        (
            RoutingMode::Silent,
            first_refreshed_reason.unwrap_or(decision.summary_reason),
        )
    };

    GuardedDecision {
        mode,
        summary_reason,
        recipients,
        excluded_targets,
        wake_count: wakes,
        refreshed: first_refreshed_reason.is_some(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn employee(id: &str) -> EmployeeId {
        EmployeeId::parse(id).expect("valid test employee id")
    }

    fn record(id: &str, revision: Uuid) -> EmployeeRecord {
        EmployeeRecord {
            id: employee(id),
            status: EmployeeStatus::Active,
            active_revision_id: Some(revision),
            routing_enabled: true,
        }
    }

    fn wake(id: &str) -> RecipientDecision {
        RecipientDecision {
            employee_id: employee(id),
            action: RecipientAction::Wake,
            reason: RoutingReason::StructuredDispatch,
            score: None,
            evidence: Vec::new(),
        }
    }

    fn decision(recipients: Vec<RecipientDecision>) -> RoutingDecision {
        let policy = RoutingPolicy::default();
        RoutingDecision {
            message_id: "m".repeat(64),
            mode: RoutingMode::Deterministic,
            summary_reason: RoutingReason::StructuredDispatch,
            policy_version: policy.version.clone(),
            policy_fingerprint: policy.fingerprint(),
            recipients,
        }
    }

    fn chain(hop_count: u8, visited: &[&str]) -> ChainState {
        let mut state =
            ChainState::fresh(MessageId::from_bytes([7; 32]), &RoutingPolicy::default());
        state.hop_count = hop_count;
        state.visited = visited.iter().map(|id| employee(id)).collect();
        state.wake_count = state.visited.len();
        state
    }

    #[test]
    fn exhausted_hop_budget_drops_every_proposed_wake_with_an_explanation() {
        let revision = Uuid::new_v4();
        let employees = [record("cem", revision), record("zeynep", revision)]
            .into_iter()
            .map(|record| (record.id.clone(), record))
            .collect();
        let guarded = reapply_guards(
            &decision(vec![wake("zeynep")]),
            &MessageOrigin::Employee(employee("cem")),
            &chain(2, &["cem"]),
            2,
            &employees,
        );

        assert_eq!(guarded.wake_count, 0);
        assert_eq!(guarded.mode, RoutingMode::Silent);
        assert_eq!(guarded.summary_reason, RoutingReason::HopLimitReached);
        assert!(guarded.refreshed);
        assert_eq!(
            guarded.recipients[0].decision.reason,
            RoutingReason::HopLimitReached
        );
    }

    #[test]
    fn visited_employees_and_unknown_targets_are_dropped_or_excluded() {
        let revision = Uuid::new_v4();
        let employees = [record("cem", revision), record("zeynep", revision)]
            .into_iter()
            .map(|record| (record.id.clone(), record))
            .collect();
        let guarded = reapply_guards(
            &decision(vec![wake("cem"), wake("zeynep"), wake("ghost")]),
            &MessageOrigin::Human("sefa".to_owned()),
            &chain(1, &["cem"]),
            2,
            &employees,
        );

        assert_eq!(guarded.wake_count, 1);
        assert_eq!(guarded.mode, RoutingMode::Deterministic);
        assert_eq!(
            guarded.recipients[0].decision.reason,
            RoutingReason::AlreadyVisited
        );
        assert_eq!(guarded.recipients[1].decision.action, RecipientAction::Wake);
        assert_eq!(guarded.excluded_targets.len(), 1);
        assert_eq!(guarded.excluded_targets[0].target, "ghost");
    }

    #[test]
    fn wake_budget_is_applied_against_the_durable_chain_counters() {
        let revision = Uuid::new_v4();
        let employees = [
            record("cem", revision),
            record("zeynep", revision),
            record("ada", revision),
        ]
        .into_iter()
        .map(|record| (record.id.clone(), record))
        .collect();
        let mut state = chain(1, &["cem"]);
        state.max_wakes = 2;
        let guarded = reapply_guards(
            &decision(vec![wake("zeynep"), wake("ada")]),
            &MessageOrigin::Human("sefa".to_owned()),
            &state,
            16,
            &employees,
        );

        assert_eq!(guarded.wake_count, 1);
        assert_eq!(
            guarded.recipients[1].decision.reason,
            RoutingReason::WakeBudgetExhausted
        );
    }

    #[test]
    fn changed_policy_or_revision_or_roster_invalidates_the_proposal() {
        let revision = Uuid::new_v4();
        let policy = RoutingPolicy::default();
        let mut employees: BTreeMap<EmployeeId, EmployeeRecord> = [record("cem", revision)]
            .into_iter()
            .map(|record| (record.id.clone(), record))
            .collect();
        let state = ChainState::fresh(MessageId::from_bytes([1; 32]), &policy);
        let proposal = RoutingProposal {
            company_id: Uuid::new_v4(),
            message_id: MessageId::from_bytes([1; 32]),
            root_message_id: MessageId::from_bytes([1; 32]),
            claim_generation: ClaimGeneration(1),
            origin: MessageOrigin::Human("sefa".to_owned()),
            input_hash: [0; 32],
            candidates: vec![CandidateRevision {
                employee_id: employee("cem"),
                revision_id: revision,
            }],
            roster_scope: RosterScope::EligibleRoster,
            decision: decision(vec![wake("cem")]),
            scorer: None,
        };

        assert_eq!(
            revalidate_inputs(&proposal, &policy, &employees, &state),
            None
        );

        let changed_policy = RoutingPolicy {
            semantic_threshold: 0.9,
            ..policy.clone()
        };
        assert!(matches!(
            revalidate_inputs(&proposal, &changed_policy, &employees, &state),
            Some(RevalidationFailure::PolicyChanged { .. })
        ));

        employees.insert(employee("zeynep"), record("zeynep", revision));
        assert!(matches!(
            revalidate_inputs(&proposal, &policy, &employees, &state),
            Some(RevalidationFailure::RosterChanged { .. })
        ));

        employees.remove(&employee("zeynep"));
        employees.insert(employee("cem"), record("cem", Uuid::new_v4()));
        assert!(matches!(
            revalidate_inputs(&proposal, &policy, &employees, &state),
            Some(RevalidationFailure::CandidateRevisionChanged { .. })
        ));
    }

    #[test]
    fn chain_snapshot_materializes_hops_and_visits() {
        let state = chain(2, &["cem", "zeynep", "ada"]);
        let snapshot = state.snapshot().expect("consistent chain");
        assert_eq!(snapshot.hop_count(), 2);
        assert_eq!(snapshot.wake_count(), 3);
        assert!(snapshot.has_visited(&employee("zeynep")));

        let mut broken = chain(1, &[]);
        broken.wake_count = 1;
        assert!(broken.snapshot().is_err());
    }
}
