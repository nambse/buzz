use super::*;
use serde::Deserialize;

/// Explicit worker destination/target selection; default recipes have none.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct EmployeeReviewedDestination {
    /// Existing retained employee-owned namespace target.
    pub target_id: Uuid,
    /// Exact Office destination; never an employee-global audience.
    pub destination_channel_id: Uuid,
}

/// Selected immutable commitment and provenance, with no locally stored text.
#[derive(Clone)]
pub struct ReviewedEmployeeSelectionPin {
    /// Exact approval, namespace and current epochs.
    pub pin: ReviewedEmployeePin,
    /// Canonical retained v1 sharing provenance string.
    pub provenance: String,
}

/// Canonical current selection for a bounded remote employee-owned read.
#[derive(Clone)]
pub struct ReviewedEmployeeSelection {
    /// Authenticated deployment company.
    pub company_id: Uuid,
    /// Stable employee owner.
    pub employee_id: ortak_domain::EmployeeId,
    /// Exact run-pinned full memory binding.
    pub binding: ortak_domain::MemoryBinding,
    /// Explicit target and destination, matched to configured owned resources.
    pub destination: EmployeeReviewedDestination,
    /// Exact retained target deployment, independent of employee model revision.
    pub deployment_id: Uuid,
    /// Retained original ownership receipt plus employee protocol/namespace pins.
    pub creation_receipt: serde_json::Value,
    /// Actual current human and source, derived before remote I/O.
    pub origin: EmployeeMemoryOrigin,
    /// Relationship before experience; at most eight exact remote IDs.
    pub records: Vec<ReviewedEmployeeSelectionPin>,
    /// Candidate or byte bounds omitted current records.
    pub truncated: bool,
}

/// Actual remote set; an empty set retains the legacy snapshot path.
#[derive(Clone, Default)]
pub struct ReviewedEmployeeRecall {
    /// Exact selected remote records, preserving the admitted priority order.
    pub records: Vec<ReviewedEmployeeRecord>,
    /// Remote bounds omitted eligible records.
    pub truncated: bool,
}
