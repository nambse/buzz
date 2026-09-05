//! Repository and service ports owned by the control layer.
//!
//! Adapters implement these traits; application services depend only on the
//! traits. The traits use native `async fn`, so implementations are selected
//! statically and the services are generic over them.

use std::collections::BTreeSet;
use std::time::Duration;

use chrono::{DateTime, Utc};
use ortak_domain::{
    Employee, EmployeeCatalog, EmployeeId, MessageEnvelope, MessageOrigin, RoutingPolicy,
    RoutingReason, SemanticScore,
};
use ortak_router::{SemanticRoutingRequest, SemanticScoringFailure};
use uuid::Uuid;

use crate::error::Result;
use crate::ids::{ClaimGeneration, CompanyScope, MessageId};
use crate::inbox::{InboxClaim, InboxEvent, InboxInsertOutcome, InboxReleaseOutcome, InboxRow};
use crate::outbox::{OutboxFailOutcome, OutboxKind, OutboxLease};
use crate::provisioning::{
    IdentityReservation, OperationUpdate, ProvisioningOperation, ProvisioningRequest,
    RevisionActivation, StepRecord,
};
use crate::routing::{
    ChainState, EmployeeRecord, RoutingCommitOutcome, RoutingProposal, ScorerMetadata,
    StoredDecision,
};
use crate::run_event::RunEvent;

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
    /// Employees that may be woken for this conversation, derived by the
    /// adapter from live conversation membership and each employee's
    /// current verified Office identity.
    ///
    /// The routing service intersects every path (structured mentions,
    /// aliases, replies, assignments, and the semantic roster) with this
    /// set. A deterministic target outside it is recorded as a visible
    /// `target_not_channel_member` drop and never falls through to
    /// semantic fan-out. An empty set means no employee may wake.
    pub eligible_employee_ids: BTreeSet<EmployeeId>,
}

/// An explicit, server-derived refusal to build a routable envelope.
///
/// A refusal is committed as one silent, empty routing decision through the
/// same authoritative inbox-claim transaction as any other decision, so the
/// inbox row reaches `decided` with an Activity-visible reason and no
/// dispatch outbox row. The reason must be a closed [`RoutingReason`] code.
#[derive(Clone, Debug, PartialEq)]
pub struct NormalizationRefusal {
    /// Closed snake_case reason persisted as `summary_reason`.
    pub reason: RoutingReason,
    /// The most specific origin the server could derive without trusting
    /// message content or client tags (for an encrypted wrap this is the
    /// outer signing key, recorded as a human-class origin id).
    pub origin: MessageOrigin,
}

/// Result of normalizing the accepted event behind an inbox row.
#[derive(Clone, Debug, PartialEq)]
pub enum Normalization {
    /// A routable envelope with server-derived origin, context, and targets.
    Message(Box<NormalizedMessage>),
    /// The event is Office input the server refuses to route right now; the
    /// refusal becomes a durable silent decision.
    Refused(NormalizationRefusal),
    /// The event is not Office message input at all (a kind the router can
    /// never act on); the inbox row is finalized as `dropped` with no
    /// decision.
    NotOfficeInput,
}

/// Turns an inbox row into a typed [`Normalization`].
///
/// Implementations must derive every trusted field (origin, channel, reply
/// parent, mentions, loop root) from canonical server rows, never from the
/// message body or client-supplied tags alone, and must never read or pass
/// on encrypted content.
#[allow(async_fn_in_trait)]
pub trait MessageNormalizer {
    /// Normalizes the accepted event behind an inbox row.
    async fn normalize(&self, scope: &CompanyScope, inbox: &InboxRow) -> Result<Normalization>;
}

/// Durable provisioning saga state (Architecture v0 §6).
#[allow(async_fn_in_trait)]
pub trait ProvisioningRepository {
    /// Creates the operation with every step `pending`, or returns the
    /// existing operation for the idempotency key when its manifest
    /// fingerprint, mode, and dry-run flag all match; any difference is a
    /// conflict.
    async fn begin_operation(
        &self,
        scope: &CompanyScope,
        request: &ProvisioningRequest,
    ) -> Result<ProvisioningOperation>;

    /// Reads an operation with its steps in execution order.
    async fn load_operation(
        &self,
        scope: &CompanyScope,
        operation_id: Uuid,
    ) -> Result<Option<ProvisioningOperation>>;

    /// Updates status, current step, error, and finish time.
    ///
    /// Fenced by [`OperationStatus::can_transition_to`] and by
    /// `result_revision_id`: a write that would regress a terminal or
    /// activated operation, or turn compensation back into a run, is refused
    /// with [`ProvisioningError::Superseded`] and leaves the row unchanged.
    ///
    /// [`OperationStatus::can_transition_to`]: crate::provisioning::OperationStatus::can_transition_to
    /// [`ProvisioningError::Superseded`]: crate::provisioning::ProvisioningError::Superseded
    async fn update_operation(
        &self,
        scope: &CompanyScope,
        operation_id: Uuid,
        update: &OperationUpdate,
    ) -> Result<()>;

    /// Upserts one step record by `(operation, step)`.
    ///
    /// Fenced by [`StepState::can_transition_to`] and by the operation row:
    /// once the operation is terminal or has a `result_revision_id`, or when
    /// the stored step state does not allow the new state, the write is
    /// refused with [`ProvisioningError::Superseded`] and nothing changes.
    ///
    /// [`StepState::can_transition_to`]: crate::provisioning::StepState::can_transition_to
    /// [`ProvisioningError::Superseded`]: crate::provisioning::ProvisioningError::Superseded
    async fn record_step(
        &self,
        scope: &CompanyScope,
        operation_id: Uuid,
        step: &StepRecord,
    ) -> Result<()>;

    /// Inserts the employee row as `draft` if absent; reports the existing
    /// status otherwise. Never changes an existing row.
    async fn reserve_employee_identity(
        &self,
        scope: &CompanyScope,
        employee_id: &EmployeeId,
    ) -> Result<IdentityReservation>;

    /// In one transaction: inserts the immutable revision, its runtime,
    /// memory, and Office bindings with validation timestamps, replaces the
    /// employee's aliases, sets the employee active on that revision, records
    /// the activation step, and marks the operation `succeeded` with the
    /// revision id. Returns the new revision id.
    ///
    /// Replaying an operation whose `result_revision_id` is already set
    /// returns that id without writing. Dry-run, terminal, and
    /// `compensating` operations are refused with
    /// [`ProvisioningError::InvalidTransition`].
    ///
    /// [`ProvisioningError::InvalidTransition`]: crate::provisioning::ProvisioningError::InvalidTransition
    async fn activate_revision(
        &self,
        scope: &CompanyScope,
        operation_id: Uuid,
        activation: &RevisionActivation,
    ) -> Result<Uuid>;
}

/// Result of appending normalized events to a run.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RunEventAppend {
    /// Sequences assigned to the appended events, in input order.
    pub sequences: Vec<i64>,
    /// Events skipped because their runtime cursor was already stored.
    pub duplicate_cursors: Vec<String>,
}

/// Ordered, append-only run activity (Architecture v0 §4.6).
#[allow(async_fn_in_trait)]
pub trait RunEventRepository {
    /// Appends already-normalized events with dense sequences under the run
    /// row lock. Events whose `runtime_cursor` already exists are skipped, not
    /// duplicated. Events must pass [`RunEvent::validate`].
    async fn append_run_events(
        &self,
        scope: &CompanyScope,
        run_id: Uuid,
        events: &[RunEvent],
    ) -> Result<RunEventAppend>;

    /// Reads up to `limit` events with `sequence > after` in order.
    async fn run_events_after(
        &self,
        scope: &CompanyScope,
        run_id: Uuid,
        after: i64,
        limit: i64,
    ) -> Result<Vec<RunEvent>>;
}
