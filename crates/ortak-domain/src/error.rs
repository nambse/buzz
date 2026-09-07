use thiserror::Error;

/// Validation failures produced by pure Ortak domain objects.
#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum DomainError {
    /// An employee identifier is empty or contains unsupported characters.
    #[error("invalid employee id: {0}")]
    InvalidEmployeeId(String),
    /// A required textual field is empty.
    #[error("{field} must not be empty")]
    EmptyField {
        /// Name of the invalid field.
        field: &'static str,
    },
    /// A field violates a bounded schema constraint; its value is never echoed.
    #[error("invalid or unbounded field: {field}")]
    InvalidField {
        /// Stable schema field name.
        field: &'static str,
    },
    /// Two employees claim the same normalized alias.
    #[error("employee alias '{alias}' belongs to both '{first}' and '{second}'")]
    AliasCollision {
        /// Conflicting normalized alias.
        alias: String,
        /// First employee identifier.
        first: String,
        /// Second employee identifier.
        second: String,
    },
    /// An employee identifier occurs more than once in a catalog.
    #[error("duplicate employee id: {0}")]
    DuplicateEmployeeId(String),
    /// A score or threshold is outside the inclusive zero-to-one range.
    #[error("{field} must be between 0 and 1")]
    InvalidScore {
        /// Name of the invalid field.
        field: &'static str,
    },
    /// A bounded policy value is zero or internally inconsistent.
    #[error("invalid routing policy: {0}")]
    InvalidRoutingPolicy(&'static str),
    /// Adopt mode requires a stable external profile reference.
    #[error("adopt mode requires runtime.profile_ref")]
    MissingAdoptProfile,
    /// Credential fields must contain opaque references rather than values.
    /// The rejected value is deliberately not retained or displayed.
    #[error("credential field must contain a safe opaque credential reference")]
    InvalidCredentialReference,
    /// Adapter options may not contain fields or values that look like secrets.
    #[error("{field} contains secret-like data; use credential_refs instead")]
    UnsafeAdapterOption {
        /// Option map containing the unsafe data. The key/value are not retained.
        field: &'static str,
    },
    /// An Office signing public key is malformed.
    #[error("office public key must be 64 hexadecimal characters")]
    InvalidOfficePublicKey,
    /// Semantic evidence must use a bounded stable-code grammar for safe audit/UI use.
    #[error("semantic evidence must contain at most 8 safe ASCII labels")]
    InvalidSemanticEvidence,
    /// A message lacks a stable identifier, actor/conversation identity, or text.
    #[error("invalid message field: {field}")]
    InvalidMessage {
        /// Stable field name; the rejected value is never retained.
        field: &'static str,
    },
    /// A manifest uses a schema this binary cannot interpret safely.
    #[error("unsupported employee manifest schema: {0}")]
    UnsupportedManifestSchema(String),
    /// A delivery chain counter cannot be advanced without overflowing.
    #[error("delivery chain counter overflow")]
    DeliveryChainOverflow,
    /// A project slug is empty or contains unsupported characters.
    #[error("project slug must match ^[a-z0-9][a-z0-9_-]{{0,63}}$")]
    InvalidProjectSlug,
    /// The project is archived and accepts no mutation.
    #[error("project is archived")]
    ProjectArchived,
    /// The work state machine does not allow this transition.
    #[error("work item cannot move from {from} to {to}")]
    InvalidWorkTransition {
        /// Current state.
        from: crate::WorkState,
        /// Requested state.
        to: crate::WorkState,
    },
    /// The work item is in a terminal state and accepts no mutation.
    #[error("work item is {state} and cannot change")]
    WorkItemTerminal {
        /// Terminal state.
        state: crate::WorkState,
    },
    /// Completion gates are not all satisfied.
    #[error("work item cannot complete: {} gate(s) block it", blockers.len())]
    CompletionBlocked {
        /// Every unsatisfied criterion and unapproved required gate.
        blockers: Vec<crate::CompletionBlocker>,
    },
    /// Work cannot start while a dependency is unfinished.
    #[error("work item is blocked by {count} unfinished dependency(ies)")]
    DependenciesUnresolved {
        /// Number of blocking dependencies.
        count: usize,
    },
    /// A work item cannot depend on itself.
    #[error("work item cannot depend on itself")]
    SelfDependency,
    /// The dependency would close a cycle in the project graph.
    #[error("dependency would create a cycle")]
    DependencyCycle,
    /// The dependency already exists.
    #[error("dependency already exists")]
    DuplicateWorkDependency,
    /// Only an existing active dependency can be removed.
    #[error("work dependency is not active")]
    UnknownWorkDependency,
    /// The criterion does not belong to the work item.
    #[error("unknown acceptance criterion")]
    UnknownCriterion,
    /// The criterion was already satisfied.
    #[error("acceptance criterion is already satisfied")]
    CriterionAlreadySatisfied,
    /// The approval gate does not belong to the work item.
    #[error("unknown approval gate")]
    UnknownApproval,
    /// The approval gate was already resolved.
    #[error("approval gate is already resolved")]
    ApprovalAlreadyResolved,
    /// The employee already holds an active assignment on the item.
    #[error("employee is already assigned")]
    DuplicateAssignment,
    /// Only an active assignment can be released or replaced.
    #[error("employee has no active assignment on this work item")]
    AssignmentNotActive,
    /// The record is already attached.
    #[error("record is already attached")]
    DuplicateAttachment,
}
