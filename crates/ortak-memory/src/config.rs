use std::{collections::BTreeSet, time::Duration};

use ortak_domain::{CredentialRef, EmployeeId, MemoryBinding, ProvisioningMode};
use url::Url;
use uuid::Uuid;
use zeroize::Zeroizing;

use crate::{invalid, MemoryError, HONCHO_VERSION, PROTOCOL};

/// One explicitly selected deployment; this is configuration, not health evidence.
#[derive(Clone)]
pub struct HonchoDeploymentSelection {
    /// Operator-owned deployment identity, changed when the service is replaced.
    pub deployment_id: Uuid,
    /// Must be the reviewed `ortak-honcho/1` extension protocol.
    pub protocol: String,
    /// Must be the reviewed Honcho `3.1.1` version.
    pub honcho_version: String,
    /// Opaque service reference present in every allowed memory binding.
    pub endpoint_ref: String,
    /// Fixed HTTPS or explicit loopback origin, without path/query/credentials.
    pub origin: String,
    /// Opaque credential reference authorized for this deployment.
    pub token_ref: CredentialRef,
}

/// A server-authorized employee and its complete immutable memory binding.
#[derive(Clone)]
pub struct HonchoEmployeeBinding {
    /// Stable employee identity.
    pub employee_id: EmployeeId,
    /// Exact binding; requests cannot change any field, including options.
    pub binding: MemoryBinding,
    /// Fresh owned bundles can validate memory I/O; adoption stays read-only.
    pub mode: ProvisioningMode,
    /// Explicit permission for the company-truth namespace, false by default in callers.
    pub allow_company_truth: bool,
    /// Project namespaces this employee may use through this adapter.
    pub allowed_projects: BTreeSet<Uuid>,
}

/// Bounded, secret-free adapter configuration resolved by the server.
#[derive(Clone)]
pub struct HonchoMemoryConfig {
    /// Explicit deployment/version and endpoint selection.
    pub deployment: HonchoDeploymentSelection,
    /// Finite cohort of at most 64 distinct employee/workspace bindings.
    pub employees: Vec<HonchoEmployeeBinding>,
    /// One HTTP request deadline, between one and fifteen seconds.
    pub request_timeout: Duration,
    /// Binding-specific roundtrip evidence lifetime, at most fifteen minutes.
    pub witness_lifetime: Duration,
}

/// Resolved token material. Intentionally has no Debug or serialization implementation.
pub struct ResolvedHonchoToken {
    pub(crate) token_ref: CredentialRef,
    pub(crate) secret: Zeroizing<String>,
}

impl ResolvedHonchoToken {
    /// Accepts material from an authorized resolver; never put the token in config/Git.
    pub fn new(token_ref: CredentialRef, secret: Zeroizing<String>) -> Self {
        Self { token_ref, secret }
    }

    /// Resolves one explicitly named fresh environment variable without logging it.
    pub fn from_env(token_ref: CredentialRef, name: &str) -> Result<Self, MemoryError> {
        if name.is_empty()
            || name.len() > 128
            || !name
                .bytes()
                .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
        {
            return Err(invalid("invalid memory token environment reference"));
        }
        let secret = std::env::var(name).map_err(|_| invalid("memory token is unavailable"))?;
        Ok(Self::new(token_ref, Zeroizing::new(secret)))
    }
}

pub(crate) fn name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

pub(crate) fn key(value: &str) -> bool {
    !value.is_empty() && value.len() <= 200 && value.bytes().all(|b| (0x21..=0x7e).contains(&b))
}

pub(crate) fn validate(config: &HonchoMemoryConfig) -> Result<Url, MemoryError> {
    let deployment = &config.deployment;
    if deployment.deployment_id.is_nil()
        || deployment.protocol != PROTOCOL
        || deployment.honcho_version != HONCHO_VERSION
        || deployment.endpoint_ref.is_empty()
        || deployment.endpoint_ref.len() > 256
        || deployment.endpoint_ref.chars().any(char::is_control)
        || !(Duration::from_secs(1)..=Duration::from_secs(15)).contains(&config.request_timeout)
        || config.witness_lifetime.is_zero()
        || config.witness_lifetime > Duration::from_secs(900)
        || config.employees.is_empty()
        || config.employees.len() > 64
    {
        return Err(invalid(
            "unsupported or unbounded memory deployment configuration",
        ));
    }
    let origin = Url::parse(&deployment.origin).map_err(|_| invalid("invalid memory origin"))?;
    let loopback = match origin.host() {
        Some(url::Host::Domain("localhost")) => true,
        Some(url::Host::Ipv4(ip)) => ip.is_loopback(),
        Some(url::Host::Ipv6(ip)) => ip.is_loopback(),
        _ => false,
    };
    if !(origin.scheme() == "https" || (origin.scheme() == "http" && loopback))
        || origin.host().is_none()
        || !origin.username().is_empty()
        || origin.password().is_some()
        || origin.path() != "/"
        || origin.query().is_some()
        || origin.fragment().is_some()
        || origin.port() == Some(0)
    {
        return Err(invalid(
            "memory origin must be fixed HTTPS or loopback without credentials or path",
        ));
    }
    let mut employees = BTreeSet::new();
    let mut workspaces = BTreeSet::new();
    for allowed in &config.employees {
        let b = &allowed.binding;
        if !employees.insert(&allowed.employee_id)
            || !workspaces.insert(&b.workspace)
            || b.adapter != "honcho"
            || b.endpoint_ref != deployment.endpoint_ref
            || !b.options.is_empty()
            || !name(&b.workspace)
            || !name(&b.user_peer)
            || !name(&b.employee_peer)
            || b.user_peer == b.employee_peer
            || allowed.allowed_projects.len() > 256
            || allowed.allowed_projects.iter().any(Uuid::is_nil)
        {
            return Err(invalid("invalid or ambiguous authorized memory binding"));
        }
    }
    Ok(origin)
}
