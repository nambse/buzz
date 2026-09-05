use ortak_domain::CredentialRef;
use serde::Deserialize;
use url::Url;
use uuid::Uuid;
use zeroize::Zeroizing;

/// Explicit operator selection; no provider, model or credential defaults.
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SemanticConfig {
    /// Identity of this selected provider deployment.
    pub deployment_id: Uuid,
    /// Fixed HTTPS or literal loopback origin; the adapter owns the API path.
    pub origin: String,
    /// Explicit model name sent to the provider.
    pub model: String,
    /// Exact model snapshot required in the provider's response.
    pub response_model: String,
    /// Opaque credential reference selected for this provider.
    pub token_ref: CredentialRef,
}

/// Resolved authentication material; deliberately neither Debug nor serializable.
pub struct SemanticToken {
    pub(crate) reference: CredentialRef,
    pub(crate) secret: Zeroizing<String>,
}

impl SemanticToken {
    /// Receives an explicit resolver result without persisting the secret.
    pub fn new(reference: CredentialRef, secret: Zeroizing<String>) -> Self {
        Self { reference, secret }
    }

    /// Reads only the explicitly selected environment variable; errors omit its value.
    pub fn from_env(reference: CredentialRef, name: &str) -> Result<Self, &'static str> {
        if name.is_empty()
            || name.len() > 128
            || !name
                .bytes()
                .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
        {
            return Err("invalid semantic credential environment reference");
        }
        let value = std::env::var(name).map_err(|_| "semantic credential unavailable")?;
        Ok(Self::new(reference, Zeroizing::new(value)))
    }
}

pub(crate) fn validate(config: &SemanticConfig) -> Result<Url, &'static str> {
    let model = |s: &str| {
        !s.is_empty()
            && s.len() <= 128
            && s.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-_./:".contains(&b))
    };
    if config.deployment_id.is_nil() || !model(&config.model) || !model(&config.response_model) {
        return Err("semantic deployment and exact model selections required");
    }
    let origin = Url::parse(&config.origin).map_err(|_| "invalid semantic origin")?;
    let loopback = match origin.host() {
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
        return Err(
            "semantic origin must be fixed HTTPS or literal loopback without credentials or path",
        );
    }
    Ok(origin)
}
