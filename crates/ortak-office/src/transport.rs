//! Scoped environment-backed signing and authenticated, byte-preserving HTTP delivery.
//! No secret value or remote error body is retained in errors or Debug output.

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use base64::Engine;
use nostr::JsonUtil;
use ortak_control::adapter::Detail;
use ortak_control::office_identity::OfficePublicKey;
use ortak_control::{CompanyScope, PgControlPlane};
use ortak_domain::{CredentialRef, EmployeeId};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use uuid::Uuid;
use zeroize::Zeroizing;

use crate::{
    FrozenSignedEvent, OfficePublishError, OfficePublisher, OfficeSigner, OfficeSignerError,
    PublishReceipt, SigningRequest,
};

/// Public configuration selecting one employee's opaque signer and secret env name.
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OfficeSignerBinding {
    /// Company owning this signer.
    pub company_id: Uuid,
    /// Stable employee identity, independent of runtime revisions.
    pub employee_id: EmployeeId,
    /// Opaque reference matching the verified Office binding.
    pub signer_ref: CredentialRef,
    /// Expected public key; a different loaded secret fails startup.
    pub public_key: OfficePublicKey,
    /// Explicit environment variable containing a fresh 64-hex secret key.
    pub secret_env: String,
}

/// Closed configuration errors never include secret values or parser input.
#[derive(Debug, thiserror::Error)]
pub enum OfficeTransportConfigError {
    /// Invalid, duplicate, or oversized public configuration.
    #[error("invalid Office transport configuration")]
    InvalidConfiguration,
    /// A configured secret variable is absent or not valid Unicode.
    #[error("configured Office signing secret is unavailable")]
    SecretUnavailable,
    /// The secret is malformed or produces a different public key.
    #[error("configured Office signing secret does not match its public identity")]
    SecretMismatch,
    /// The bounded HTTP client could not be constructed.
    #[error("Office HTTP transport could not be initialized")]
    HttpInitialization,
}

struct SignerEntry {
    company_id: Uuid,
    employee_id: EmployeeId,
    signer_ref: CredentialRef,
    keys: nostr::Keys,
}

/// Real signer loaded once from an explicit operator allowlist. Deliberately no Debug.
#[derive(Clone)]
pub struct EnvOfficeSigner {
    entries: Arc<Vec<SignerEntry>>,
}

impl EnvOfficeSigner {
    /// Loads at most 64 identities from explicit environment names. Secret text
    /// is zeroized immediately after parsing; no environment writes occur.
    pub fn from_env(
        bindings: Vec<OfficeSignerBinding>,
    ) -> Result<Self, OfficeTransportConfigError> {
        Self::load(bindings, |name| {
            std::env::var(name).map_err(|_| OfficeTransportConfigError::SecretUnavailable)
        })
    }

    fn load(
        bindings: Vec<OfficeSignerBinding>,
        mut read: impl FnMut(&str) -> Result<String, OfficeTransportConfigError>,
    ) -> Result<Self, OfficeTransportConfigError> {
        if bindings.is_empty() || bindings.len() > 64 {
            return Err(OfficeTransportConfigError::InvalidConfiguration);
        }
        let mut identities = HashSet::new();
        let mut public_keys = HashSet::new();
        let mut entries = Vec::with_capacity(bindings.len());
        for binding in bindings {
            if binding.company_id.is_nil()
                || !valid_env_name(&binding.secret_env)
                || !identities.insert((
                    binding.company_id,
                    binding.employee_id.clone(),
                    binding.signer_ref.as_str().to_owned(),
                ))
                || !public_keys.insert(binding.public_key.to_hex())
            {
                return Err(OfficeTransportConfigError::InvalidConfiguration);
            }
            let secret = Zeroizing::new(read(&binding.secret_env)?);
            if secret.len() != 64 {
                return Err(OfficeTransportConfigError::SecretMismatch);
            }
            let secret_key = nostr::SecretKey::from_hex(secret.as_str())
                .map_err(|_| OfficeTransportConfigError::SecretMismatch)?;
            let keys = nostr::Keys::new(secret_key);
            if keys.public_key().to_bytes() != *binding.public_key.as_bytes() {
                return Err(OfficeTransportConfigError::SecretMismatch);
            }
            entries.push(SignerEntry {
                company_id: binding.company_id,
                employee_id: binding.employee_id,
                signer_ref: binding.signer_ref,
                keys,
            });
        }
        Ok(Self {
            entries: Arc::new(entries),
        })
    }

    fn keys(
        &self,
        company: Uuid,
        employee: &EmployeeId,
        reference: &CredentialRef,
        public_key: &OfficePublicKey,
    ) -> Result<&nostr::Keys, OfficeSignerError> {
        self.entries
            .iter()
            .find(|entry| {
                entry.company_id == company
                    && &entry.employee_id == employee
                    && &entry.signer_ref == reference
                    && entry.keys.public_key().to_bytes() == *public_key.as_bytes()
            })
            .map(|entry| &entry.keys)
            .ok_or_else(|| OfficeSignerError::Rejected {
                detail: Detail::new("Office signer identity is not configured"),
            })
    }

    fn authorization(
        &self,
        event: &FrozenSignedEvent,
        reference: &CredentialRef,
        url: &str,
    ) -> Result<String, OfficeSignerError> {
        let keys = self.keys(
            event.company_id(),
            event.employee_id(),
            reference,
            event.public_key(),
        )?;
        let raw = vec![
            vec!["u".to_owned(), url.to_owned()],
            vec!["method".to_owned(), "POST".to_owned()],
            vec![
                "payload".to_owned(),
                hex::encode(Sha256::digest(event.signed_bytes())),
            ],
            vec!["nonce".to_owned(), Uuid::new_v4().to_string()],
        ];
        let tags = raw
            .into_iter()
            .map(nostr::Tag::parse)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| signing_failed())?;
        let signed = nostr::UnsignedEvent::new(
            keys.public_key(),
            nostr::Timestamp::now(),
            nostr::Kind::from_u16(27235),
            tags,
            String::new(),
        )
        .sign_with_keys(keys)
        .map_err(|_| signing_failed())?;
        Ok(format!(
            "Nostr {}",
            base64::engine::general_purpose::STANDARD.encode(signed.as_json())
        ))
    }
}

impl OfficeSigner for EnvOfficeSigner {
    async fn sign(&self, request: &SigningRequest) -> Result<FrozenSignedEvent, OfficeSignerError> {
        let keys = self.keys(
            request.company_id(),
            request.employee_id(),
            request.signer_ref(),
            request.expected_public_key(),
        )?;
        let event = request
            .unsigned()
            .to_nostr()?
            .sign_with_keys(keys)
            .map_err(|_| signing_failed())?;
        Ok(FrozenSignedEvent::seal(
            request.unsigned(),
            event.as_json().as_bytes(),
        )?)
    }
}

fn valid_env_name(name: &str) -> bool {
    name.starts_with("ORTAK_")
        && name.len() <= 128
        && name
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_')
}

fn signing_failed() -> OfficeSignerError {
    OfficeSignerError::Rejected {
        detail: Detail::new("Office signing failed"),
    }
}

/// Explicit server-owned company/community to canonical relay-origin binding.
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OfficeRelayBinding {
    /// Ortak company.
    pub company_id: Uuid,
    /// Retained Office community, checked again against the live database binding.
    pub community_id: Uuid,
    /// HTTPS origin, or HTTP loopback for isolated development. No path or credentials.
    pub origin: String,
}

/// Real HTTP publisher. Authorization finishes before network I/O begins.
#[derive(Clone)]
pub struct HttpOfficePublisher {
    control: PgControlPlane,
    signer: EnvOfficeSigner,
    bindings: Arc<Vec<OfficeRelayBinding>>,
    client: reqwest::Client,
}

impl HttpOfficePublisher {
    /// Builds a publisher with 1..64 explicit routes and a 1ms..30s total timeout.
    /// Redirects and ambient proxy routing are disabled; responses are capped at 8KiB.
    pub fn new(
        control: PgControlPlane,
        signer: EnvOfficeSigner,
        bindings: Vec<OfficeRelayBinding>,
        timeout: Duration,
    ) -> Result<Self, OfficeTransportConfigError> {
        let mut companies = HashSet::new();
        let mut communities = HashSet::new();
        if bindings.is_empty()
            || bindings.len() > 64
            || timeout.is_zero()
            || timeout > Duration::from_secs(30)
            || bindings.iter().any(|binding| {
                binding.company_id.is_nil()
                    || binding.community_id.is_nil()
                    || !companies.insert(binding.company_id)
                    || !communities.insert(binding.community_id)
                    || !valid_origin(&binding.origin)
            })
        {
            return Err(OfficeTransportConfigError::InvalidConfiguration);
        }
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy()
            .timeout(timeout)
            .connect_timeout(timeout.min(Duration::from_secs(5)))
            .build()
            .map_err(|_| OfficeTransportConfigError::HttpInitialization)?;
        Ok(Self {
            control,
            signer,
            bindings: Arc::new(bindings),
            client,
        })
    }

    async fn send(
        &self,
        event: &FrozenSignedEvent,
        reference: &CredentialRef,
        origin: &str,
    ) -> Result<PublishReceipt, OfficePublishError> {
        let url = format!("{origin}/events");
        let auth = self
            .signer
            .authorization(event, reference, &url)
            .map_err(|_| rejected("office_signer_unavailable"))?;
        let mut response = self
            .client
            .post(&url)
            .header(reqwest::header::AUTHORIZATION, auth)
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(event.signed_bytes().to_vec())
            .send()
            .await
            .map_err(|_| unavailable("office_http_failed"))?;
        if !response.status().is_success() {
            return Err(
                if response.status().is_server_error() || response.status().as_u16() == 429 {
                    unavailable("office_http_unavailable")
                } else {
                    rejected("office_http_rejected")
                },
            );
        }
        let mut body = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| unavailable("office_response_interrupted"))?
        {
            if body.len() + chunk.len() > 8192 {
                return Err(rejected("office_response_too_large"));
            }
            body.extend_from_slice(&chunk);
        }
        receipt(&body, &event.event_id().to_hex())
    }
}

impl OfficePublisher for HttpOfficePublisher {
    async fn publish(
        &self,
        scope: &CompanyScope,
        event: &FrozenSignedEvent,
    ) -> Result<PublishReceipt, OfficePublishError> {
        let route = self
            .bindings
            .iter()
            .find(|binding| {
                binding.company_id == scope.company_id() && event.company_id() == scope.company_id()
            })
            .ok_or_else(|| rejected("office_route_unconfigured"))?;
        let expected_host = buzz_core::tenant::relay_url_authority(&route.origin);
        let (community, reference) =
            crate::postgres::before_publish(&self.control, scope, event, &expected_host)
                .await
                .map_err(|_| rejected("office_authority_unavailable"))?;
        if route.community_id != community {
            return Err(rejected("office_community_mismatch"));
        }
        self.send(event, &reference, &route.origin).await
    }
}

fn valid_origin(origin: &str) -> bool {
    if origin.len() > 2048 {
        return false;
    }
    let Ok(url) = url::Url::parse(origin) else {
        return false;
    };
    let loopback = matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"));
    (url.scheme() == "https" || url.scheme() == "http" && loopback)
        && url.origin().ascii_serialization() == origin
        && url.username().is_empty()
        && url.password().is_none()
        && url.query().is_none()
        && url.fragment().is_none()
}

fn receipt(body: &[u8], event_id: &str) -> Result<PublishReceipt, OfficePublishError> {
    #[derive(Deserialize)]
    struct Ack {
        event_id: String,
        accepted: bool,
        message: String,
    }
    let ack: Ack = serde_json::from_slice(body).map_err(|_| rejected("office_ack_malformed"))?;
    if ack.event_id != event_id || !ack.accepted {
        return Err(rejected("office_ack_rejected"));
    }
    Ok(if ack.message == "duplicate:" {
        PublishReceipt::AlreadyPresent
    } else {
        PublishReceipt::Accepted
    })
}

fn rejected(code: &str) -> OfficePublishError {
    OfficePublishError::Rejected {
        detail: Detail::new(code),
    }
}
fn unavailable(code: &str) -> OfficePublishError {
    OfficePublishError::Unavailable {
        detail: Detail::new(code),
    }
}

#[cfg(test)]
#[path = "transport/tests.rs"]
mod tests;
