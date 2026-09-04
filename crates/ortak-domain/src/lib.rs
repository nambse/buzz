//! Pure domain types for the Ortak company workspace.
//!
//! This crate deliberately performs no network, database, process, or file I/O.

mod employee;
mod error;
mod message;
mod routing;

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
