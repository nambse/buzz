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
}
