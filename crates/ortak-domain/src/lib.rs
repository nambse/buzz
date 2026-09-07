//! Pure domain types for the Ortak company workspace.
//!
//! This crate deliberately performs no network, database, process, or file I/O.

mod employee;
mod error;
mod message;
mod routing;
mod work;

pub use employee::{
    normalize_alias, ApprovalRequirement, CredentialRef, Employee, EmployeeCatalog, EmployeeId,
    EmployeeManifest, EmployeeRoutingPolicy, EmployeeStatus, MemoryBinding, OfficeBinding,
    PermissionPolicy, ProvisioningMode, RuntimeBinding, ToolCapability,
    EMPLOYEE_MANIFEST_SCHEMA_V0,
};
pub use error::DomainError;
pub use message::{
    ConversationContext, DeliveryChain, MessageEnvelope, MessageKind, MessageOrigin, ReplyContext,
};
pub use routing::{
    EvidenceLabel, RecipientAction, RecipientDecision, RoutingDecision, RoutingMode, RoutingPolicy,
    RoutingReason, SemanticScore, HARD_MAX_CHAIN_HOPS, HARD_MAX_CHAIN_WAKES,
    HARD_MAX_MESSAGE_BYTES, HARD_MAX_RECIPIENTS,
};
pub use work::{
    creates_dependency_cycle, AcceptanceCriterion, ApprovalDecision, ApprovalGate,
    ApprovalGateSpec, ApprovalStatus, Assignment, AssignmentRole, AssignmentStatus, AttachmentRef,
    CompletionBlocker, CriterionEdit, CriterionStatus, EditWorkDefinition, NewChildWork,
    NewProject, NewWorkItem, NewWorkItemIds, Project, ProjectEvent, ProjectSlug, ProjectStatus,
    WorkActor, WorkAttachment, WorkDependency, WorkEvent, WorkItem, WorkPriority, WorkState,
    MAX_WORK_APPROVALS, MAX_WORK_ASSIGNMENTS, MAX_WORK_ATTACHMENTS, MAX_WORK_CHILDREN,
    MAX_WORK_CRITERIA, MAX_WORK_CRITERION_BYTES, MAX_WORK_DEPENDENCIES, MAX_WORK_DEPTH,
    MAX_WORK_DESCRIPTION_BYTES, MAX_WORK_REASON_BYTES, MAX_WORK_REFERENCE_BYTES,
    MAX_WORK_TITLE_BYTES,
};
