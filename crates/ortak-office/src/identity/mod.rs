//! Explicitly scoped adoption of prepared Office identities and durable profile
//! publication. The caller must authorize selection of this instance: the
//! provisioning port deliberately carries no caller principal. Creation and
//! deletion are unsupported, and no delivery-time authority is repurposed here.

mod postgres;
mod profile;

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use ortak_control::adapter::{Detail, HealthReport, ResourceOutcome};
use ortak_control::office_identity::{
    OfficeIdentityAdapter, OfficeIdentityError, OfficeMembershipRequest, OfficePublicKey,
    ProfilePublication, SignerVerification,
};
use ortak_control::PgControlPlane;
use ortak_domain::{CredentialRef, EmployeeId, OfficeBinding, ProvisioningMode};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::transport::{valid_origin, EnvOfficeSigner, OfficeTransportConfigError};

/// An explicitly prepared employee and the complete channel membership cohort
/// whose current presence is required by this adapter.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct OfficeIdentityEmployee {
    /// Stable employee in the configured company.
    pub employee_id: EmployeeId,
    /// Exact public key, opaque reference, and optional canonical home channel.
    pub office: OfficeBinding,
    /// One to 64 canonical channel UUIDs in the configured community.
    pub channels: Vec<Uuid>,
}

/// One server-owned company/community route and a finite identity allowlist.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct OfficeIdentityConfig {
    /// Company authorized by the composition root.
    pub company_id: Uuid,
    /// Canonical Office community bound to that company in PostgreSQL.
    pub community_id: Uuid,
    /// Canonical HTTPS origin, or HTTP loopback for isolated development.
    pub origin: String,
    /// One to 64 explicitly authorized employees; no discovery is performed.
    pub employees: Vec<OfficeIdentityEmployee>,
}

impl OfficeIdentityConfig {
    /// Validates the complete public allowlist without reading secrets or
    /// contacting the database. The composition root may call this before
    /// constructing the environment-backed signer.
    pub fn validate(&self) -> Result<(), OfficeTransportConfigError> {
        let mut employees = HashSet::new();
        let mut keys = HashSet::new();
        let mut references = HashSet::new();
        if self.company_id.is_nil()
            || self.community_id.is_nil()
            || !valid_origin(&self.origin)
            || self.employees.is_empty()
            || self.employees.len() > 64
        {
            return Err(OfficeTransportConfigError::InvalidConfiguration);
        }
        for entry in &self.employees {
            let key = OfficePublicKey::parse_hex(&entry.office.public_key)
                .map_err(|_| OfficeTransportConfigError::InvalidConfiguration)?;
            let channels: HashSet<_> = entry.channels.iter().copied().collect();
            let home_valid = entry.office.home_channel_ref.as_ref().is_none_or(|home| {
                Uuid::parse_str(home)
                    .is_ok_and(|id| id.to_string() == *home && channels.contains(&id))
            });
            if !employees.insert(entry.employee_id.clone())
                || !keys.insert(key)
                || !references.insert(entry.office.signer_ref.clone())
                || entry.office.public_key != key.to_hex()
                || channels.is_empty()
                || channels.len() > 64
                || channels.len() != entry.channels.len()
                || channels.iter().any(Uuid::is_nil)
                || !home_valid
            {
                return Err(OfficeTransportConfigError::InvalidConfiguration);
            }
        }
        Ok(())
    }
}

/// Production identity port using current canonical PostgreSQL membership,
/// an explicitly selected signer, and NIP-98-authenticated profile publication.
/// Profile bytes are committed to the adapter journal before network I/O.
/// Deliberately has no Debug implementation containing signer state.
#[derive(Clone)]
pub struct PgOfficeIdentityAdapter {
    control: PgControlPlane,
    signer: EnvOfficeSigner,
    config: Arc<OfficeIdentityConfig>,
    client: reqwest::Client,
}

impl PgOfficeIdentityAdapter {
    /// Constructs a bound adapter, proving all configured signer mappings before
    /// use. Redirects and ambient proxies are disabled. Network calls have a
    /// total timeout in 1ms..30s and response bodies are bounded to 8KiB.
    pub fn new(
        control: PgControlPlane,
        signer: EnvOfficeSigner,
        mut config: OfficeIdentityConfig,
        timeout: Duration,
    ) -> Result<Self, OfficeTransportConfigError> {
        config.validate()?;
        if timeout < Duration::from_millis(1) || timeout > Duration::from_secs(30) {
            return Err(OfficeTransportConfigError::InvalidConfiguration);
        }
        for entry in &mut config.employees {
            entry.channels.sort_unstable();
            let expected = OfficePublicKey::parse_hex(&entry.office.public_key)
                .map_err(|_| OfficeTransportConfigError::InvalidConfiguration)?;
            signer
                .keys(
                    config.company_id,
                    &entry.employee_id,
                    &entry.office.signer_ref,
                    &expected,
                )
                .map_err(|_| OfficeTransportConfigError::SecretMismatch)?;
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
            config: Arc::new(config),
            client,
        })
    }

    fn employee(
        &self,
        id: &EmployeeId,
        office: &OfficeBinding,
    ) -> Result<&OfficeIdentityEmployee, OfficeIdentityError> {
        self.config
            .employees
            .iter()
            .find(|entry| &entry.employee_id == id && &entry.office == office)
            .ok_or_else(|| rejected("office_identity_not_configured"))
    }

    fn keys(&self, entry: &OfficeIdentityEmployee) -> Result<&nostr::Keys, OfficeIdentityError> {
        let expected = OfficePublicKey::parse_hex(&entry.office.public_key)?;
        self.signer
            .keys(
                self.config.company_id,
                &entry.employee_id,
                &entry.office.signer_ref,
                &expected,
            )
            .map_err(|_| OfficeIdentityError::signer(&entry.office.signer_ref))
    }
}

impl OfficeIdentityAdapter for PgOfficeIdentityAdapter {
    async fn verify_signer(
        &self,
        signer_ref: &CredentialRef,
        expected: &OfficePublicKey,
    ) -> Result<SignerVerification, OfficeIdentityError> {
        let entry = self
            .config
            .employees
            .iter()
            .find(|entry| {
                &entry.office.signer_ref == signer_ref
                    && entry.office.public_key == expected.to_hex()
            })
            .ok_or_else(|| rejected("office_signer_not_configured"))?;
        let keys = self.keys(entry)?;
        // An ephemeral local challenge is never published. Its fresh nonce and
        // full scope demonstrate actual signing rather than accepting metadata.
        let challenge = serde_json::json!({
            "purpose": "ortak-office-identity-proof-v1",
            "company_id": self.config.company_id, "community_id": self.config.community_id,
            "employee_id": entry.employee_id, "nonce": Uuid::new_v4(),
        })
        .to_string();
        let event = nostr::UnsignedEvent::new(
            keys.public_key(),
            nostr::Timestamp::now(),
            nostr::Kind::from_u16(buzz_core::kind::KIND_HTTP_AUTH as u16),
            [],
            challenge,
        )
        .sign_with_keys(keys)
        .map_err(|_| rejected("office_signer_proof_failed"))?;
        event
            .verify()
            .map_err(|_| rejected("office_signer_proof_failed"))?;
        let produced_public_key = OfficePublicKey::parse_hex(&event.pubkey.to_hex())?;
        Ok(SignerVerification {
            produced_public_key,
            matches_expected: &produced_public_key == expected,
        })
    }

    async fn ensure_membership(
        &self,
        request: &OfficeMembershipRequest,
    ) -> Result<ResourceOutcome, OfficeIdentityError> {
        let entry = self.employee(&request.employee_id, &request.binding)?;
        if request.mode != ProvisioningMode::Adopt || !valid_key(&request.idempotency_key) {
            return Err(rejected("office_membership_create_unsupported"));
        }
        self.check_membership(entry).await?;
        Ok(ResourceOutcome::adopted(format!(
            "office-member:{}:{}",
            self.config.community_id, entry.office.public_key
        )))
    }

    async fn remove_created_membership(
        &self,
        _resource_ref: &str,
        _idempotency_key: &str,
    ) -> Result<(), OfficeIdentityError> {
        Err(rejected("office_membership_delete_unsupported"))
    }

    async fn membership_health(
        &self,
        public_key: &OfficePublicKey,
    ) -> Result<HealthReport, OfficeIdentityError> {
        let entry = self
            .config
            .employees
            .iter()
            .find(|entry| entry.office.public_key == public_key.to_hex())
            .ok_or_else(|| rejected("office_identity_not_configured"))?;
        self.check_membership(entry).await?;
        Ok(HealthReport::healthy("office_current_membership_verified"))
    }

    async fn publish_profile(
        &self,
        employee_id: &EmployeeId,
        binding: &OfficeBinding,
        display_name: &str,
        idempotency_key: &str,
    ) -> Result<ProfilePublication, OfficeIdentityError> {
        let entry = self.employee(employee_id, binding)?;
        if !valid_key(idempotency_key)
            || display_name.trim().is_empty()
            || display_name.len() > 256
            || display_name.chars().any(char::is_control)
        {
            return Err(rejected("office_profile_invalid_request"));
        }
        let frozen = self
            .freeze_profile(entry, display_name, idempotency_key)
            .await?;
        self.authorize_profile(entry, display_name, idempotency_key, &frozen)
            .await?;
        if !frozen.acknowledged {
            self.send_profile(entry, &frozen).await?;
        }
        self.acknowledge_profile(entry, display_name, idempotency_key, &frozen)
            .await?;
        Ok(ProfilePublication {
            receipt_ref: frozen.event_id,
        })
    }
}

fn valid_key(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 256
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b':' | b'-' | b'_' | b'.'))
}

fn rejected(code: &str) -> OfficeIdentityError {
    OfficeIdentityError::Rejected {
        detail: Detail::new(code),
    }
}

fn unavailable(code: &str) -> OfficeIdentityError {
    OfficeIdentityError::Unavailable {
        detail: Detail::new(code),
    }
}

#[cfg(test)]
mod tests;
