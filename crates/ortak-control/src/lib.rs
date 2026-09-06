#![deny(unsafe_code)]
#![warn(missing_docs)]
//! Ortak control plane: repository ports, the authoritative routing
//! transaction, the inbox routing service, and PostgreSQL adapters for the
//! Milestone 1 durable schema (migration 0045).
//!
//! Guarantees enforced here:
//!
//! - Company scope is derived from the authenticated community binding or the
//!   company registry ([`CompanyScope`] cannot be built from client input).
//! - Office inbox rows are inserted idempotently and claimed under a
//!   generation fence; only the generation that holds the claim can finalize.
//! - Exactly one dispatching decision exists per `(company, message)`; a replay
//!   or losing worker writes no second decision and no dispatch outbox row.
//! - The durable `delivery_chains` row is locked before any wake is reserved;
//!   sibling branches serialize on it, hop/wake budgets are spent under that
//!   lock, and `delivery_chain_visits` reserves each employee once per root.
//! - Decision, recipients, visit reservations, chain counters, inbox
//!   finalization, and run-dispatch outbox rows commit in one transaction.
//! - Semantic scoring runs outside every database transaction; the commit
//!   revalidates policy, candidate revisions, roster, and chain state first.
//!
//! Milestone 3–5 foundation (runtime, memory, observability, provisioning):
//!
//! - [`runtime`], [`memory`], and [`office_identity`] are adapter ports that
//!   see only opaque credential/endpoint references, never secret values.
//! - [`run_event`] normalizes runtime streams into bounded, redacted records
//!   that satisfy the `run_events` column contract before persistence.
//! - [`provisioning`] is a resumable saga over `provisioning_operations` and
//!   `provisioning_operation_steps`; adopted resources are never created,
//!   deleted, replaced, or activated by compensation, and activation requires
//!   every runtime, memory, Office membership, and signer gate to pass.
//! - [`fakes`] hosts in-memory adapters for tests and dry-run tooling; no
//!   module here connects to a real Hermes, Honcho, or Office deployment.

pub mod adapter;
pub mod cohort;
pub mod confidential;
pub mod credentials;
mod error;
pub mod fakes;
mod ids;
pub mod inbox;
pub mod memory;
pub mod memory_jobs;
pub mod office_authority;
pub mod office_identity;
pub mod outbox;
pub mod ports;
pub mod postgres;
pub mod provisioning;
pub mod routing;
pub mod run_event;
pub mod runtime;
pub mod scorer;
pub mod semantic;
pub mod service;
pub mod workspace;

pub use error::{ControlError, Result};
pub use ids::{ClaimGeneration, CompanyScope, MessageId};
pub use postgres::PgControlPlane;
pub use scorer::DisabledSemanticScorer;
pub use semantic::{ScoringBudget, SemanticScoringInput};
pub use service::{InboxRoutingService, RoutingWorkerConfig, ServiceOutcome};
