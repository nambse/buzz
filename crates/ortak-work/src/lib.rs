#![deny(unsafe_code)]
#![warn(missing_docs)]
//! Ortak Work and Projects foundation (Architecture v0 §4.3, §7;
//! Implementation Plan Milestone 6).
//!
//! This crate is the control-layer seam over the pure
//! [`ortak_domain`] work aggregates and the migration 0047 relations. It
//! delivers the minimal useful flow behind the [`WorkRepository`] port on
//! [`PgControlPlane`](ortak_control::PgControlPlane) and the thin
//! [`WorkService`] in front of it:
//!
//! - create a project idempotently by slug;
//! - create a work item, or promote one from a decided Office message
//!   idempotently by `(company, message)`;
//! - assign active employees;
//! - add a same-project dependency with a transactional cycle check under
//!   the project row lock;
//! - move the item through the closed state machine with an
//!   `expected_version` compare-and-set;
//! - satisfy an acceptance criterion and resolve an approval gate;
//! - read one aggregate with its history and list a project's work in
//!   stable keyset order.
//!
//! Guarantees enforced here:
//!
//! - **Company scope is not a filter.** Every method takes the
//!   server-resolved [`CompanyScope`](ortak_control::CompanyScope); an
//!   unknown id and an id of another company are the same not-found error.
//! - **Facts are joined, never trusted.** Employees must exist and be
//!   `active` in the company; source messages must be `decided` inbox rows of
//!   the company; dependency targets must be items of the same company and
//!   project; attachment targets must exist in the company; employee actors
//!   must be active employees of the company.
//! - **One mutation, one version, one history event, one transaction.** The
//!   item row is locked, the caller's `expected_version` is compared, the
//!   pure domain command runs, the child rows and the item row are updated,
//!   and exactly one bounded typed history event is appended, all in one
//!   commit. The database guards (migration 0047) refuse any version step
//!   other than `+1`, any history gap, and any mutation of a terminal item.
//! - **Nothing is erased.** Cancel and archive are state changes; criteria,
//!   approvals, assignments, dependencies, attachments, and history rows are
//!   never deleted.
//!
//! Not in this crate: Work/Projects APIs and realtime projections, desktop
//! surfaces, dispatch-from-work, and artifact storage (see the Milestone 6
//! implementation state in `docs/ortak/IMPLEMENTATION_PLAN_V0.md`).

mod error;
pub mod model;
pub mod postgres;
pub mod repository;
pub mod reviewed_exports;
pub mod service;

pub use error::{Result, WorkError};
pub use model::{
    ProjectRecord, WorkHistoryRecord, WorkItemAggregate, WorkListCursor, WorkListPage,
    WorkListQuery, WorkSummary, DEFAULT_WORK_PAGE_SIZE, MAX_WORK_HISTORY_ROWS, MAX_WORK_PAGE_SIZE,
};
pub use repository::{
    AddDependency, ArchiveProject, AssignEmployee, AttachRecord, CreateProject, CreateWorkItem,
    ProjectCreation, ResolveApproval, SatisfyCriterion, TransitionWorkItem, WorkItemCreation,
    WorkRepository,
};
pub use service::WorkService;

pub use postgres::{
    ApiProject, ApiProjectCreation, ApiProjectPage, ApiWorkPrincipal, AuthorizedWork, ProjectRole,
    WorkExecutionReceipt, WorkMutation,
};

pub use postgres::{schedule_work_outputs, WorkOutputReport};
pub use postgres::{DependencyAction, WorkDependencyPage, WorkDependencyView};
pub use postgres::{EmployeeWorkQueueEntry, EmployeeWorkQueuePage};
pub use postgres::{WorkChildCreation, WorkDecomposition};
pub use postgres::{WorkExecutionView, WorkTextArtifact};

// Explicitly reviewed project context and retained stop-use receipts.
pub use postgres::{
    ReviewedFact, ReviewedFactDraft, ReviewedFactPage, ReviewedFactRecall, ReviewedFactReceipt,
    ReviewedFactSource,
};

// Explicit conversation review; approval remains separate from publication/use.
pub use postgres::{
    ConversationMemoryAudience, ReviewedConversationFact, ReviewedConversationFactDraft,
    ReviewedConversationFactPage, ReviewedConversationFactReceipt, ReviewedConversationPreview,
    ReviewedConversationPreviewRequest,
};
