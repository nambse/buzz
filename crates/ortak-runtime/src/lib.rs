#![deny(unsafe_code)]
#![warn(missing_docs)]
//! Ortak run dispatch and supervision foundation (Architecture v0 §4.3,
//! §4.4, §4.6; Implementation Plan Milestones 3 and 4).
//!
//! This crate turns a leased `run_dispatch` outbox row into one supervised
//! run over the existing [`RuntimeAdapter`](ortak_control::runtime::RuntimeAdapter)
//! port and feeds the runtime's stream back into `run_events`.
//!
//! Guarantees enforced here:
//!
//! - **A lease is a hint, never an authority.** [`RunDispatchRepository`]
//!   re-derives every fact from company-scoped durable rows: the exact outbox
//!   row and its current lease token/state/kind, the routing decision, the
//!   WAKE recipient and its pinned `employee_revision_id`, the delivery-chain
//!   visit, the decided inbox row and its signed Office event, the employee
//!   lifecycle status, and the immutable revision manifest plus its validated
//!   runtime binding. The result is the crate-sealed [`DispatchAuthority`];
//!   nothing in `outbox.payload` is trusted.
//! - **One durable run per `(company, routing_decision, employee)`**, pinned
//!   to the recipient's revision and the decision's message/root, created in
//!   `queued` state under the lease fence before any runtime call.
//! - **Runtime starts happen outside database transactions** with the stable
//!   idempotency key [`run_idempotency_key`], so a crash after the external
//!   start and before the acknowledgement is retried with the same key.
//!   Correlation is a compare-and-set on the `runs` row: a different runtime
//!   reference is refused, and the outbox lease is completed in the same
//!   commit that makes the correlation durable.
//! - **Events resume from the last durable cursor**, are normalized and
//!   redacted through the existing [`RunEvent`](ortak_control::run_event::RunEvent)
//!   contract, are appended idempotently, and move the run through
//!   `waiting`/`completed`/`failed`/`cancelled` only from typed events, with
//!   the terminal row update and the terminal event in one commit.
//! - **Cancellation is supervised and idempotent**, selects the run by its
//!   durable id (never by a client-supplied runtime reference), calls the
//!   adapter outside any transaction, then records the normalized
//!   cancellation.
//!
//! Office publication for `reply`/`channel` intents is deliberately not
//! enqueued here: the canonical server-derived delivery target for a run is
//! the next boundary (see [`supervisor`]). Nothing in this crate connects to
//! a real Hermes, Honcho, or Office deployment.

pub mod authority;
mod error;
pub mod postgres;
pub mod repository;
pub mod state;
pub mod supervisor;

pub use authority::{run_idempotency_key, DispatchAuthority, DispatchRefusal, RunInput};
pub use error::{Result, RunSupervisionError};
pub use repository::{
    AppendOutcome, CorrelationOutcome, DispatchAuthorization, PrepareOutcome, PreparedRun,
    RunCursorState, RunDispatchRepository,
};
pub use state::{fold_status, status_after, InvalidTransition, RunStatus, TerminalRecord};
pub use supervisor::{
    CancellationOutcome, DispatchOutcome, PumpOutcome, RunSupervisor, SupervisorConfig,
};
