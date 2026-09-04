//! Repository and service ports owned by the control layer.
//!
//! Adapters implement these traits; application services depend only on the
//! traits. The traits use native `async fn`, so implementations are selected
//! statically and the services are generic over them.

use std::time::Duration;

use chrono::{DateTime, Utc};
use ortak_domain::{
    Employee, EmployeeCatalog, EmployeeId, MessageEnvelope, RoutingPolicy, SemanticScore,
};
use ortak_router::{SemanticRoutingRequest, SemanticScoringFailure};
use uuid::Uuid;

use crate::error::Result;
use crate::ids::{ClaimGeneration, CompanyScope, MessageId};
use crate::inbox::{InboxClaim, InboxEvent, InboxInsertOutcome, InboxReleaseOutcome, InboxRow};
use crate::outbox::{OutboxFailOutcome, OutboxKind, OutboxLease};
use crate::routing::{
    ChainState, EmployeeRecord, RoutingCommitOutcome, RoutingProposal, ScorerMetadata,
    StoredDecision,
};

/// Server-owned company resolution.
#[allow(async_fn_in_trait)]
pub trait CompanyDirectory {
    /// Resolves the company bound to an authenticated community; unknown mappings fail closed.
    async fn resolve_company_for_community(&self, community_id: Uuid) -> Result<CompanyScope>;

    /// Resolves a company from the operator-facing registry slug.
    async fn resolve_company_by_slug(&self, slug: &str) -> Result<CompanyScope>;

    /// Reads the validated company-wide routing policy.
    async fn routing_policy(&self, scope: &CompanyScope) -> Result<RoutingPolicy>;
}

/// Durable Office inbox handoff.
#[allow(async_fn_in_trait)]
pub trait InboxRepository {
    /// Idempotently records an accepted event, deriving the company from the community binding.
    async fn insert_accepted_event(
        &self,
        community_id: Uuid,
        event: &InboxEvent,
    ) -> Result<InboxInsertOutcome>;

    /// Claims the oldest due row, including rows whose previous lease expired.
    async fn claim_next(
        &self,
        scope: &CompanyScope,
        worker_id: &str,
        lease: Duration,
        max_attempts: i32,
    ) -> Result<Option<InboxClaim>>;

    /// Claims one specific row if it is due or its lease expired.
    async fn claim_message(
        &self,
        scope: &CompanyScope,
        message_id: MessageId,
        worker_id: &str,
        lease: Duration,
        max_attempts: i32,
    ) -> Result<Option<InboxClaim>>;

    /// Returns a claimed row to `pending` (or terminal `failed`) with a durable error.
    async fn release_for_retry(
        &self,
        scope: &CompanyScope,
        message_id: MessageId,
        claim_generation: ClaimGeneration,
        error: &str,
        retry_after: DateTime<Utc>,
        max_attempts: i32,
    ) -> Result<InboxReleaseOutcome>;

    /// Finalizes a claimed row as `dropped` without a decision; false when the claim is stale.
    async fn finalize_dropped(
        &self,
        scope: &CompanyScope,
        message_id: MessageId,
        claim_generation: ClaimGeneration,
        reason: &str,
    ) -> Result<bool>;

    /// Reads one inbox row.
    async fn inbox_row(
        &self,
        scope: &CompanyScope,
        message_id: MessageId,
    ) -> Result<Option<InboxRow>>;
}

/// One roster entry read for routing.
#[derive(Clone, Debug, PartialEq)]
pub struct RosterEmployee {
    /// Lifecycle facts used by refreshed guards.
    pub record: EmployeeRecord,
    /// Full definition from the active revision manifest; `None` without an active revision.
    pub employee: Option<Employee>,
}

/// Out-of-transaction read used to prepare a routing proposal.
#[derive(Clone, Debug, PartialEq)]
pub struct RoutingSnapshot {
    /// Inbox row as read.
    pub inbox: InboxRow,
    /// Company policy as read.
    pub policy: RoutingPolicy,
    /// Every employee of the company in stable id order.
    pub roster: Vec<RosterEmployee>,
}

impl RoutingSnapshot {
    /// Builds the validated routing catalog from employees with an active revision.
    pub fn catalog(&self) -> Result<EmployeeCatalog> {
        Ok(EmployeeCatalog::new(
            self.roster
                .iter()
                .filter_map(|entry| entry.employee.clone()),
        )?)
    }

    /// Returns the active revision of an employee, if any.
    pub fn active_revision(&self, employee_id: &EmployeeId) -> Option<Uuid> {
        self.roster
            .iter()
            .find(|entry| &entry.record.id == employee_id)
            .and_then(|entry| entry.record.active_revision_id)
    }
}

/// Routing decision persistence and the authoritative commit transaction.
#[allow(async_fn_in_trait)]
pub trait RoutingRepository {
    /// Reads the inbox row, policy, and roster used to prepare a proposal.
    async fn routing_snapshot(
        &self,
        scope: &CompanyScope,
        message_id: MessageId,
    ) -> Result<Option<RoutingSnapshot>>;

    /// Reads the durable chain row and its visits without locking.
    async fn chain_state(
        &self,
        scope: &CompanyScope,
        root_message_id: MessageId,
    ) -> Result<Option<ChainState>>;

    /// Runs the short authoritative transaction for one proposal.
    async fn commit_routing(
        &self,
        scope: &CompanyScope,
        proposal: &RoutingProposal,
    ) -> Result<RoutingCommitOutcome>;

    /// Reads the one dispatching decision for a message, with its recipients.
    async fn decision_for_message(
        &self,
        scope: &CompanyScope,
        message_id: MessageId,
    ) -> Result<Option<StoredDecision>>;
}

/// Leased, bounded-retry transactional outbox.
#[allow(async_fn_in_trait)]
pub trait OutboxRepository {
    /// Leases up to `limit` due rows for one worker.
    async fn claim_due(
        &self,
        scope: &CompanyScope,
        kind: Option<OutboxKind>,
        worker_id: &str,
        lease: Duration,
        limit: i64,
    ) -> Result<Vec<OutboxLease>>;

    /// Marks a leased row delivered; false when the lease token is stale.
    async fn complete(&self, scope: &CompanyScope, lease: &OutboxLease) -> Result<bool>;

    /// Records a failed attempt, scheduling a retry or reaching terminal failure.
    async fn fail(
        &self,
        scope: &CompanyScope,
        lease: &OutboxLease,
        error: &str,
        retry_after: DateTime<Utc>,
    ) -> Result<OutboxFailOutcome>;

    /// Operator retry: returns a terminal `failed` row to `pending`.
    async fn reopen(&self, scope: &CompanyScope, outbox_id: Uuid) -> Result<bool>;
}

/// Result of one remote scoring call, with the provenance to pin.
#[derive(Clone, Debug, PartialEq)]
pub struct ScoringOutcome {
    /// Bounded scores or a typed failure.
    pub result: std::result::Result<Vec<SemanticScore>, SemanticScoringFailure>,
    /// Adapter/model/prompt/scorer versions and telemetry.
    pub metadata: ScorerMetadata,
}

/// Remote semantic scorer, always called outside database transactions.
#[allow(async_fn_in_trait)]
pub trait SemanticScorer {
    /// Scores the least-privilege request under the control layer's deadline.
    async fn score(&self, request: &SemanticRoutingRequest) -> ScoringOutcome;
}

/// Transport-independent message produced by the Office adapter.
#[derive(Clone, Debug, PartialEq)]
pub struct NormalizedMessage {
    /// Envelope with server-derived origin, context, and trusted targets.
    pub envelope: MessageEnvelope,
    /// Root of the delivery chain this message belongs to.
    pub root_message_id: MessageId,
}

/// Turns an inbox row into a normalized envelope, or `None` when it cannot route.
#[allow(async_fn_in_trait)]
pub trait MessageNormalizer {
    /// Normalizes the accepted event behind an inbox row.
    async fn normalize(
        &self,
        scope: &CompanyScope,
        inbox: &InboxRow,
    ) -> Result<Option<NormalizedMessage>>;
}
