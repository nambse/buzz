//! One-shot semantic input capture through the production inbox service.

use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use chrono::{DateTime, Utc};
use ortak_domain::{EmployeeId, MessageEnvelope, RoutingPolicy};
use ortak_router::SemanticScoringFailure;
use uuid::Uuid;

use crate::fakes::InMemoryProvisioningRepository;
use crate::inbox::{
    InboxClaim, InboxEvent, InboxInsertOutcome, InboxReleaseOutcome, InboxRow, InboxState,
};
use crate::office_authority::OfficeAuthority;
use crate::ports::{
    InboxRepository, MessageNormalizer, Normalization, NormalizedMessage, RosterEmployee,
    RoutingRepository, RoutingSnapshot, ScoringOutcome, SemanticScorer,
};
use crate::routing::{
    ChainState, RoutingCommitOutcome, RoutingProposal, ScorerMetadata, StoredDecision,
};
use crate::{
    ClaimGeneration, CompanyScope, ControlError, InboxRoutingService, MessageId, Result,
    RoutingWorkerConfig, SemanticScoringInput,
};

/// In-memory adapter-test fixture with one fresh, randomly generated company.
///
/// Captures input from the real inbox service without exposing its constructor.
/// Retaining this fixture permits independent source/policy/revision cache tests
/// under the same company. It cannot select an existing company's identity and
/// performs no network, database, credential lookup, or employee activation.
#[derive(Clone, Debug)]
pub struct SemanticScoringFixture {
    scope: CompanyScope,
}

impl Default for SemanticScoringFixture {
    fn default() -> Self {
        Self::new()
    }
}

impl SemanticScoringFixture {
    /// Creates a new isolated fake company through the existing provisioning fixture.
    pub fn new() -> Self {
        Self {
            scope: InMemoryProvisioningRepository::new().scope(),
        }
    }

    /// Fresh fixture scope, usable to construct a company-bound adapter under test.
    pub fn scope(&self) -> &CompanyScope {
        &self.scope
    }

    /// Routes the supplied fake snapshot through the actual service and captures
    /// its sealed scoring input. Deterministic/refused inputs return an error.
    /// Reusing a message id is allowed here because each call is a one-shot fake
    /// repository; real inbox uniqueness and final authority remain DB concerns.
    pub async fn capture(
        &self,
        policy: RoutingPolicy,
        roster: Vec<RosterEmployee>,
        envelope: MessageEnvelope,
        eligible: BTreeSet<EmployeeId>,
    ) -> Result<SemanticScoringInput> {
        let message_id = MessageId::parse_hex(&envelope.id)?;
        let root_message_id = MessageId::parse_hex(envelope.chain().root_message_id())?;
        let now = Utc::now();
        let expires = now + chrono::Duration::seconds(60);
        let event = InboxEvent {
            event_id: message_id,
            event_created_at: now,
            event_kind: 9,
            author_pubkey: [1; 32],
            channel_id: None,
        };
        let claim = InboxClaim {
            company_id: self.scope.company_id(),
            message_id,
            claim_generation: ClaimGeneration(1),
            claimed_by: "semantic-capture".to_owned(),
            claim_expires_at: expires,
            attempt_count: 1,
            event: event.clone(),
        };
        let snapshot = RoutingSnapshot {
            office_authority: OfficeAuthority::new(self.scope.company_id(), 1, None),
            inbox: InboxRow {
                event,
                state: InboxState::Claimed,
                claim_generation: claim.claim_generation,
                claimed_by: Some(claim.claimed_by.clone()),
                claim_expires_at: Some(expires),
                attempt_count: 1,
                retry_after: None,
                last_error: None,
                received_at: now,
                finalized_at: None,
            },
            policy,
            roster,
        };
        let capture = Arc::new(Mutex::new(None));
        let service = InboxRoutingService::new(
            CaptureRepository(snapshot),
            CaptureNormalizer(NormalizedMessage {
                envelope,
                root_message_id,
                eligible_employee_ids: eligible,
            }),
            CaptureScorer(capture.clone()),
            RoutingWorkerConfig::default(),
        );
        service.route_claim(&self.scope, &claim).await?;
        let input = capture.lock().map_err(|_| fixture_error())?.take();
        input.ok_or(ControlError::InvalidProposal(
            "fixture did not request semantic scoring",
        ))
    }
}

fn fixture_error() -> ControlError {
    ControlError::InvalidProposal("unsupported semantic fixture repository operation")
}

struct CaptureRepository(RoutingSnapshot);
impl InboxRepository for CaptureRepository {
    async fn insert_accepted_event(&self, _: Uuid, _: &InboxEvent) -> Result<InboxInsertOutcome> {
        Err(fixture_error())
    }
    async fn claim_next(
        &self,
        _: &CompanyScope,
        _: &str,
        _: Duration,
        _: i32,
    ) -> Result<Option<InboxClaim>> {
        Err(fixture_error())
    }
    async fn claim_message(
        &self,
        _: &CompanyScope,
        _: MessageId,
        _: &str,
        _: Duration,
        _: i32,
    ) -> Result<Option<InboxClaim>> {
        Err(fixture_error())
    }
    async fn release_for_retry(
        &self,
        _: &CompanyScope,
        _: MessageId,
        _: ClaimGeneration,
        _: &str,
        _: DateTime<Utc>,
        _: i32,
    ) -> Result<InboxReleaseOutcome> {
        Ok(InboxReleaseOutcome::Retrying)
    }
    async fn finalize_dropped(
        &self,
        _: &CompanyScope,
        _: MessageId,
        _: ClaimGeneration,
        _: &str,
    ) -> Result<bool> {
        Ok(true)
    }
    async fn inbox_row(&self, _: &CompanyScope, _: MessageId) -> Result<Option<InboxRow>> {
        Err(fixture_error())
    }
}
impl RoutingRepository for CaptureRepository {
    async fn routing_snapshot(
        &self,
        _: &CompanyScope,
        _: MessageId,
    ) -> Result<Option<RoutingSnapshot>> {
        Ok(Some(self.0.clone()))
    }
    async fn chain_state(&self, _: &CompanyScope, _: MessageId) -> Result<Option<ChainState>> {
        Ok(None)
    }
    async fn commit_routing(
        &self,
        _: &CompanyScope,
        _: &RoutingProposal,
    ) -> Result<RoutingCommitOutcome> {
        // Capture only; this fake never fabricates a persisted decision or dispatch.
        Ok(RoutingCommitOutcome::StaleClaim {
            observed_state: InboxState::Claimed,
            observed_generation: ClaimGeneration(2),
        })
    }
    async fn decision_for_message(
        &self,
        _: &CompanyScope,
        _: MessageId,
    ) -> Result<Option<StoredDecision>> {
        Ok(None)
    }
}
struct CaptureNormalizer(NormalizedMessage);
impl MessageNormalizer for CaptureNormalizer {
    async fn normalize(&self, _: &CompanyScope, _: &InboxRow) -> Result<Normalization> {
        Ok(Normalization::Message(Box::new(self.0.clone())))
    }
}
struct CaptureScorer(Arc<Mutex<Option<SemanticScoringInput>>>);
impl SemanticScorer for CaptureScorer {
    fn metadata(&self) -> ScorerMetadata {
        crate::DisabledSemanticScorer::metadata()
    }
    async fn score(&self, input: &SemanticScoringInput) -> ScoringOutcome {
        let result = if let Ok(mut capture) = self.0.lock() {
            *capture = Some(input.clone());
            Err(SemanticScoringFailure::Disabled)
        } else {
            Err(SemanticScoringFailure::Unavailable)
        };
        ScoringOutcome {
            result,
            metadata: self.metadata(),
        }
    }
}
