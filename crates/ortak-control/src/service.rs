//! Inbox routing application service.
//!
//! The service owns the two-phase flow from Architecture v0 §4.2: claim and
//! snapshot, optional semantic scoring outside any transaction, then the
//! authoritative commit with bounded re-score attempts.

use std::collections::BTreeSet;
use std::time::Duration;

use chrono::Utc;
use ortak_domain::{
    ConversationContext, EmployeeId, MessageEnvelope, MessageOrigin, RoutingDecision, RoutingMode,
    RoutingPolicy, RoutingReason,
};
use ortak_router::{Router, RoutingPreparation, SemanticScoringFailure};
use sha2::{Digest, Sha256};
use tokio::time::{timeout_at, Instant};
use uuid::Uuid;

use crate::error::{ControlError, Result};
use crate::ids::{CompanyScope, MessageId};
use crate::inbox::{InboxClaim, InboxState};
use crate::ports::{
    InboxRepository, MessageNormalizer, Normalization, NormalizationRefusal, NormalizedMessage,
    RoutingRepository, RoutingSnapshot, ScoringOutcome, SemanticScorer,
};
use crate::routing::{
    CandidateRevision, ChainState, CommittedDecision, RosterScope, RoutingCommitOutcome,
    RoutingProposal,
};

use crate::semantic::SemanticScoringInput;

/// Worker tuning for the inbox routing service.
#[derive(Clone, Debug)]
pub struct RoutingWorkerConfig {
    /// Identity recorded on inbox claims.
    pub worker_id: String,
    /// Inbox claim lease; a slower scorer loses its claim to a reclaiming worker.
    pub claim_lease: Duration,
    /// Total scoring time across refresh attempts; clamped to 1 ms..=5 s and
    /// capped by the claim lease. Time spent refreshing after the first score
    /// consumes the same budget. No new scorer call begins after it expires.
    pub semantic_timeout: Duration,
    /// Claims allowed before an inbox row becomes terminal `failed`.
    pub max_claim_attempts: i32,
    /// Refresh/re-score attempts before an explainable silent decision.
    pub max_revalidation_attempts: u32,
    /// Delay before a released claim becomes due again.
    pub retry_backoff: Duration,
}

impl Default for RoutingWorkerConfig {
    fn default() -> Self {
        Self {
            worker_id: format!("router-{}", Uuid::new_v4()),
            claim_lease: Duration::from_secs(60),
            semantic_timeout: Duration::from_secs(5),
            max_claim_attempts: 5,
            max_revalidation_attempts: 3,
            retry_backoff: Duration::from_secs(30),
        }
    }
}

/// Outcome of routing one claimed inbox row.
#[derive(Clone, Debug, PartialEq)]
pub enum ServiceOutcome {
    /// A decision was committed, together with any dispatch rows it
    /// produced; a refusal or silent decision produces none.
    Committed(CommittedDecision),
    /// The message already had its dispatching decision.
    AlreadyDecided {
        /// Existing decision id.
        decision_id: Uuid,
    },
    /// Another worker reclaimed the row; nothing was written.
    StaleClaim,
    /// The event is not Office message input and was finalized as `dropped`
    /// without a decision. Explicit normalization refusals never take this
    /// path: they commit as a silent [`ServiceOutcome::Committed`] decision.
    Dropped,
}

/// Claims inbox rows and drives them to a committed decision.
#[derive(Clone, Debug)]
pub struct InboxRoutingService<R, N, S> {
    repository: R,
    normalizer: N,
    scorer: S,
    config: RoutingWorkerConfig,
}

impl<R, N, S> InboxRoutingService<R, N, S>
where
    R: InboxRepository + RoutingRepository,
    N: MessageNormalizer,
    S: SemanticScorer,
{
    /// Builds a service over the given adapters.
    pub fn new(repository: R, normalizer: N, scorer: S, config: RoutingWorkerConfig) -> Self {
        Self {
            repository,
            normalizer,
            scorer,
            config,
        }
    }

    /// Returns the underlying repository.
    pub fn repository(&self) -> &R {
        &self.repository
    }

    /// Claims the next due inbox row and routes it; `None` when nothing is due.
    pub async fn claim_and_route(&self, scope: &CompanyScope) -> Result<Option<ServiceOutcome>> {
        let Some(claim) = self
            .repository
            .claim_next(
                scope,
                &self.config.worker_id,
                self.config.claim_lease,
                self.config.max_claim_attempts,
            )
            .await?
        else {
            return Ok(None);
        };
        self.route_claim(scope, &claim).await.map(Some)
    }

    /// Routes one held claim. On error the claim is released with a durable retry record.
    ///
    /// A claim taken from another company's inbox is rejected before any
    /// normalization, scoring, or write, and without touching the scoped
    /// inbox row that may share its message id and claim generation.
    pub async fn route_claim(
        &self,
        scope: &CompanyScope,
        claim: &InboxClaim,
    ) -> Result<ServiceOutcome> {
        if claim.company_id != scope.company_id() {
            return Err(ControlError::InvalidProposal(
                "inbox claim company does not match the company scope",
            ));
        }
        match self.route_claim_inner(scope, claim).await {
            Ok(outcome) => Ok(outcome),
            Err(error) => {
                let retry_after = Utc::now()
                    + chrono::Duration::from_std(self.config.retry_backoff)
                        .unwrap_or_else(|_| chrono::Duration::seconds(30));
                if let Err(release_error) = self
                    .repository
                    .release_for_retry(
                        scope,
                        claim.message_id,
                        claim.claim_generation,
                        &error.to_string(),
                        retry_after,
                        self.config.max_claim_attempts,
                    )
                    .await
                {
                    tracing::error!(
                        message_id = %claim.message_id,
                        error = %release_error,
                        "failed to release inbox claim after routing error"
                    );
                }
                Err(error)
            }
        }
    }

    async fn route_claim_inner(
        &self,
        scope: &CompanyScope,
        claim: &InboxClaim,
    ) -> Result<ServiceOutcome> {
        let mut attempts = 0u32;
        let mut scoring_deadline = None::<Instant>;
        while attempts < self.config.max_revalidation_attempts.max(1) {
            attempts += 1;
            let snapshot = self.snapshot_for_claim(scope, claim).await?;
            let Some(snapshot) = snapshot else {
                return Ok(ServiceOutcome::StaleClaim);
            };

            let normalized = match self.normalizer.normalize(scope, &snapshot.inbox).await? {
                Normalization::Message(normalized) => *normalized,
                Normalization::Refused(refusal) => {
                    // No scorer call and no dispatch: the refusal is the
                    // one durable decision for this message.
                    return self.commit_refusal(scope, claim, &snapshot, refusal).await;
                }
                Normalization::NotOfficeInput => {
                    let finalized = self
                        .repository
                        .finalize_dropped(
                            scope,
                            claim.message_id,
                            claim.claim_generation,
                            "unroutable_event",
                        )
                        .await?;
                    return Ok(if finalized {
                        ServiceOutcome::Dropped
                    } else {
                        ServiceOutcome::StaleClaim
                    });
                }
            };

            let root_message_id = normalized.root_message_id;
            let chain = self
                .repository
                .chain_state(scope, root_message_id)
                .await?
                .unwrap_or_else(|| ChainState::fresh(root_message_id, &snapshot.policy));
            let envelope = normalized.envelope.with_delivery_chain(chain.snapshot()?);
            if envelope.id != claim.message_id.to_hex() {
                return Err(ControlError::InvalidProposal(
                    "normalized envelope id does not match the claimed message",
                ));
            }

            // The full company catalog resolves identity, so an alias
            // collision anywhere in the roster still fails closed here; the
            // router applies conversation eligibility before it spends any
            // recipient or chain-wake budget.
            let catalog = snapshot.catalog()?;
            let router = Router::new(snapshot.policy.clone())?;
            let eligible = &normalized.eligible_employee_ids;
            let (decision, scorer, candidates, roster_scope) =
                match router.prepare_with_conversation_eligibility(&envelope, &catalog, eligible) {
                    RoutingPreparation::Final(decision) => {
                        let candidates = decision
                            .recipients
                            .iter()
                            .filter_map(|recipient| {
                                snapshot.active_revision(&recipient.employee_id).map(
                                    |revision_id| CandidateRevision {
                                        employee_id: recipient.employee_id.clone(),
                                        revision_id,
                                    },
                                )
                            })
                            .collect::<Vec<_>>();
                        (decision, None, candidates, RosterScope::Targets)
                    }
                    RoutingPreparation::Semantic(request) => {
                        let input = SemanticScoringInput::new(
                            scope,
                            claim,
                            &snapshot,
                            &envelope,
                            eligible,
                            request.clone(),
                        )?;
                        let candidates = input.candidates().to_vec();
                        let now = Instant::now();
                        let lease_remaining = remaining_claim_time(claim, &snapshot);
                        let budget = self
                            .config
                            .semantic_timeout
                            .clamp(Duration::from_millis(1), Duration::from_secs(5))
                            .min(self.config.claim_lease)
                            .min(lease_remaining);
                        let deadline = scoring_deadline.get_or_insert(now + budget);
                        // A refreshed lease may shorten the budget, never renew it.
                        *deadline = (*deadline).min(now + lease_remaining);
                        // Remote scoring runs here, with no database transaction open.
                        // timeout_at drops the owned future; no detached late result can
                        // enter completion, persistence, or a subsequent attempt.
                        let outcome = score_before_deadline(&self.scorer, &input, *deadline).await;
                        let decision = router.complete_semantic(request, outcome.result);
                        (
                            decision,
                            Some(outcome.metadata),
                            candidates,
                            RosterScope::EligibleRoster,
                        )
                    }
                };

            let proposal = RoutingProposal {
                office_authority: snapshot.office_authority.clone(),
                office_input_hash: office_input_hash(&envelope, root_message_id, eligible),
                company_id: claim.company_id,
                message_id: claim.message_id,
                root_message_id,
                claim_generation: claim.claim_generation,
                origin: envelope.origin.clone(),
                input_hash: routing_input_hash(&envelope, &candidates, eligible, &snapshot.policy),
                candidates,
                roster_scope,
                eligible_employee_ids: eligible.clone(),
                decision,
                scorer,
            };

            match self.repository.commit_routing(scope, &proposal).await? {
                RoutingCommitOutcome::Committed(committed) => {
                    return Ok(ServiceOutcome::Committed(committed));
                }
                RoutingCommitOutcome::AlreadyDecided { decision_id } => {
                    return Ok(ServiceOutcome::AlreadyDecided { decision_id });
                }
                RoutingCommitOutcome::StaleClaim { .. } => {
                    return Ok(ServiceOutcome::StaleClaim);
                }
                RoutingCommitOutcome::InputsChanged(failure) => {
                    tracing::info!(
                        message_id = %claim.message_id,
                        attempt = attempts,
                        ?failure,
                        "routing inputs changed after scoring; refreshing"
                    );
                }
            }
        }

        self.commit_exhausted(scope, claim, attempts).await
    }

    async fn snapshot_for_claim(
        &self,
        scope: &CompanyScope,
        claim: &InboxClaim,
    ) -> Result<Option<RoutingSnapshot>> {
        let Some(snapshot) = self
            .repository
            .routing_snapshot(scope, claim.message_id)
            .await?
        else {
            return Err(ControlError::InvalidData(format!(
                "inbox row {} vanished while claimed",
                claim.message_id
            )));
        };
        if snapshot.inbox.state != InboxState::Claimed
            || snapshot.inbox.claim_generation != claim.claim_generation
        {
            return Ok(None);
        }
        Ok(Some(snapshot))
    }

    /// Records an explainable silent decision once bounded refresh attempts run out.
    async fn commit_exhausted(
        &self,
        scope: &CompanyScope,
        claim: &InboxClaim,
        attempts: u32,
    ) -> Result<ServiceOutcome> {
        let Some(snapshot) = self.snapshot_for_claim(scope, claim).await? else {
            return Ok(ServiceOutcome::StaleClaim);
        };
        let normalized: NormalizedMessage =
            match self.normalizer.normalize(scope, &snapshot.inbox).await? {
                Normalization::Message(normalized) => *normalized,
                Normalization::Refused(refusal) => {
                    return self.commit_refusal(scope, claim, &snapshot, refusal).await;
                }
                Normalization::NotOfficeInput => return Ok(ServiceOutcome::StaleClaim),
            };
        let proposal = RoutingProposal {
            office_authority: snapshot.office_authority.clone(),
            office_input_hash: office_input_hash(
                &normalized.envelope,
                normalized.root_message_id,
                &normalized.eligible_employee_ids,
            ),
            company_id: claim.company_id,
            message_id: claim.message_id,
            root_message_id: normalized.root_message_id,
            claim_generation: claim.claim_generation,
            origin: normalized.envelope.origin.clone(),
            input_hash: routing_input_hash(
                &normalized.envelope,
                &[],
                &normalized.eligible_employee_ids,
                &snapshot.policy,
            ),
            candidates: Vec::new(),
            roster_scope: RosterScope::Targets,
            eligible_employee_ids: normalized.eligible_employee_ids,
            decision: silent_decision(
                claim.message_id,
                RoutingReason::RevalidationExhausted,
                &snapshot.policy,
            ),
            scorer: None,
        };
        match self.repository.commit_routing(scope, &proposal).await? {
            RoutingCommitOutcome::Committed(committed) => Ok(ServiceOutcome::Committed(committed)),
            RoutingCommitOutcome::AlreadyDecided { decision_id } => {
                Ok(ServiceOutcome::AlreadyDecided { decision_id })
            }
            RoutingCommitOutcome::StaleClaim { .. } => Ok(ServiceOutcome::StaleClaim),
            RoutingCommitOutcome::InputsChanged(_) => {
                Err(ControlError::RevalidationExhausted { attempts })
            }
        }
    }

    /// Commits a normalization refusal as the message's one silent decision.
    ///
    /// The refusal goes through the same authoritative transaction as every
    /// other decision (inbox claim fence, one-decision key, root chain row,
    /// inbox finalization), with no candidates, no recipients, no scorer
    /// provenance, and therefore no dispatch outbox row. The chain root is
    /// the message itself: a refused message never joins or extends an
    /// existing delivery chain.
    async fn commit_refusal(
        &self,
        scope: &CompanyScope,
        claim: &InboxClaim,
        snapshot: &RoutingSnapshot,
        refusal: NormalizationRefusal,
    ) -> Result<ServiceOutcome> {
        let proposal = RoutingProposal {
            office_authority: snapshot.office_authority.clone(),
            office_input_hash: refusal_input_hash(
                claim.message_id,
                &refusal,
                &RoutingPolicy::default(),
            ),
            company_id: claim.company_id,
            message_id: claim.message_id,
            root_message_id: claim.message_id,
            claim_generation: claim.claim_generation,
            input_hash: refusal_input_hash(claim.message_id, &refusal, &snapshot.policy),
            origin: refusal.origin,
            candidates: Vec::new(),
            roster_scope: RosterScope::Targets,
            eligible_employee_ids: BTreeSet::new(),
            decision: silent_decision(claim.message_id, refusal.reason, &snapshot.policy),
            scorer: None,
        };
        match self.repository.commit_routing(scope, &proposal).await? {
            RoutingCommitOutcome::Committed(committed) => Ok(ServiceOutcome::Committed(committed)),
            RoutingCommitOutcome::AlreadyDecided { decision_id } => {
                Ok(ServiceOutcome::AlreadyDecided { decision_id })
            }
            RoutingCommitOutcome::StaleClaim { .. } => Ok(ServiceOutcome::StaleClaim),
            RoutingCommitOutcome::InputsChanged(failure) => {
                // A refusal pins no candidates and no roster, so only a
                // policy change can invalidate it; that is a retryable
                // condition, not a reason to guess.
                Err(ControlError::InvalidData(format!(
                    "refusal for {} invalidated by refreshed inputs: {failure:?}",
                    claim.message_id
                )))
            }
        }
    }
}

/// An empty silent decision pinned to the current policy.
fn silent_decision(
    message_id: MessageId,
    summary_reason: RoutingReason,
    policy: &RoutingPolicy,
) -> RoutingDecision {
    RoutingDecision {
        message_id: message_id.to_hex(),
        mode: RoutingMode::Silent,
        summary_reason,
        policy_version: policy.version.clone(),
        policy_fingerprint: policy.fingerprint(),
        recipients: Vec::new(),
    }
}

/// Canonical SHA-256 pinned on a refusal decision: the message id, the
/// closed reason code, the derived origin, and the policy fingerprint. No
/// message content enters the hash, so an encrypted wrap contributes only
/// its id.
pub fn refusal_input_hash(
    message_id: MessageId,
    refusal: &NormalizationRefusal,
    policy: &RoutingPolicy,
) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(b"ortak-routing-refusal-v0\0");
    hash_field(&mut hasher, message_id.as_bytes());
    hash_field(&mut hasher, origin_label(&refusal.origin).as_bytes());
    hash_field(&mut hasher, closed_code(&refusal.reason).as_bytes());
    hash_field(&mut hasher, policy.fingerprint().as_bytes());
    hasher.finalize().into()
}

fn origin_label(origin: &MessageOrigin) -> String {
    match origin {
        MessageOrigin::Human(id) => format!("human:{id}"),
        MessageOrigin::Employee(id) => format!("employee:{id}"),
        MessageOrigin::Integration(id) => format!("integration:{id}"),
        MessageOrigin::System => "system".to_owned(),
    }
}

fn remaining_claim_time(claim: &InboxClaim, snapshot: &RoutingSnapshot) -> Duration {
    let expires_at = snapshot
        .inbox
        .claim_expires_at
        .map(|expires| expires.min(claim.claim_expires_at))
        .unwrap_or(claim.claim_expires_at);
    (expires_at - Utc::now()).to_std().unwrap_or(Duration::ZERO)
}

async fn score_before_deadline<S: SemanticScorer>(
    scorer: &S,
    input: &SemanticScoringInput,
    deadline: Instant,
) -> ScoringOutcome {
    let started = Instant::now();
    let mut metadata = scorer.metadata();
    if started < deadline {
        let outcome = timeout_at(
            deadline,
            scorer.score(input, crate::semantic::ScoringBudget::until(deadline)),
        )
        .await;
        // A future which blocks within one poll can return after timeout_at's
        // timer was due. Its result is still late and must never wake anyone.
        match outcome {
            Ok(outcome) if Instant::now() < deadline => return outcome,
            _ => {}
        }
    }
    metadata.latency_ms = Some(i32::try_from(started.elapsed().as_millis()).unwrap_or(i32::MAX));
    metadata.usage = None;
    ScoringOutcome {
        result: Err(SemanticScoringFailure::TimedOut),
        metadata,
    }
}

/// Fingerprints normalized Office authorization without candidate revisions or
/// routing policy. Admission can revalidate the canonical message while keeping
/// the configuration revision chosen by the committed routing decision.
pub fn office_input_hash(
    envelope: &MessageEnvelope,
    root_message_id: MessageId,
    eligible: &BTreeSet<EmployeeId>,
) -> [u8; 32] {
    let envelope = envelope
        .clone()
        .with_delivery_chain(ortak_domain::DeliveryChain::root(root_message_id.to_hex()));
    routing_input_hash(&envelope, &[], eligible, &RoutingPolicy::default())
}

/// Canonical SHA-256 of the bounded router/scorer input pinned on a decision.
///
/// Every field is length-prefixed and every list is count-prefixed, so two
/// different inputs cannot share an encoding. The hash covers the immutable
/// normalized message (id, kind, origin, conversation, body, reply parent
/// and its origin, delivery-chain root), the trusted server-derived targets
/// (dispatch targets, structured mentions, Work assignments), the candidate
/// employees with their evaluated revisions, the conversation-eligible
/// employee set the router confined wakes to, and the policy fingerprint.
/// Mutable chain counters are deliberately excluded: the commit transaction
/// reapplies them from the locked chain row.
pub fn routing_input_hash(
    envelope: &MessageEnvelope,
    candidates: &[CandidateRevision],
    eligible: &BTreeSet<EmployeeId>,
    policy: &RoutingPolicy,
) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(b"ortak-routing-input-v1\0");
    hash_field(&mut hasher, envelope.id.as_bytes());
    hash_field(&mut hasher, closed_code(&envelope.kind).as_bytes());
    hash_field(&mut hasher, origin_label(&envelope.origin).as_bytes());
    match &envelope.conversation {
        ConversationContext::Channel { channel_id } => {
            hash_field(&mut hasher, b"channel");
            hash_field(&mut hasher, channel_id.as_bytes());
        }
        ConversationContext::Direct {
            conversation_id,
            employee_participants,
        } => {
            hash_field(&mut hasher, b"direct");
            hash_field(&mut hasher, conversation_id.as_bytes());
            hash_employee_list(&mut hasher, employee_participants);
        }
    }
    hash_field(&mut hasher, envelope.body.as_bytes());
    match &envelope.reply_to {
        None => hash_field(&mut hasher, b"no_reply"),
        Some(reply) => {
            hash_field(&mut hasher, b"reply");
            hash_field(&mut hasher, reply.message_id.as_bytes());
            hash_field(&mut hasher, origin_label(&reply.origin).as_bytes());
        }
    }
    hash_field(&mut hasher, envelope.chain().root_message_id().as_bytes());
    hash_employee_list(&mut hasher, &envelope.dispatch_targets);
    hash_employee_list(&mut hasher, &envelope.structured_mentions);
    hash_employee_list(&mut hasher, &envelope.assigned_employee_ids);
    hasher.update((candidates.len() as u64).to_be_bytes());
    for candidate in candidates {
        hash_field(&mut hasher, candidate.employee_id.as_str().as_bytes());
        hasher.update(candidate.revision_id.as_bytes());
    }
    // `BTreeSet` iterates in sorted order, so the encoding is canonical.
    hasher.update((eligible.len() as u64).to_be_bytes());
    for employee_id in eligible {
        hash_field(&mut hasher, employee_id.as_str().as_bytes());
    }
    hash_field(&mut hasher, policy.fingerprint().as_bytes());
    hasher.finalize().into()
}

fn hash_employee_list(hasher: &mut Sha256, employee_ids: &[EmployeeId]) {
    hasher.update((employee_ids.len() as u64).to_be_bytes());
    for employee_id in employee_ids {
        hash_field(hasher, employee_id.as_str().as_bytes());
    }
}

/// Stable snake_case code of a closed-vocabulary value.
fn closed_code<T: serde::Serialize>(value: &T) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_default()
}

fn hash_field(hasher: &mut Sha256, bytes: &[u8]) {
    hasher.update((bytes.len() as u64).to_be_bytes());
    hasher.update(bytes);
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use ortak_domain::{
        DeliveryChain, Employee, EmployeeCatalog, EmployeeManifest, EmployeeStatus,
        MessageEnvelope, RecipientAction, ReplyContext, RoutingMode, RoutingPolicy, RoutingReason,
    };
    use ortak_router::{Router, RoutingPreparation};

    use super::*;

    fn fixture(name: &str) -> Employee {
        let yaml = std::fs::read_to_string(format!(
            "{}/../../config/employees/{name}.yaml",
            env!("CARGO_MANIFEST_DIR")
        ))
        .expect("read fixture");
        let manifest: EmployeeManifest = serde_yaml::from_str(&yaml).expect("parse fixture");
        let mut employee = manifest.employee;
        employee.status = EmployeeStatus::Active;
        employee
    }

    fn catalog() -> EmployeeCatalog {
        EmployeeCatalog::new([fixture("cem"), fixture("zeynep")]).expect("catalog")
    }

    fn only(id: &str) -> BTreeSet<EmployeeId> {
        std::iter::once(EmployeeId::parse(id).expect("id")).collect()
    }

    fn employee_id(id: &str) -> EmployeeId {
        EmployeeId::parse(id).expect("id")
    }

    /// A message that structurally mentions Cem then Zeynep while only
    /// Zeynep is live in the channel.
    fn mixed_mention_message() -> MessageEnvelope {
        let mut message = MessageEnvelope::human_channel(
            "m".repeat(64),
            "sefa",
            "office",
            "Cem ve Zeynep, bakar mısınız?",
        );
        message.structured_mentions = vec![employee_id("cem"), employee_id("zeynep")];
        message
    }

    fn assert_cem_dropped_zeynep_woken(preparation: RoutingPreparation) {
        let RoutingPreparation::Final(decision) = preparation else {
            panic!("explicit mentions never fall through to semantic scoring");
        };
        assert_eq!(decision.mode, RoutingMode::Deterministic);
        assert_eq!(decision.summary_reason, RoutingReason::StructuredMention);
        assert_eq!(decision.wake_count(), 1);
        let by_id = decision
            .recipients
            .iter()
            .map(|recipient| (recipient.employee_id.as_str(), recipient))
            .collect::<BTreeMap<_, _>>();
        let cem = by_id["cem"];
        assert_eq!(cem.action, RecipientAction::Drop);
        assert_eq!(cem.reason, RoutingReason::TargetNotChannelMember);
        let zeynep = by_id["zeynep"];
        assert_eq!(zeynep.action, RecipientAction::Wake);
        assert_eq!(zeynep.reason, RoutingReason::StructuredMention);
    }

    #[test]
    fn ineligible_mention_does_not_consume_the_only_recipient_slot() {
        let router = Router::new(RoutingPolicy {
            max_recipients: 1,
            ..RoutingPolicy::default()
        })
        .expect("router");
        assert_cem_dropped_zeynep_woken(router.prepare_with_conversation_eligibility(
            &mixed_mention_message(),
            &catalog(),
            &only("zeynep"),
        ));
    }

    #[test]
    fn ineligible_mention_does_not_consume_the_last_chain_wake() {
        let policy = RoutingPolicy::default();
        let prior = ["prior-a", "prior-b", "prior-c"].map(employee_id);
        let chain = DeliveryChain::root("r".repeat(64))
            .advance_for_dispatch(prior.iter())
            .expect("chain");
        assert_eq!(
            policy.max_chain_wakes - chain.wake_count(),
            1,
            "exactly one chain wake remains"
        );
        let router = Router::new(policy).expect("router");
        assert_cem_dropped_zeynep_woken(router.prepare_with_conversation_eligibility(
            &mixed_mention_message().with_delivery_chain(chain),
            &catalog(),
            &only("zeynep"),
        ));
    }

    #[test]
    fn deterministic_target_outside_the_eligible_set_is_a_visible_drop_not_fanout() {
        let router = Router::new(RoutingPolicy::default()).expect("router");
        let message =
            MessageEnvelope::human_channel("m".repeat(64), "sefa", "office", "Cem, selam");
        let RoutingPreparation::Final(decision) =
            router.prepare_with_conversation_eligibility(&message, &catalog(), &only("zeynep"))
        else {
            panic!("a known but ineligible alias target must not become a semantic request");
        };
        assert_eq!(decision.mode, RoutingMode::Silent);
        assert_eq!(
            decision.summary_reason,
            RoutingReason::TargetNotChannelMember
        );
        assert_eq!(decision.recipients.len(), 1);
        assert_eq!(decision.recipients[0].employee_id.as_str(), "cem");
        assert_eq!(decision.recipients[0].action, RecipientAction::Drop);
    }

    #[test]
    fn semantic_roster_is_restricted_to_the_eligible_set() {
        let router = Router::new(RoutingPolicy::default()).expect("router");
        let message =
            MessageEnvelope::human_channel("m".repeat(64), "sefa", "office", "Herkese merhaba");
        let RoutingPreparation::Semantic(request) =
            router.prepare_with_conversation_eligibility(&message, &catalog(), &only("zeynep"))
        else {
            panic!("an untargeted human message reaches semantic preparation");
        };
        let ids = request
            .candidates()
            .iter()
            .map(|candidate| candidate.employee_id().as_str().to_owned())
            .collect::<Vec<_>>();
        assert_eq!(ids, vec!["zeynep".to_owned()]);

        let RoutingPreparation::Final(decision) =
            router.prepare_with_conversation_eligibility(&message, &catalog(), &BTreeSet::new())
        else {
            panic!("an empty eligible set is a final silent decision");
        };
        assert_eq!(decision.mode, RoutingMode::Silent);
        assert_eq!(decision.wake_count(), 0);
    }

    #[test]
    fn input_hash_covers_conversation_reply_kind_targets_and_root() {
        let policy = RoutingPolicy::default();
        let base = MessageEnvelope::human_channel("m".repeat(64), "sefa", "office", "merhaba");
        let none = BTreeSet::new();
        let baseline = routing_input_hash(&base, &[], &none, &policy);

        let mut other_channel = base.clone();
        other_channel.conversation = ConversationContext::Channel {
            channel_id: "elsewhere".to_owned(),
        };
        let mut reply = base.clone();
        reply.reply_to = Some(ReplyContext {
            message_id: "p".repeat(64),
            origin: MessageOrigin::Employee(EmployeeId::parse("cem").expect("id")),
        });
        let mut mentioned = base.clone();
        mentioned.structured_mentions = vec![EmployeeId::parse("zeynep").expect("id")];
        let mut assigned = base.clone();
        assigned.assigned_employee_ids = vec![EmployeeId::parse("cem").expect("id")];
        let mut dispatched = base.clone();
        dispatched.dispatch_targets = vec![EmployeeId::parse("cem").expect("id")];
        let mut reaction = base.clone();
        reaction.kind = ortak_domain::MessageKind::Reaction;
        let chained = base
            .clone()
            .with_delivery_chain(ortak_domain::DeliveryChain::root("r".repeat(64)));

        let variants = [
            other_channel,
            reply,
            mentioned,
            assigned,
            dispatched,
            reaction,
            chained,
        ];
        let mut hashes = variants
            .iter()
            .map(|variant| routing_input_hash(variant, &[], &none, &policy))
            .collect::<Vec<_>>();
        hashes.push(baseline);
        hashes.sort_unstable();
        hashes.dedup();
        assert_eq!(
            hashes.len(),
            variants.len() + 1,
            "every field changes the hash"
        );
        assert_eq!(
            routing_input_hash(&base, &[], &none, &policy),
            baseline,
            "hash is deterministic"
        );
        assert_ne!(
            routing_input_hash(&base, &[], &only("cem"), &policy),
            baseline,
            "the eligible set changes the hash"
        );
    }
}

#[cfg(test)]
#[path = "service_deadline_tests.rs"]
mod deadline_tests;
