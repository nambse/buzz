//! Dispatch authority derived from durable rows, and the pure validation
//! that turns a pinned revision into runtime configuration and bounded input.
//!
//! A leased `run_dispatch` outbox row carries a JSON payload written by the
//! routing commit. That payload is only a hint. Everything that reaches the
//! runtime is derived again from company-scoped rows and sealed into
//! [`DispatchAuthority`], which has no public constructor.

use std::collections::BTreeMap;
use std::fmt;

use ortak_control::run_event::strip_control_characters;
use ortak_control::runtime::{RunContext, RunSpec, RuntimeError, MAX_RUN_INPUT_BYTES};
use ortak_control::MessageId;
use ortak_domain::{
    CredentialRef, Employee, EmployeeId, EmployeeStatus, MemoryBinding, PermissionPolicy,
    RuntimeBinding,
};
use uuid::Uuid;

/// Why a leased dispatch cannot start a run. Every variant is bounded and
/// carries identifiers or closed-vocabulary values only.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DispatchRefusal {
    /// A durable stop request fences any subsequent start attempt.
    CancellationRequested,
    /// The company was suspended after the scope was resolved.
    CompanyNotActive,
    /// Canonical Office authorization changed since the committed decision.
    OfficeAuthorityChanged,
    /// Current Work project, source, assignment or execution version changed.
    WorkAuthorityChanged,
    /// A disable invalidated the employee lifecycle pinned by this work.
    EmployeeLifecycleChanged,
    /// The outbox row names no routing decision.
    DecisionMissing,
    /// The decision has no recipient row for the employee.
    RecipientMissing,
    /// The recipient row is not a `wake`.
    RecipientNotWake {
        /// Recipient action found.
        action: String,
    },
    /// The recipient row pins no employee revision.
    RecipientRevisionUnpinned,
    /// No delivery-chain visit reservation exists for the recipient.
    VisitMissing,
    /// The inbox row is missing or not `decided`.
    InboxNotDecided {
        /// Inbox state found, if any.
        state: Option<String>,
    },
    /// The employee row is missing.
    EmployeeMissing,
    /// The employee lifecycle status does not accept work.
    EmployeeNotActive {
        /// Durable status.
        status: EmployeeStatus,
    },
    /// The pinned revision row does not exist for this employee.
    RevisionMissing,
    /// The pinned revision manifest cannot be read as an employee definition
    /// for this employee.
    ManifestUnreadable,
    /// The pinned revision has no `employee_runtime_bindings` row.
    RuntimeBindingMissing,
    /// The runtime binding row was never validated by its adapter.
    RuntimeBindingUnvalidated,
    /// The runtime binding row disagrees with the manifest.
    RuntimeBindingMismatch {
        /// Field that differs.
        field: &'static str,
    },
    /// A required memory binding row is missing.
    MemoryBindingMissing,
    /// The exact memory binding has not passed resource validation.
    MemoryBindingUnvalidated,
    /// Pinned, stored, and active memory identities disagree.
    MemoryBindingChanged,
    /// No configured adapter can serve the required memory binding.
    MemoryAdapterUnavailable,
    /// Memory health or bounded recall failed.
    MemoryUnavailable,
    /// Recalled records or the durable snapshot violate their scope or bounds.
    MemoryContextRejected,
    /// The binding names a different adapter than the one dispatching.
    AdapterMismatch {
        /// Adapter the binding names.
        expected: String,
        /// Adapter available.
        found: String,
    },
    /// The signed Office event behind the inbox row is not readable.
    MessageUnavailable,
    /// The signed Office event was deleted.
    MessageDeleted,
    /// The inbox row's kind is not a plaintext channel text kind; its
    /// content (for a gift wrap, ciphertext) is never handed to a runtime.
    UnsupportedMessageKind {
        /// Kind the inbox row carries.
        kind: i32,
    },
    /// The inbox row is not channel-scoped, so it cannot be a channel run.
    MessageChannelMissing,
    /// The inbox row disagrees with the canonical signed event on a fact
    /// the run input depends on.
    MessageProvenanceMismatch {
        /// Fact that disagreed.
        field: &'static str,
    },
    /// The message has no routable text after bounding.
    EmptyMessage,
}

impl fmt::Display for DispatchRefusal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CancellationRequested => formatter.write_str("run cancellation requested"),
            Self::CompanyNotActive => formatter.write_str("company is not active"),
            Self::OfficeAuthorityChanged => formatter.write_str("Office authorization changed"),
            Self::WorkAuthorityChanged => formatter.write_str("Work authorization changed"),
            Self::EmployeeLifecycleChanged => formatter.write_str("employee lifecycle changed"),
            Self::DecisionMissing => formatter.write_str("routing decision missing"),
            Self::RecipientMissing => formatter.write_str("routing recipient missing"),
            Self::RecipientNotWake { action } => {
                write!(formatter, "routing recipient action is {action}, not wake")
            }
            Self::RecipientRevisionUnpinned => {
                formatter.write_str("routing recipient pins no employee revision")
            }
            Self::VisitMissing => formatter.write_str("delivery chain visit missing"),
            Self::InboxNotDecided { state } => {
                write!(formatter, "inbox row is {state:?}, not decided")
            }
            Self::EmployeeMissing => formatter.write_str("employee row missing"),
            Self::EmployeeNotActive { status } => {
                write!(formatter, "employee status is {status:?}, not active")
            }
            Self::RevisionMissing => formatter.write_str("pinned employee revision missing"),
            Self::ManifestUnreadable => formatter.write_str("pinned revision manifest unreadable"),
            Self::RuntimeBindingMissing => formatter.write_str("runtime binding row missing"),
            Self::RuntimeBindingUnvalidated => formatter.write_str("runtime binding not validated"),
            Self::RuntimeBindingMismatch { field } => {
                write!(
                    formatter,
                    "runtime binding {field} differs from the manifest"
                )
            }
            Self::MemoryBindingMissing => formatter.write_str("memory binding row missing"),
            Self::MemoryBindingUnvalidated => formatter.write_str("memory binding not validated"),
            Self::MemoryBindingChanged => formatter.write_str("memory binding identity changed"),
            Self::MemoryAdapterUnavailable => formatter.write_str("memory adapter unavailable"),
            Self::MemoryUnavailable => formatter.write_str("memory health or recall unavailable"),
            Self::MemoryContextRejected => formatter.write_str("memory context rejected"),
            Self::AdapterMismatch { expected, found } => {
                write!(formatter, "binding adapter {expected} is not {found}")
            }
            Self::MessageUnavailable => formatter.write_str("office message unavailable"),
            Self::MessageDeleted => formatter.write_str("office message deleted"),
            Self::UnsupportedMessageKind { kind } => {
                write!(
                    formatter,
                    "office message kind {kind} is not a channel text kind"
                )
            }
            Self::MessageChannelMissing => formatter.write_str("office message has no channel"),
            Self::MessageProvenanceMismatch { field } => {
                write!(
                    formatter,
                    "inbox {field} disagrees with the canonical office event"
                )
            }
            Self::EmptyMessage => formatter.write_str("office message has no text"),
        }
    }
}

/// Bounded run input derived from the canonical Office event.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RunInput {
    /// Message text, control-stripped and bounded to [`MAX_RUN_INPUT_BYTES`].
    pub body: String,
    /// True when the original text exceeded the ceiling.
    pub truncated: bool,
    /// Office channel the message was posted to, when channel-scoped.
    pub channel_id: Option<Uuid>,
    /// Nostr event kind of the message.
    pub event_kind: i32,
}

/// `employee_runtime_bindings` row as read for the pinned revision.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StoredRuntimeBinding {
    /// Adapter column.
    pub adapter: String,
    /// External profile reference.
    pub profile_ref: Option<String>,
    /// Model column.
    pub model: String,
    /// Workspace column.
    pub workspace_ref: String,
    /// Credential reference strings.
    pub credential_refs: Vec<String>,
    /// Non-secret options.
    pub options: BTreeMap<String, String>,
    /// Whether the adapter validated the binding at activation.
    pub validated: bool,
}

/// Memory row belonging to the same pinned employee revision.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StoredMemoryBinding {
    /// Complete secret-free binding read from durable columns.
    pub binding: MemoryBinding,
    /// Whether resource validation was recorded for this revision.
    pub validated: bool,
}

/// Runtime configuration validated together from one pinned revision manifest.
/// No public constructor permits mixing a binding and another revision's policy.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ValidatedRunConfiguration {
    binding: RuntimeBinding,
    permissions: PermissionPolicy,
    memory: Option<MemoryBinding>,
    memory_validated: bool,
}

impl ValidatedRunConfiguration {
    /// Runtime binding from the validated manifest.
    pub fn binding(&self) -> &RuntimeBinding {
        &self.binding
    }

    /// Complete memory identity pinned by the same employee manifest.
    pub fn memory_binding(&self) -> Option<&MemoryBinding> {
        self.memory.as_ref()
    }

    /// Validates the stored memory row and the employee's live memory identity.
    /// Unlike policy, a retired memory workspace must not remain executable.
    pub fn with_validated_memory(
        mut self,
        stored: Option<&StoredMemoryBinding>,
        active: Option<&MemoryBinding>,
    ) -> std::result::Result<Self, DispatchRefusal> {
        match self.memory.as_ref() {
            Some(binding) => {
                let stored = stored.ok_or(DispatchRefusal::MemoryBindingMissing)?;
                if !stored.validated {
                    return Err(DispatchRefusal::MemoryBindingUnvalidated);
                }
                if &stored.binding != binding || active != Some(binding) {
                    return Err(DispatchRefusal::MemoryBindingChanged);
                }
            }
            None if stored.is_some() || active.is_some() => {
                return Err(DispatchRefusal::MemoryBindingChanged);
            }
            None => {}
        }
        self.memory_validated = true;
        Ok(self)
    }

    /// Structurally validated permission policy from the same manifest.
    pub fn permissions(&self) -> &PermissionPolicy {
        &self.permissions
    }
}

/// Validates the pinned revision against its lifecycle and binding rows and
/// returns the runtime binding and permission policy the run must use.
///
/// The manifest is parsed as a full [`Employee`], must describe
/// `employee_id`, and must pass definition validation; the stored binding row
/// must exist, be validated, and match the manifest field by field. The
/// revision does not have to be the employee's active one: the routing
/// decision's pinned revision is authoritative.
pub fn validate_pinned_revision(
    employee_id: &EmployeeId,
    status: EmployeeStatus,
    manifest: &serde_json::Value,
    stored: Option<&StoredRuntimeBinding>,
) -> std::result::Result<ValidatedRunConfiguration, DispatchRefusal> {
    if status != EmployeeStatus::Active {
        return Err(DispatchRefusal::EmployeeNotActive { status });
    }
    let employee: Employee = serde_json::from_value(manifest.clone())
        .map_err(|_| DispatchRefusal::ManifestUnreadable)?;
    if &employee.id != employee_id || employee.validate_definition().is_err() {
        return Err(DispatchRefusal::ManifestUnreadable);
    }
    let stored = stored.ok_or(DispatchRefusal::RuntimeBindingMissing)?;
    if !stored.validated {
        return Err(DispatchRefusal::RuntimeBindingUnvalidated);
    }
    let binding = employee.runtime;
    let mismatch = |field| DispatchRefusal::RuntimeBindingMismatch { field };
    if stored.adapter != binding.adapter {
        return Err(mismatch("adapter"));
    }
    if stored.profile_ref != binding.profile_ref {
        return Err(mismatch("profile_ref"));
    }
    if stored.model != binding.model {
        return Err(mismatch("model"));
    }
    if stored.workspace_ref != binding.workspace_ref {
        return Err(mismatch("workspace_ref"));
    }
    let manifest_refs = binding
        .credential_refs
        .iter()
        .map(CredentialRef::as_str)
        .collect::<Vec<_>>();
    if stored.credential_refs != manifest_refs {
        return Err(mismatch("credential_refs"));
    }
    if stored.options != binding.options {
        return Err(mismatch("options"));
    }
    Ok(ValidatedRunConfiguration {
        binding,
        permissions: employee.permissions,
        memory_validated: employee.memory.is_none(),
        memory: employee.memory,
    })
}

/// Bounds the canonical message text for the runtime: control characters
/// (other than newline, carriage return, and tab) are stripped, the text is
/// cut at [`MAX_RUN_INPUT_BYTES`] on a character boundary, and an empty
/// result is refused.
pub fn bound_message_text(content: &str) -> std::result::Result<(String, bool), DispatchRefusal> {
    let cleaned = strip_control_characters(content);
    let truncated = cleaned.len() > MAX_RUN_INPUT_BYTES;
    let body =
        ortak_control::adapter::truncate_at_char_boundary(&cleaned, MAX_RUN_INPUT_BYTES).to_owned();
    if body.trim().is_empty() {
        return Err(DispatchRefusal::EmptyMessage);
    }
    Ok((body, truncated))
}

/// Stable runtime idempotency key for one durable run. A retried start in a
/// fresh process derives the same key, so the runtime returns the same run.
pub fn run_idempotency_key(company_id: Uuid, run_id: Uuid) -> String {
    format!("ortak-run:{company_id}:{run_id}")
}

/// Everything a dispatch needs, derived solely from company-scoped durable
/// rows by [`RunDispatchRepository`](crate::repository::RunDispatchRepository).
///
/// There is no public constructor and every field is read-only, so a caller
/// cannot present identity, revision, message, binding, or permission values of its own
/// to run creation, runtime start, or correlation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DispatchAuthority {
    office_authority: Option<ortak_control::office_authority::OfficeAuthority>,
    company_id: Uuid,
    outbox_id: Uuid,
    lease_token: Uuid,
    routing_decision_id: Option<Uuid>,
    employee_id: EmployeeId,
    employee_revision_id: Uuid,
    message_id: Option<MessageId>,
    root_message_id: Option<MessageId>,
    work: Option<WorkRunOrigin>,
    work_generation: Option<i64>,
    configuration: ValidatedRunConfiguration,
    input: RunInput,
}

/// Immutable provenance of a human-requested Work run, never dispatch authority itself.
#[derive(Clone, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct WorkRunOrigin {
    /// Durable run created by the request transaction.
    pub run_id: Uuid,
    /// Owning Work item.
    pub work_item_id: Uuid,
    /// Owning project.
    pub project_id: Uuid,
    /// Work version created by execution request.
    pub execution_version: i64,
    /// Digest of immutable canonical definition bytes.
    pub definition_hash: String,
}

impl DispatchAuthority {
    /// Crate-private constructor for the repository seam.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        company_id: Uuid,
        outbox_id: Uuid,
        lease_token: Uuid,
        routing_decision_id: Uuid,
        employee_id: EmployeeId,
        employee_revision_id: Uuid,
        message_id: MessageId,
        root_message_id: MessageId,
        configuration: ValidatedRunConfiguration,
        input: RunInput,
    ) -> Self {
        Self {
            office_authority: None,
            company_id,
            outbox_id,
            lease_token,
            routing_decision_id: Some(routing_decision_id),
            employee_id,
            employee_revision_id,
            message_id: Some(message_id),
            root_message_id: Some(root_message_id),
            work: None,
            work_generation: None,
            configuration,
            input,
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn from_work(
        company_id: Uuid,
        outbox_id: Uuid,
        lease_token: Uuid,
        employee_id: EmployeeId,
        employee_revision_id: Uuid,
        configuration: ValidatedRunConfiguration,
        input: RunInput,
        work: WorkRunOrigin,
        generation: i64,
    ) -> Self {
        Self {
            office_authority: None,
            company_id,
            outbox_id,
            lease_token,
            routing_decision_id: None,
            employee_id,
            employee_revision_id,
            message_id: None,
            root_message_id: None,
            work: Some(work),
            work_generation: Some(generation),
            configuration,
            input,
        }
    }

    /// Work execution provenance, absent for conversational routing.
    pub fn work_origin(&self) -> Option<&WorkRunOrigin> {
        self.work.as_ref()
    }

    pub(crate) fn work_generation(&self) -> Option<i64> {
        self.work_generation
    }

    /// Company boundary.
    pub fn company_id(&self) -> Uuid {
        self.company_id
    }

    pub(crate) fn with_office_authority(
        mut self,
        authority: ortak_control::office_authority::OfficeAuthority,
    ) -> Self {
        self.office_authority = Some(authority);
        self
    }

    pub(crate) fn office_authority(
        &self,
    ) -> Option<&ortak_control::office_authority::OfficeAuthority> {
        self.office_authority.as_ref()
    }

    /// Outbox row the lease was verified against.
    pub fn outbox_id(&self) -> Uuid {
        self.outbox_id
    }

    /// Lease token observed on the row; every later write is fenced by it.
    pub fn lease_token(&self) -> Uuid {
        self.lease_token
    }

    /// Routing decision that woke the employee.
    pub fn routing_decision_id(&self) -> Option<Uuid> {
        self.routing_decision_id
    }

    /// Recipient employee, from the outbox and recipient rows.
    pub fn employee_id(&self) -> &EmployeeId {
        &self.employee_id
    }

    /// Revision pinned by the routing recipient row.
    pub fn employee_revision_id(&self) -> Uuid {
        self.employee_revision_id
    }

    /// Triggering message, from the decision row.
    pub fn message_id(&self) -> Option<MessageId> {
        self.message_id
    }

    /// Delivery-chain root, from the decision row.
    pub fn root_message_id(&self) -> Option<MessageId> {
        self.root_message_id
    }

    /// Runtime binding from the validated revision manifest.
    pub fn binding(&self) -> &RuntimeBinding {
        self.configuration.binding()
    }

    /// Permission policy from the same validated, pinned revision manifest.
    pub fn permissions(&self) -> &PermissionPolicy {
        self.configuration.permissions()
    }

    /// Complete memory identity from the same pinned revision.
    pub fn memory_binding(&self) -> Option<&MemoryBinding> {
        self.configuration.memory_binding()
    }

    pub(crate) fn require_validated_memory(&self) -> std::result::Result<(), DispatchRefusal> {
        if !self.configuration.memory_validated {
            return Err(DispatchRefusal::MemoryBindingUnvalidated);
        }
        Ok(())
    }

    /// Bounded input derived from the canonical Office event.
    pub fn input(&self) -> &RunInput {
        &self.input
    }

    /// Builds the validated runtime spec for the durable run `run_id`.
    pub fn run_spec(&self, run_id: Uuid) -> std::result::Result<RunSpec, RuntimeError> {
        let spec = RunSpec {
            run_id,
            employee_id: self.employee_id.clone(),
            revision_id: self.employee_revision_id,
            binding: self.binding().clone(),
            permissions: self.permissions().clone(),
            input: self.input.body.clone(),
            context: RunContext {
                conversation_ref: self
                    .input
                    .channel_id
                    .filter(|_| self.work.is_none())
                    .map(|channel| channel.to_string()),
                reply_to_message_id: self.message_id.map(|id| id.to_hex()),
                work_item_id: self.work.as_ref().map(|work| work.work_item_id),
                memory_context: Vec::new(),
            },
            idempotency_key: run_idempotency_key(self.company_id, run_id),
        };
        spec.validate()?;
        Ok(spec)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use ortak_domain::{EmployeeId, EmployeeManifest, EmployeeStatus, PermissionPolicy};
    use uuid::Uuid;

    use super::{
        bound_message_text, run_idempotency_key, validate_pinned_revision, DispatchAuthority,
        DispatchRefusal, RunInput, StoredRuntimeBinding,
    };

    fn manifest() -> (EmployeeId, serde_json::Value, StoredRuntimeBinding) {
        let yaml = std::fs::read_to_string(format!(
            "{}/../../config/employees/cem.yaml",
            env!("CARGO_MANIFEST_DIR")
        ))
        .expect("read fixture");
        let manifest: EmployeeManifest = serde_yaml::from_str(&yaml).expect("parse fixture");
        let mut employee = manifest.employee;
        employee.status = EmployeeStatus::Active;
        let stored = StoredRuntimeBinding {
            adapter: employee.runtime.adapter.clone(),
            profile_ref: employee.runtime.profile_ref.clone(),
            model: employee.runtime.model.clone(),
            workspace_ref: employee.runtime.workspace_ref.clone(),
            credential_refs: employee
                .runtime
                .credential_refs
                .iter()
                .map(|reference| reference.as_str().to_owned())
                .collect(),
            options: employee.runtime.options.clone(),
            validated: true,
        };
        let id = employee.id.clone();
        (
            id,
            serde_json::to_value(&employee).expect("manifest json"),
            stored,
        )
    }

    #[test]
    fn matching_manifest_yields_binding_and_permissions_and_rejects_invalid_specs() {
        let (id, manifest, stored) = manifest();
        let configuration =
            validate_pinned_revision(&id, EmployeeStatus::Active, &manifest, Some(&stored))
                .expect("valid");
        let binding = configuration.binding();
        assert_eq!(binding.adapter, "hermes");
        assert_eq!(
            binding.profile_ref.as_deref(),
            Some("/opt/data/profiles/cem")
        );
        assert_eq!(
            serde_json::to_value(configuration.permissions()).expect("permissions json"),
            manifest["permissions"]
        );
        let authority = DispatchAuthority::new(
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            id,
            Uuid::new_v4(),
            ortak_control::MessageId::from_bytes([1; 32]),
            ortak_control::MessageId::from_bytes([1; 32]),
            configuration,
            RunInput {
                body: "Cem, selam".to_owned(),
                truncated: false,
                channel_id: Some(Uuid::new_v4()),
                event_kind: 9,
            },
        );
        let mut spec = authority.run_spec(Uuid::new_v4()).expect("valid spec");
        assert_eq!(&spec.permissions, authority.permissions());
        spec.permissions = PermissionPolicy::default();
        spec.validate().expect("empty policy is structurally valid");
        spec.permissions.allowed_networks = vec!["private-policy-value\n".to_owned()];
        assert!(matches!(
            spec.validate(),
            Err(ortak_control::runtime::RuntimeError::InvalidSpec { detail })
                if detail.to_string() == "run permission policy is invalid"
        ));
    }

    #[test]
    fn lifecycle_binding_and_identity_mismatches_are_refused() {
        let (id, manifest, stored) = manifest();
        assert_eq!(
            validate_pinned_revision(&id, EmployeeStatus::Disabled, &manifest, Some(&stored)),
            Err(DispatchRefusal::EmployeeNotActive {
                status: EmployeeStatus::Disabled
            })
        );
        assert_eq!(
            validate_pinned_revision(&id, EmployeeStatus::Paused, &manifest, Some(&stored)),
            Err(DispatchRefusal::EmployeeNotActive {
                status: EmployeeStatus::Paused
            })
        );
        assert_eq!(
            validate_pinned_revision(&id, EmployeeStatus::Active, &manifest, None),
            Err(DispatchRefusal::RuntimeBindingMissing)
        );
        let unvalidated = StoredRuntimeBinding {
            validated: false,
            ..stored.clone()
        };
        assert_eq!(
            validate_pinned_revision(&id, EmployeeStatus::Active, &manifest, Some(&unvalidated)),
            Err(DispatchRefusal::RuntimeBindingUnvalidated)
        );
        let drifted = StoredRuntimeBinding {
            model: "other-model".to_owned(),
            ..stored.clone()
        };
        assert_eq!(
            validate_pinned_revision(&id, EmployeeStatus::Active, &manifest, Some(&drifted)),
            Err(DispatchRefusal::RuntimeBindingMismatch { field: "model" })
        );
        let mut foreign_options = stored.clone();
        foreign_options.options = BTreeMap::new();
        assert_eq!(
            validate_pinned_revision(
                &id,
                EmployeeStatus::Active,
                &manifest,
                Some(&foreign_options)
            ),
            Err(DispatchRefusal::RuntimeBindingMismatch { field: "options" })
        );
        let other = EmployeeId::parse("zeynep").expect("id");
        assert_eq!(
            validate_pinned_revision(&other, EmployeeStatus::Active, &manifest, Some(&stored)),
            Err(DispatchRefusal::ManifestUnreadable)
        );
        assert_eq!(
            validate_pinned_revision(
                &id,
                EmployeeStatus::Active,
                &serde_json::json!({"id": "cem"}),
                Some(&stored)
            ),
            Err(DispatchRefusal::ManifestUnreadable)
        );
    }

    #[test]
    fn message_text_is_bounded_and_control_free() {
        let (body, truncated) = bound_message_text("Cem,\u{0} selam\tnasılsın?\n").expect("text");
        assert_eq!(body, "Cem, selam\tnasılsın?\n");
        assert!(!truncated);
        let long = "é".repeat(40 * 1024);
        let (body, truncated) = bound_message_text(&long).expect("text");
        assert!(truncated);
        assert!(body.len() <= ortak_control::runtime::MAX_RUN_INPUT_BYTES);
        assert_eq!(
            bound_message_text(" \u{1} \n"),
            Err(DispatchRefusal::EmptyMessage)
        );
    }

    #[test]
    fn idempotency_key_is_stable_per_run() {
        let company = Uuid::new_v4();
        let run = Uuid::new_v4();
        assert_eq!(
            run_idempotency_key(company, run),
            run_idempotency_key(company, run)
        );
        assert_ne!(
            run_idempotency_key(company, run),
            run_idempotency_key(company, Uuid::new_v4())
        );
    }
}
