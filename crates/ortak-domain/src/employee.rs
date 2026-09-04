use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use serde::{Deserialize, Serialize};
use unicode_normalization::UnicodeNormalization;

use crate::DomainError;

/// Stable, human-readable identifier for an employee.
#[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(try_from = "String", into = "String")]
pub struct EmployeeId(String);

impl EmployeeId {
    /// Parses and validates a stable employee identifier.
    pub fn parse(value: impl Into<String>) -> Result<Self, DomainError> {
        let value = value.into();
        let valid = !value.is_empty()
            && value.len() <= 64
            && value
                .chars()
                .enumerate()
                .all(|(index, character)| match character {
                    'a'..='z' | '0'..='9' => true,
                    '-' | '_' => index > 0,
                    _ => false,
                });

        if valid {
            Ok(Self(value))
        } else {
            Err(DomainError::InvalidEmployeeId(value))
        }
    }

    /// Returns the identifier as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for EmployeeId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl TryFrom<String> for EmployeeId {
    type Error = DomainError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::parse(value)
    }
}

impl From<EmployeeId> for String {
    fn from(value: EmployeeId) -> Self {
        value.0
    }
}

/// Lifecycle state that controls whether an employee can receive work.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EmployeeStatus {
    /// Definition exists but is not available for work.
    Draft,
    /// Employee is available for policy-controlled routing.
    Active,
    /// Employee is temporarily excluded from new work.
    Paused,
    /// Employee is administratively disabled.
    Disabled,
}

/// Whether provisioning creates a resource or adopts an existing one.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProvisioningMode {
    /// Create a new external resource through its adapter.
    Create,
    /// Attach to an existing external resource without owning its deletion.
    Adopt,
}

/// Opaque pointer to credential-manager material; never a credential value.
#[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(try_from = "String", into = "String")]
pub struct CredentialRef(String);

impl CredentialRef {
    /// Parses a conservative secret-manager reference without retaining invalid input.
    pub fn parse(value: impl AsRef<str>) -> Result<Self, DomainError> {
        let value = value.as_ref();
        let locator = value
            .strip_prefix("credential://")
            .or_else(|| value.strip_prefix("secret://"))
            .filter(|locator| !locator.is_empty() && locator.len() <= 512)
            .filter(|locator| {
                locator.chars().all(|character| {
                    character.is_ascii_alphanumeric()
                        || matches!(character, '-' | '_' | '.' | '/' | ':' | '@' | '#')
                })
            })
            .filter(|locator| {
                locator
                    .split('/')
                    .all(|segment| !segment.is_empty() && segment != "." && segment != "..")
            });

        if locator.is_some() {
            Ok(Self(value.to_owned()))
        } else {
            Err(DomainError::InvalidCredentialReference)
        }
    }

    /// Returns the opaque reference for an authorized adapter resolver.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for CredentialRef {
    type Error = DomainError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::parse(value)
    }
}

impl From<CredentialRef> for String {
    fn from(value: CredentialRef) -> Self {
        value.0
    }
}

/// Runtime configuration that is independent from any concrete provider SDK.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeBinding {
    /// Runtime adapter name, such as `hermes`.
    pub adapter: String,
    /// External profile reference; required for adopted resources.
    pub profile_ref: Option<String>,
    /// Provider model reference used by the runtime.
    pub model: String,
    /// External workspace or checkout reference.
    pub workspace_ref: String,
    /// Opaque secret-manager references, never credential values.
    #[serde(default)]
    pub credential_refs: Vec<CredentialRef>,
    /// Adapter-specific non-secret options.
    #[serde(default)]
    pub options: BTreeMap<String, String>,
}

/// Durable memory configuration behind the Ortak memory port.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryBinding {
    /// Memory adapter name, such as `honcho`.
    pub adapter: String,
    /// Service-discovery or configuration reference for the endpoint.
    pub endpoint_ref: String,
    /// Memory workspace or namespace.
    pub workspace: String,
    /// Stable peer identifier representing the human operator.
    pub user_peer: String,
    /// Stable peer identifier representing the employee.
    pub employee_peer: String,
    /// Adapter-specific non-secret options.
    #[serde(default)]
    pub options: BTreeMap<String, String>,
}

/// Office identity and default placement for an employee.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct OfficeBinding {
    /// Public signing identity. Private key material is never part of a manifest.
    pub public_key: String,
    /// Opaque reference resolved by the Office signer adapter.
    pub signer_ref: CredentialRef,
    /// Optional stable reference to a default Office channel.
    pub home_channel_ref: Option<String>,
}

/// Tool capability identifiers supported by the v0 policy registry.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolCapability {
    /// Read and publish through policy-controlled Office actions.
    Office,
    /// Use the web tool subject to network policy.
    Web,
    /// Read or edit allowed workspace files.
    Files,
    /// Run commands in an allowed workspace.
    Terminal,
}

/// Operations that always require a human approval gate in v0.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalRequirement {
    /// Publish content outside the company boundary.
    ExternalPublish,
    /// Change a credential binding or secret reference.
    CredentialChange,
    /// Perform a destructive filesystem operation.
    DestructiveFileOperation,
}

/// Tool and workspace boundaries applied before and during a run.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PermissionPolicy {
    /// Tools the employee may invoke without changing its revision.
    #[serde(default)]
    pub allowed_tools: Vec<ToolCapability>,
    /// Workspace references the employee may access.
    #[serde(default)]
    pub allowed_workspaces: Vec<String>,
    /// Network destination references the employee may access.
    #[serde(default)]
    pub allowed_networks: Vec<String>,
    /// Operations that require human approval.
    #[serde(default)]
    pub approval_required: Vec<ApprovalRequirement>,
}

/// Per-employee participation policy used by the conversation router.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EmployeeRoutingPolicy {
    /// Whether the employee may be considered for new routing decisions.
    pub enabled: bool,
    /// Optional score floor that can only tighten the company-wide threshold.
    pub semantic_min_score: Option<f32>,
}

impl EmployeeRoutingPolicy {
    /// Validates score ranges.
    pub fn validate(&self) -> Result<(), DomainError> {
        if self
            .semantic_min_score
            .is_some_and(|score| !score.is_finite() || !(0.0..=1.0).contains(&score))
        {
            return Err(DomainError::InvalidScore {
                field: "employee.routing.semantic_min_score",
            });
        }
        Ok(())
    }
}

/// Versionable employee definition used by routing and runtime dispatch.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Employee {
    /// Stable employee identifier.
    pub id: EmployeeId,
    /// Display name.
    pub name: String,
    /// Company title.
    pub title: String,
    /// Short employee biography or charter.
    pub biography: String,
    /// Lifecycle state.
    pub status: EmployeeStatus,
    /// Additional unique names accepted by deterministic routing.
    #[serde(default)]
    pub aliases: Vec<String>,
    /// Areas for which the employee is accountable.
    #[serde(default)]
    pub responsibilities: Vec<String>,
    /// Semantic expertise labels used by future scorer adapters.
    #[serde(default)]
    pub domains: Vec<String>,
    /// Runtime binding.
    pub runtime: RuntimeBinding,
    /// Optional durable memory binding.
    pub memory: Option<MemoryBinding>,
    /// Office identity binding.
    pub office: OfficeBinding,
    /// Tool and resource permissions.
    #[serde(default)]
    pub permissions: PermissionPolicy,
    /// Routing participation policy.
    pub routing: EmployeeRoutingPolicy,
}

impl Employee {
    /// Validates an employee definition without performing I/O.
    pub fn validate(&self, provisioning: ProvisioningMode) -> Result<(), DomainError> {
        self.validate_definition()?;

        if provisioning == ProvisioningMode::Adopt
            && self
                .runtime
                .profile_ref
                .as_deref()
                .is_none_or(|value| value.trim().is_empty())
        {
            return Err(DomainError::MissingAdoptProfile);
        }

        Ok(())
    }

    /// Validates fields required whenever an employee enters a routing catalog.
    pub fn validate_definition(&self) -> Result<(), DomainError> {
        require_bounded_text("employee.name", &self.name, 128)?;
        require_bounded_text("employee.title", &self.title, 256)?;
        require_bounded_text("employee.biography", &self.biography, 4_096)?;
        validate_bounded_values("employee.aliases", &self.aliases, 32, 128)?;
        validate_bounded_values("employee.responsibilities", &self.responsibilities, 32, 512)?;
        validate_stable_codes("employee.domains", &self.domains, 64, 128)?;
        require_bounded_text("employee.runtime.adapter", &self.runtime.adapter, 64)?;
        require_bounded_text("employee.runtime.model", &self.runtime.model, 256)?;
        require_bounded_text(
            "employee.runtime.workspace_ref",
            &self.runtime.workspace_ref,
            1_024,
        )?;
        if self.runtime.credential_refs.len() > 32 {
            return Err(DomainError::InvalidField {
                field: "employee.runtime.credential_refs",
            });
        }
        if let Some(profile_ref) = &self.runtime.profile_ref {
            require_bounded_text("employee.runtime.profile_ref", profile_ref, 1_024)?;
        }
        validate_adapter_options("employee.runtime.options", &self.runtime.options)?;

        if let Some(memory) = &self.memory {
            require_bounded_text("employee.memory.adapter", &memory.adapter, 64)?;
            require_bounded_text("employee.memory.endpoint_ref", &memory.endpoint_ref, 1_024)?;
            require_bounded_text("employee.memory.workspace", &memory.workspace, 256)?;
            require_bounded_text("employee.memory.user_peer", &memory.user_peer, 128)?;
            require_bounded_text("employee.memory.employee_peer", &memory.employee_peer, 128)?;
            validate_adapter_options("employee.memory.options", &memory.options)?;
        }

        if self.office.public_key.len() != 64
            || !self
                .office
                .public_key
                .chars()
                .all(|character| character.is_ascii_hexdigit())
        {
            return Err(DomainError::InvalidOfficePublicKey);
        }

        if let Some(home_channel_ref) = &self.office.home_channel_ref {
            require_bounded_text("employee.office.home_channel_ref", home_channel_ref, 1_024)?;
        }

        validate_bounded_values(
            "employee.permissions.allowed_workspaces",
            &self.permissions.allowed_workspaces,
            64,
            1_024,
        )?;
        validate_bounded_values(
            "employee.permissions.allowed_networks",
            &self.permissions.allowed_networks,
            64,
            1_024,
        )?;
        if has_duplicates(&self.permissions.allowed_tools)
            || has_duplicates(&self.permissions.approval_required)
        {
            return Err(DomainError::InvalidField {
                field: "employee.permissions",
            });
        }

        self.routing.validate()
    }

    /// Returns whether the employee is eligible before per-message loop guards.
    pub fn accepts_routing(&self) -> bool {
        self.status == EmployeeStatus::Active && self.routing.enabled
    }

    /// Returns every normalized alias, including the id and display name.
    pub fn normalized_aliases(&self) -> BTreeSet<String> {
        std::iter::once(self.id.as_str())
            .chain(std::iter::once(self.name.as_str()))
            .chain(self.aliases.iter().map(String::as_str))
            .map(normalize_alias)
            .filter(|alias| !alias.is_empty())
            .collect()
    }
}

/// Secret-free, versioned input to employee provisioning.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EmployeeManifest {
    /// Manifest schema identifier.
    pub schema_version: String,
    /// Create or adopt semantics for external resources.
    pub provisioning: ProvisioningMode,
    /// Employee definition to validate and provision.
    pub employee: Employee,
}

impl EmployeeManifest {
    /// Validates schema identity and the contained employee.
    pub fn validate(&self) -> Result<(), DomainError> {
        if self.schema_version != EMPLOYEE_MANIFEST_SCHEMA_V0 {
            return Err(DomainError::UnsupportedManifestSchema(
                self.schema_version.clone(),
            ));
        }
        self.employee.validate(self.provisioning)
    }
}

/// Employee manifest schema supported by Architecture v0.
pub const EMPLOYEE_MANIFEST_SCHEMA_V0: &str = "ortak.employee/v0";

/// Validated employee lookup and company-wide alias index.
#[derive(Clone, Debug)]
pub struct EmployeeCatalog {
    employees: BTreeMap<EmployeeId, Employee>,
    aliases: BTreeMap<String, EmployeeId>,
}

impl EmployeeCatalog {
    /// Builds a catalog while enforcing unique employee ids and aliases.
    pub fn new(employees: impl IntoIterator<Item = Employee>) -> Result<Self, DomainError> {
        let mut by_id: BTreeMap<EmployeeId, Employee> = BTreeMap::new();
        let mut aliases: BTreeMap<String, EmployeeId> = BTreeMap::new();

        for employee in employees {
            employee.validate_definition()?;
            if by_id.contains_key(&employee.id) {
                return Err(DomainError::DuplicateEmployeeId(employee.id.to_string()));
            }

            for alias in employee.normalized_aliases() {
                if let Some(existing) = aliases.get(&alias) {
                    if existing != &employee.id {
                        return Err(DomainError::AliasCollision {
                            alias,
                            first: existing.to_string(),
                            second: employee.id.to_string(),
                        });
                    }
                } else {
                    aliases.insert(alias, employee.id.clone());
                }
            }

            by_id.insert(employee.id.clone(), employee);
        }

        Ok(Self {
            employees: by_id,
            aliases,
        })
    }

    /// Finds an employee by stable identifier.
    pub fn get(&self, id: &EmployeeId) -> Option<&Employee> {
        self.employees.get(id)
    }

    /// Iterates through employees in stable identifier order.
    pub fn employees(&self) -> impl Iterator<Item = &Employee> {
        self.employees.values()
    }

    /// Iterates through normalized aliases and their owners.
    pub fn aliases(&self) -> impl Iterator<Item = (&str, &EmployeeId)> {
        self.aliases
            .iter()
            .map(|(alias, employee_id)| (alias.as_str(), employee_id))
    }
}

/// Normalizes a human-facing employee alias for deterministic matching.
pub fn normalize_alias(value: &str) -> String {
    let folded = value
        .trim()
        .trim_start_matches('@')
        .nfkc()
        .flat_map(char::to_lowercase)
        // Unicode lowercase maps `İ` to `i` + COMBINING DOT ABOVE. Employee
        // aliases are user-facing identifiers, so accept common Turkish
        // dotted/dotless-I spelling variants. Catalog collision checks still
        // fail closed when that tolerance would make two employees ambiguous.
        .filter(|character| *character != '\u{307}')
        .map(|character| if character == 'ı' { 'i' } else { character })
        .collect::<String>();

    folded
        .nfkc()
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn require_text(field: &'static str, value: &str) -> Result<(), DomainError> {
    if value.trim().is_empty() || value.chars().any(char::is_control) {
        Err(DomainError::EmptyField { field })
    } else {
        Ok(())
    }
}

fn require_bounded_text(
    field: &'static str,
    value: &str,
    max_bytes: usize,
) -> Result<(), DomainError> {
    require_text(field, value)?;
    if value.len() > max_bytes {
        Err(DomainError::InvalidField { field })
    } else {
        Ok(())
    }
}

fn validate_bounded_values(
    field: &'static str,
    values: &[String],
    max_items: usize,
    max_bytes: usize,
) -> Result<(), DomainError> {
    if values.len() > max_items {
        return Err(DomainError::InvalidField { field });
    }
    values
        .iter()
        .try_for_each(|value| require_bounded_text(field, value, max_bytes))
}

fn validate_stable_codes(
    field: &'static str,
    values: &[String],
    max_items: usize,
    max_bytes: usize,
) -> Result<(), DomainError> {
    validate_bounded_values(field, values, max_items, max_bytes)?;
    if values.iter().any(|value| {
        !value.chars().all(|character| {
            character.is_ascii_lowercase()
                || character.is_ascii_digit()
                || matches!(character, '_' | '-' | '.' | ':')
        })
    }) {
        Err(DomainError::InvalidField { field })
    } else {
        Ok(())
    }
}

fn has_duplicates<T>(values: &[T]) -> bool
where
    T: Ord,
{
    let mut seen = BTreeSet::new();
    values.iter().any(|value| !seen.insert(value))
}

fn validate_adapter_options(
    field: &'static str,
    options: &BTreeMap<String, String>,
) -> Result<(), DomainError> {
    const SECRET_KEYS: &[&str] = &[
        "api_key",
        "apikey",
        "auth_token",
        "bearer_token",
        "credential",
        "credentials",
        "oauth",
        "oauth_token",
        "password",
        "passwd",
        "private_key",
        "privatekey",
        "secret",
        "token",
        "access_token",
        "refresh_token",
    ];

    if options.len() > 64 {
        return Err(DomainError::UnsafeAdapterOption { field });
    }

    for (key, value) in options {
        let normalized_key = key.trim().to_ascii_lowercase().replace(['-', '.'], "_");
        let normalized_value = value.trim().to_ascii_lowercase();
        let unsafe_key = SECRET_KEYS.contains(&normalized_key.as_str())
            || [
                "_api_key",
                "_credential",
                "_oauth",
                "_password",
                "_private_key",
                "_secret",
                "_token",
            ]
            .iter()
            .any(|suffix| normalized_key.ends_with(suffix));
        let unsafe_value = normalized_value.starts_with("bearer ")
            || normalized_value.starts_with("nsec1")
            || normalized_value.contains("-----begin private key-----");
        let malformed = normalized_key.is_empty()
            || key.len() > 128
            || value.is_empty()
            || value.len() > 2_048
            || key.chars().any(char::is_control)
            || value.chars().any(char::is_control);

        if unsafe_key || unsafe_value || malformed {
            return Err(DomainError::UnsafeAdapterOption { field });
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{normalize_alias, CredentialRef, EmployeeId};

    #[test]
    fn employee_id_rejects_path_like_values() {
        assert!(EmployeeId::parse("cem").is_ok());
        assert!(EmployeeId::parse("../cem").is_err());
        assert!(EmployeeId::parse("Cem").is_err());
    }

    #[test]
    fn alias_normalization_is_unicode_aware_and_collapses_spacing() {
        assert_eq!(normalize_alias("  @ZEYNEP  "), "zeynep");
        assert_eq!(normalize_alias("Cem   Yılmaz"), "cem yilmaz");
        assert_eq!(normalize_alias("İpek"), "ipek");
        assert_eq!(normalize_alias("ıpek"), "ipek");
    }

    #[test]
    fn credential_references_require_a_safe_nonempty_locator() {
        assert!(CredentialRef::parse("credential://runtime/cem/key").is_ok());
        assert!(CredentialRef::parse("secret://vault/team#field").is_ok());
        assert!(CredentialRef::parse("credential://").is_err());
        assert!(CredentialRef::parse("credential://runtime/../key").is_err());
        assert!(CredentialRef::parse("credential://runtime/key value").is_err());
        assert!(CredentialRef::parse("sk-live-secret").is_err());
    }
}
