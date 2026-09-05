//! Explicit environment-backed credential presence checks.
use super::{CredentialError, CredentialReferenceStatus, CredentialResolver};
use crate::adapter::Detail;
use ortak_domain::CredentialRef;
use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::OsString,
    sync::Arc,
};

const MAX_BINDINGS: usize = 128;
const MAX_ENVIRONMENT_NAME_BYTES: usize = 128;

/// One already-authorized opaque reference and its exact process environment name.
/// This configuration contains names only, never credential values.
#[derive(Clone, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EnvCredentialBinding {
    /// The reference the caller is authorized to check.
    pub credential_ref: CredentialRef,
    /// Portable ASCII name: `[A-Za-z_][A-Za-z0-9_]{0,127}`.
    pub environment_variable: String,
}

trait EnvironmentLookup: Send + Sync {
    fn read(&self, name: &str) -> Option<OsString>;
}
struct ProcessEnvironment;
impl EnvironmentLookup for ProcessEnvironment {
    fn read(&self, name: &str) -> Option<OsString> {
        std::env::var_os(name)
    }
}

/// Checks current environment availability through a finite explicit allowlist.
///
/// One instance belongs to an already-authorized caller/cohort. The port carries
/// no company or principal, so callers must select the correct instance before
/// invoking it. This type neither discovers references nor enforces tenant scope.
///
/// Values are read only during verification and are never returned, cached,
/// logged, or included in errors. Missing and empty values report `Missing`;
/// non-Unicode values report a sanitized `Unavailable` error. Any nonempty
/// Unicode value, including whitespace, reports `Resolvable`: credential format
/// and actual provider/signer usability remain the owning adapter's responsibility.
/// Debug formatting is deliberately not implemented.
#[derive(Clone)]
pub struct EnvCredentialResolver {
    bindings: BTreeMap<CredentialRef, String>,
    lookup: Arc<dyn EnvironmentLookup>,
}

fn unavailable(detail: &'static str) -> CredentialError {
    CredentialError::Unavailable {
        detail: Detail::new(detail),
    }
}
fn valid_name(name: &str) -> bool {
    let mut bytes = name.bytes();
    !name.is_empty()
        && name.len() <= MAX_ENVIRONMENT_NAME_BYTES
        && bytes
            .next()
            .is_some_and(|first| first.is_ascii_alphabetic() || first == b'_')
        && bytes.all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

impl EnvCredentialResolver {
    /// Validates at most 128 mappings without accessing the environment.
    ///
    /// Duplicate references, duplicate environment names and malformed names
    /// reject the entire configuration. This deliberately chooses a one-to-one
    /// mapping policy; aliases to one environment variable are not supported.
    /// An empty list creates a deny-all resolver.
    pub fn new(bindings: Vec<EnvCredentialBinding>) -> Result<Self, CredentialError> {
        Self::with_lookup(bindings, Arc::new(ProcessEnvironment))
    }

    fn with_lookup(
        bindings: Vec<EnvCredentialBinding>,
        lookup: Arc<dyn EnvironmentLookup>,
    ) -> Result<Self, CredentialError> {
        if bindings.len() > MAX_BINDINGS {
            return Err(unavailable("environment credential allowlist is too large"));
        }
        let mut allowed = BTreeMap::new();
        let mut environment_names = BTreeSet::new();
        for binding in bindings {
            if !valid_name(&binding.environment_variable)
                || allowed.contains_key(&binding.credential_ref)
                || !environment_names.insert(binding.environment_variable.clone())
            {
                return Err(unavailable("environment credential allowlist is invalid"));
            }
            allowed.insert(binding.credential_ref, binding.environment_variable);
        }
        Ok(Self {
            bindings: allowed,
            lookup,
        })
    }
}

impl CredentialResolver for EnvCredentialResolver {
    async fn verify_reference(
        &self,
        credential_ref: &CredentialRef,
    ) -> Result<CredentialReferenceStatus, CredentialError> {
        let name =
            self.bindings
                .get(credential_ref)
                .ok_or_else(|| CredentialError::Unauthorized {
                    credential_ref: credential_ref.as_str().to_owned(),
                })?;
        let Some(value) = self.lookup.read(name) else {
            return Ok(CredentialReferenceStatus::Missing);
        };
        let value = value
            .to_str()
            .ok_or_else(|| unavailable("selected environment credential is not Unicode"))?;
        Ok(if value.is_empty() {
            CredentialReferenceStatus::Missing
        } else {
            CredentialReferenceStatus::Resolvable
        })
    }
}

#[cfg(test)]
mod tests;
