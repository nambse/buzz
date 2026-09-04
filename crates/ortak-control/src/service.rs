//! Inbox routing application service.
//!
//! The service owns the two-phase flow from Architecture v0 §4.2: claim and
//! snapshot, optional semantic scoring outside any transaction, then the
//! authoritative commit with bounded re-score attempts.

use std::time::Duration;

use chrono::Utc;
use ortak_domain::{
    MessageEnvelope, MessageOrigin, RoutingDecision, RoutingMode, RoutingPolicy, RoutingReason,
};
use ortak_router::{Router, RoutingPreparation};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::error::{ControlError, Result};
use crate::ids::CompanyScope;
use crate::inbox::{InboxClaim, InboxState};
use crate::ports::{
    InboxRepository, MessageNormalizer, RoutingRepository, RoutingSnapshot, SemanticScorer,
};
use crate::routing::{
    CandidateRevision, ChainState, CommittedDecision, RosterScope, RoutingCommitOutcome,
    RoutingProposal,
};

/// Worker tuning for the inbox routing service.
#[derive(Clone, Debug)]
pub struct RoutingWorkerConfig {
    /// Identity recorded on inbox claims.
    pub worker_id: String,
    /// Inbox claim lease; a slower scorer loses its claim to a reclaiming worker.
    pub claim_lease: Duration,
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
            max_claim_attempts: 5,
            max_revalidation_attempts: 3,
            retry_backoff: Duration::from_secs(30),
        }
    }
}

/// Outcome of routing one claimed inbox row.
#[derive(Clone, Debug, PartialEq)]
pub enum ServiceOutcome {
    /// A decision was committed; dispatch rows are in the outbox.
    Committed(CommittedDecision),
    /// The message already had its dispatching decision.
    AlreadyDecided {
        /// Existing decision id.
        decision_id: Uuid,
    },
    /// Another worker reclaimed the row; nothing was written.
    StaleClaim,
    /// The event could not be normalized and was finalized as `dropped`.
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
        while attempts < self.config.max_revalidation_attempts.max(1) {
            attempts += 1;
            let snapshot = self.snapshot_for_claim(scope, claim).await?;
            let Some(snapshot) = snapshot else {
                return Ok(ServiceOutcome::StaleClaim);
            };

            let Some(normalized) = self.normalizer.normalize(scope, &snapshot.inbox).await? else {
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

            let catalog = snapshot.catalog()?;
            let router = Router::new(snapshot.policy.clone())?;
            let (decision, scorer, candidates, roster_scope) =
                match router.prepare(&envelope, &catalog) {
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
                        let candidates = request
                            .candidates()
                            .iter()
                            .filter_map(|candidate| {
                                snapshot.active_revision(candidate.employee_id()).map(
                                    |revision_id| CandidateRevision {
                                        employee_id: candidate.employee_id().clone(),
                                        revision_id,
                                    },
                                )
                            })
                            .collect::<Vec<_>>();
                        // Remote scoring runs here, with no database transaction open.
                        let outcome = self.scorer.score(&request).await;
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
                company_id: claim.company_id,
                message_id: claim.message_id,
                root_message_id,
                claim_generation: claim.claim_generation,
                origin: envelope.origin.clone(),
                input_hash: routing_input_hash(&envelope, &candidates, &snapshot.policy),
                candidates,
                roster_scope,
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
        let Some(normalized) = self.normalizer.normalize(scope, &snapshot.inbox).await? else {
            return Ok(ServiceOutcome::StaleClaim);
        };
        let proposal = RoutingProposal {
            company_id: claim.company_id,
            message_id: claim.message_id,
            root_message_id: normalized.root_message_id,
            claim_generation: claim.claim_generation,
            origin: normalized.envelope.origin.clone(),
            input_hash: routing_input_hash(&normalized.envelope, &[], &snapshot.policy),
            candidates: Vec::new(),
            roster_scope: RosterScope::Targets,
            decision: RoutingDecision {
                message_id: claim.message_id.to_hex(),
                mode: RoutingMode::Silent,
                summary_reason: RoutingReason::RevalidationExhausted,
                policy_version: snapshot.policy.version.clone(),
                policy_fingerprint: snapshot.policy.fingerprint(),
                recipients: Vec::new(),
            },
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
}

/// Canonical SHA-256 of the bounded router/scorer input pinned on a decision.
pub fn routing_input_hash(
    envelope: &MessageEnvelope,
    candidates: &[CandidateRevision],
    policy: &RoutingPolicy,
) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(b"ortak-routing-input-v0\0");
    hash_field(&mut hasher, envelope.id.as_bytes());
    hash_field(&mut hasher, envelope.body.as_bytes());
    let origin = match &envelope.origin {
        MessageOrigin::Human(id) => format!("human:{id}"),
        MessageOrigin::Employee(id) => format!("employee:{id}"),
        MessageOrigin::Integration(id) => format!("integration:{id}"),
        MessageOrigin::System => "system".to_owned(),
    };
    hash_field(&mut hasher, origin.as_bytes());
    hasher.update((candidates.len() as u64).to_be_bytes());
    for candidate in candidates {
        hash_field(&mut hasher, candidate.employee_id.as_str().as_bytes());
        hasher.update(candidate.revision_id.as_bytes());
    }
    hash_field(&mut hasher, policy.fingerprint().as_bytes());
    hasher.finalize().into()
}

fn hash_field(hasher: &mut Sha256, bytes: &[u8]) {
    hasher.update((bytes.len() as u64).to_be_bytes());
    hasher.update(bytes);
}
