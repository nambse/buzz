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

mod error;
mod ids;
pub mod inbox;
pub mod outbox;
pub mod ports;
pub mod postgres;
pub mod routing;
pub mod service;

pub use error::{ControlError, Result};
pub use ids::{ClaimGeneration, CompanyScope, MessageId};
pub use postgres::PgControlPlane;
pub use service::{InboxRoutingService, RoutingWorkerConfig, ServiceOutcome};
