use ortak_domain::{
    ApprovalDecision, ApprovalGateSpec, AssignmentRole, EmployeeId, NewProject, NewWorkItem,
    WorkPriority, WorkState,
};
use serde::Deserialize;
use uuid::Uuid;

use crate::error::{ApiError, Result};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct CreateProject {
    pub operation_id: Uuid,
    pub channel_id: Uuid,
    pub project: NewProject,
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PageQuery {
    pub cursor: Option<String>,
    pub limit: Option<u32>,
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct WorkQuery {
    pub cursor: Option<String>,
    pub limit: Option<u32>,
    pub state: Option<WorkState>,
}

// Project and actor are derived from the route/principal, never this body.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct CreateWork {
    pub operation_id: Uuid,
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub priority: WorkPriority,
    #[serde(default)]
    pub criteria: Vec<String>,
    #[serde(default)]
    pub approvals: Vec<ApprovalGateSpec>,
    pub source_message_id: Option<String>,
}

impl CreateWork {
    pub fn input(self, project_id: Uuid, promotion: bool) -> Result<(Uuid, NewWorkItem)> {
        if self.operation_id.is_nil()
            || self.criteria.len() > 16
            || self.approvals.len() > 8
            || self.source_message_id.is_some() != promotion
        {
            return Err(ApiError::invalid());
        }
        Ok((
            self.operation_id,
            NewWorkItem {
                project_id,
                title: self.title,
                description: self.description,
                priority: self.priority,
                criteria: self.criteria,
                approvals: self.approvals,
                source_message_id: self.source_message_id,
            },
        ))
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Assignment {
    pub operation_id: Uuid,
    pub expected_version: i64,
    pub employee_id: EmployeeId,
    pub role: AssignmentRole,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Transition {
    pub operation_id: Uuid,
    pub expected_version: i64,
    pub target: WorkState,
    pub reason: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Criterion {
    pub operation_id: Uuid,
    pub expected_version: i64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Approval {
    pub operation_id: Uuid,
    pub expected_version: i64,
    pub decision: ApprovalDecision,
    pub reason: Option<String>,
}
