//! Production-seam Postgres tests for run dispatch and supervision.
//!
//! Run with a disposable local database that can receive the embedded
//! migrations:
//! `ORTAK_TEST_DATABASE_URL=postgres://... cargo test -p ortak-runtime -- --ignored`

use std::time::Duration;

use chrono::Utc;
use ortak_control::fakes::FakeRuntimeAdapter;
use ortak_control::inbox::{InboxEvent, KIND_GIFT_WRAP, KIND_STREAM_MESSAGE};
use ortak_control::outbox::{OutboxFailOutcome, OutboxKind, OutboxLease};
use ortak_control::ports::{
    CompanyDirectory, InboxRepository, MessageNormalizer, Normalization, OutboxRepository,
    RoutingRepository,
};
use ortak_control::routing::{
    CandidateRevision, RosterScope, RoutingCommitOutcome, RoutingProposal,
};
use ortak_control::run_event::{
    BoundedText, DeliveryIntentKind, RedactionPolicy, RunEvent, RunEventPayload,
};
use ortak_control::runtime::{RunStartReceipt, RuntimeAdapter, RuntimeError, RuntimeRunRef};
use ortak_control::{CompanyScope, MessageId, PgControlPlane};
use ortak_domain::{
    ApprovalRequirement, Employee, EmployeeId, EmployeeManifest, EmployeeStatus, MessageOrigin,
    PermissionPolicy, RecipientAction, RecipientDecision, RoutingDecision, RoutingMode,
    RoutingPolicy, RoutingReason,
};
use ortak_runtime::{
    AppendOutcome, CancellationOutcome, CorrelationOutcome, DispatchAuthorization, DispatchOutcome,
    DispatchRefusal, PrepareOutcome, PumpOutcome, RunDispatchRepository, RunStatus,
    RunSupervisionError, RunSupervisor, SupervisorConfig,
};
use sha2::{Digest, Sha256};
use sqlx::{PgPool, Row};
use uuid::Uuid;

mod fixture;
use fixture::*;

mod authority;
mod cancellation;
mod cohort;
mod delivery_contention;
mod direct;
mod fencing;
mod lifecycle;
mod memory_context;
mod memory_output;
mod memory_snapshot;
mod office_output;
mod output_contention;
mod permissions;
mod reconciliation;
mod stop_contention;
mod supervision;
#[cfg(feature = "encrypted-dm")]
mod confidential;
