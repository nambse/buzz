//! Shared vocabulary for the runtime, memory, and Office adapter ports.
//!
//! Everything here is provider-neutral and secret-free: adapters report
//! health, ownership, and bounded human-readable detail, never credential
//! material. The types serialize to snake_case so they can be stored as
//! adapter receipts in `provisioning_operation_steps.result`.

use std::fmt;

use serde::{Deserialize, Serialize};

/// Maximum bytes retained for any adapter-reported detail string.
pub const MAX_DETAIL_BYTES: usize = 512;

/// Normalized health of one external resource or service.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HealthState {
    /// The resource answered and is usable.
    Healthy,
    /// The resource answered but is impaired; activation must not proceed.
    Degraded,
    /// The resource did not answer or is unusable.
    Unhealthy,
}

impl HealthState {
    /// True only for [`HealthState::Healthy`]; degraded never passes a gate.
    pub fn is_healthy(self) -> bool {
        matches!(self, Self::Healthy)
    }
}

/// One health observation with bounded, redaction-safe detail.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct HealthReport {
    /// Observed state.
    pub state: HealthState,
    /// Bounded operator-facing detail; adapters must not include secrets.
    pub detail: Detail,
}

impl HealthReport {
    /// Builds a healthy report.
    pub fn healthy(detail: impl AsRef<str>) -> Self {
        Self {
            state: HealthState::Healthy,
            detail: Detail::new(detail),
        }
    }

    /// Builds a degraded report.
    pub fn degraded(detail: impl AsRef<str>) -> Self {
        Self {
            state: HealthState::Degraded,
            detail: Detail::new(detail),
        }
    }

    /// Builds an unhealthy report.
    pub fn unhealthy(detail: impl AsRef<str>) -> Self {
        Self {
            state: HealthState::Unhealthy,
            detail: Detail::new(detail),
        }
    }

    /// True only when the state is healthy.
    pub fn is_healthy(&self) -> bool {
        self.state.is_healthy()
    }
}

/// Who owns the lifecycle of an external resource.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceOwnership {
    /// Ortak created the resource in this operation and may delete it during
    /// compensation.
    Created,
    /// The resource existed before Ortak bound to it. It is never deleted,
    /// replaced, or recreated by Ortak.
    Adopted,
}

impl ResourceOwnership {
    /// True for adopted resources.
    pub fn is_adopted(self) -> bool {
        matches!(self, Self::Adopted)
    }
}

/// Result of a create-or-adopt request against one external resource.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ResourceOutcome {
    /// Stable, secret-free reference to the external resource.
    pub resource_ref: String,
    /// Whether this operation created the resource or adopted an existing one.
    pub ownership: ResourceOwnership,
}

impl ResourceOutcome {
    /// Describes a resource created by this operation.
    pub fn created(resource_ref: impl Into<String>) -> Self {
        Self {
            resource_ref: resource_ref.into(),
            ownership: ResourceOwnership::Created,
        }
    }

    /// Describes a pre-existing resource that was adopted unchanged.
    pub fn adopted(resource_ref: impl Into<String>) -> Self {
        Self {
            resource_ref: resource_ref.into(),
            ownership: ResourceOwnership::Adopted,
        }
    }
}

/// Bounded, control-character-free text safe to persist in receipts and errors.
///
/// Construction truncates on a character boundary at [`MAX_DETAIL_BYTES`] and
/// strips control characters. It performs no secret detection: adapters are
/// responsible for never placing credential values in a detail.
#[derive(Clone, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct Detail(String);

impl Detail {
    /// Bounds and sanitizes a detail string.
    pub fn new(value: impl AsRef<str>) -> Self {
        let cleaned = value
            .as_ref()
            .chars()
            .filter(|character| !character.is_control() || *character == '\n')
            .collect::<String>();
        Self(truncate_at_char_boundary(&cleaned, MAX_DETAIL_BYTES).to_owned())
    }

    /// Returns the text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Detail {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self.0, formatter)
    }
}

impl fmt::Display for Detail {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// Returns the longest prefix of `value` that is at most `max_bytes` long and
/// ends on a character boundary.
pub fn truncate_at_char_boundary(value: &str, max_bytes: usize) -> &str {
    if value.len() <= max_bytes {
        return value;
    }
    let mut end = max_bytes;
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    &value[..end]
}

#[cfg(test)]
mod tests {
    use super::{truncate_at_char_boundary, Detail, HealthState, MAX_DETAIL_BYTES};

    #[test]
    fn detail_is_bounded_and_control_free() {
        let long = "é".repeat(MAX_DETAIL_BYTES);
        let detail = Detail::new(&long);
        assert!(detail.as_str().len() <= MAX_DETAIL_BYTES);
        assert_eq!(Detail::new("a\u{0}b\tc\nd").as_str(), "abc\nd");
    }

    #[test]
    fn truncation_respects_char_boundaries() {
        assert_eq!(truncate_at_char_boundary("héllo", 2), "h");
        assert_eq!(truncate_at_char_boundary("héllo", 3), "hé");
    }

    #[test]
    fn only_healthy_passes() {
        assert!(HealthState::Healthy.is_healthy());
        assert!(!HealthState::Degraded.is_healthy());
        assert!(!HealthState::Unhealthy.is_healthy());
    }
}
