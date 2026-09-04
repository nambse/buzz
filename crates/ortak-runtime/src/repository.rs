//! Repository seam for run dispatch and supervision on top of the Milestone 1
//! schema and the existing outbox lease contract.
//!
//! The existing [`OutboxRepository`](ortak_control::ports::OutboxRepository)
//! keeps ownership of leasing, bounded retry, terminal failure, and operator
//! reopen. This seam adds only what supervision needs: deriving a sealed
//! [`DispatchAuthority`] from durable rows, creating the one durable run per
//! decision recipient under the lease fence, compare-and-set correlation
//! that completes the lease in the same commit, the durable event cursor, and
//! fenced event appends that move the run through its lifecycle atomically.

use chrono::{DateTime, Utc};
use ortak_control::outbox::OutboxLease;
use ortak_control::run_event::RunEvent;
use ortak_control::runtime::{RunStartReceipt, RuntimeCursor, RuntimeRunRef};
use ortak_control::CompanyScope;
use ortak_domain::EmployeeId;
use uuid::Uuid;

use crate::authority::{DispatchAuthority, DispatchRefusal};
use crate::error::Result;
use crate::state::RunStatus;

/// Result of verifying a lease against durable rows.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DispatchAuthorization {
    /// Every durable fact checked out; the sealed authority may dispatch.
    Authorized(Box<DispatchAuthority>),
    /// A durable fact refuses the dispatch; the lease should record a
    /// bounded failure so the row retries or becomes terminal.
    Refused(DispatchRefusal),
    /// The row is no longer pending under this lease token, or the lease
    /// expired at the database clock; nothing may be written under it.
    StaleLease,
}

/// The durable run for a decision recipient, as read under the lease fence.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreparedRun {
    /// Durable run id.
    pub run_id: Uuid,
    /// Status as read.
    pub status: RunStatus,
    /// Runtime correlation, when a previous attempt already started it.
    pub runtime_run_ref: Option<RuntimeRunRef>,
    /// True when this call inserted the row.
    pub created: bool,
}

/// Result of preparing the durable run.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PrepareOutcome {
    /// The run row exists (new or replayed) and the lease is still ours.
    Prepared(PreparedRun),
    /// The lease token no longer matches; nothing was written.
    StaleLease,
}

/// Result of the compare-and-set correlation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CorrelationOutcome {
    /// The row moved from `queued` to `running` with this runtime reference.
    Correlated {
        /// True when the outbox lease was completed in the same commit;
        /// false when the lease was stale (the correlation is still durable).
        lease_completed: bool,
    },
    /// The row already held this exact runtime reference.
    AlreadyCorrelated {
        /// Status as read.
        status: RunStatus,
        /// Lease completion result, as above.
        lease_completed: bool,
    },
    /// The row holds a different runtime reference; nothing changed.
    RefConflict {
        /// Reference on the durable row.
        durable: RuntimeRunRef,
    },
    /// The row is terminal without a runtime reference; nothing changed
    /// except the lease, which is completed because no start can follow.
    Terminal {
        /// Terminal status.
        status: RunStatus,
        /// Lease completion result, as above.
        lease_completed: bool,
    },
}

/// Durable supervision state of one run.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RunCursorState {
    /// Run id.
    pub run_id: Uuid,
    /// Employee executing the run.
    pub employee_id: EmployeeId,
    /// Status as read.
    pub status: RunStatus,
    /// Runtime correlation, if any.
    pub runtime_run_ref: Option<RuntimeRunRef>,
    /// Last runtime cursor stored in `run_events`, if any.
    pub last_cursor: Option<RuntimeCursor>,
    /// Number of stored events.
    pub event_count: i64,
    /// When the row was last updated.
    pub updated_at: DateTime<Utc>,
}

/// Result of a fenced event append.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AppendOutcome {
    /// Events were appended (or all deduplicated) and the status updated.
    Appended {
        /// Sequences assigned, in input order, for events actually stored.
        sequences: Vec<i64>,
        /// Cursors skipped because they were already stored.
        duplicate_cursors: Vec<String>,
        /// Status after the append.
        status: RunStatus,
    },
    /// The run was already terminal; nothing was written.
    RunTerminal {
        /// Terminal status.
        status: RunStatus,
    },
    /// The run's durable runtime reference differs from the expected one;
    /// nothing was written.
    RefMismatch {
        /// Reference on the durable row, if any.
        durable: Option<RuntimeRunRef>,
    },
}

/// Persistence for run dispatch and supervision.
#[allow(async_fn_in_trait)]
pub trait RunDispatchRepository {
    /// Verifies `lease` against the durable `run_dispatch` row and derives
    /// the sealed authority from company-scoped rows only.
    ///
    /// Errors (nothing written): the row does not exist in the scope, the
    /// row is not `run_dispatch`, or the lease's decision/employee hints
    /// disagree with the row. A stale token or expired lease is
    /// [`DispatchAuthorization::StaleLease`]; any other durable fact that
    /// blocks the dispatch is [`DispatchAuthorization::Refused`].
    async fn authorize_dispatch(
        &self,
        scope: &CompanyScope,
        lease: &OutboxLease,
    ) -> Result<DispatchAuthorization>;

    /// In one transaction under the lease fence: inserts the `queued` run for
    /// `(company, routing_decision, employee)` if absent, pinned to the
    /// authority's revision and message/root, with its `run.queued` event at
    /// sequence 0; reads the row back; and records the run id on the outbox
    /// row. A lease that is no longer current rolls everything back.
    async fn prepare_run(
        &self,
        scope: &CompanyScope,
        authority: &DispatchAuthority,
    ) -> Result<PrepareOutcome>;

    /// Compare-and-set correlation after an external start: moves the run
    /// from `queued` without a reference to `running` with
    /// `receipt.runtime_run_ref`, refusing a different reference, and
    /// completes the outbox lease in the same commit.
    async fn correlate_run(
        &self,
        scope: &CompanyScope,
        authority: &DispatchAuthority,
        run_id: Uuid,
        receipt: &RunStartReceipt,
    ) -> Result<CorrelationOutcome>;

    /// Reads the run's status, correlation, and last durable cursor.
    async fn run_cursor_state(
        &self,
        scope: &CompanyScope,
        run_id: Uuid,
    ) -> Result<Option<RunCursorState>>;

    /// Under the run row lock: refuses terminal runs and a runtime reference
    /// other than `expected_ref`, appends the already-normalized events with
    /// dense sequences (skipping cursors already stored), folds the typed
    /// events over the current status, and writes the resulting status and
    /// terminal facts in the same commit.
    async fn append_supervised_events(
        &self,
        scope: &CompanyScope,
        run_id: Uuid,
        expected_ref: &RuntimeRunRef,
        events: &[RunEvent],
    ) -> Result<AppendOutcome>;
}
