//! Explicit server-owned central-routing selection and durable capture progress.

use ortak_domain::EmployeeId;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// One company's current capture generation and dispatch state.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RoutingCohort {
    /// Company derived from the server scope.
    pub company_id: Uuid,
    /// Canonical Office community.
    pub community_id: Uuid,
    /// Changes whenever the capture selection or lifecycle is reset.
    pub capture_id: Uuid,
    /// `off`, `capture`, or `enabled`; absence of a cohort is also off.
    pub state: String,
    /// Exact selected channels, in stable order.
    pub channel_ids: Vec<Uuid>,
    /// Exact selected employees, in stable order.
    pub employee_ids: Vec<EmployeeId>,
}

/// Durable progress for a pinned stored-event scan in one selected channel.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct InboxReconciliationProgress {
    /// Cohort capture generation; old captures cannot enable a new selection.
    pub capture_id: Uuid,
    /// Canonical channel whose stored events were scanned.
    pub channel_id: Uuid,
    /// Total bounded-page rows examined, including already-present inbox rows.
    pub scanned: i64,
    /// Inbox rows inserted; replay never increases this for an existing row.
    pub inserted: i64,
    /// The pinned window was completely traversed and durably committed.
    pub completed: bool,
}

/// Maximum explicit channel and employee cohort size.
pub const MAX_ROUTING_COHORT_SIZE: usize = 64;
/// Maximum canonical event rows in one reconciliation transaction.
pub const MAX_INBOX_RECONCILIATION_BATCH: u16 = 256;
