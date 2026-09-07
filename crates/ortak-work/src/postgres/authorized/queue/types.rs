use super::*;
use serde::Serialize;

/// One manual Work item assigned to the requested employee.
/// The assignment primary key guarantees one entry and one role per item/employee.
#[derive(Clone, Debug, Serialize)]
pub struct EmployeeWorkQueueEntry {
    /// Authorized summary; no runtime, artifact, or raw history fields.
    pub work: WorkSummary,
    /// Current active assignment role, distinct from human project permissions.
    pub assignment_role: AssignmentRole,
}
/// One bounded, authorized page of outstanding manual assignments.
#[derive(Clone, Debug, Serialize)]
pub struct EmployeeWorkQueuePage {
    /// Server-configured employee whose assignments were requested.
    pub employee_id: EmployeeId,
    /// At most 25 nonterminal items in active projects, newest first.
    pub items: Vec<EmployeeWorkQueueEntry>,
    /// Scope-bound continuation; every page reauthorizes its current rows.
    pub next_cursor: Option<String>,
}
