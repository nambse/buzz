use nostr::JsonUtil;
use serde::Deserialize;
use sha2::{Digest, Sha256};

use super::{
    rejected, unavailable, OfficeIdentityEmployee, OfficeIdentityError, PgOfficeIdentityAdapter,
};

pub(super) struct FrozenProfile {
    pub(super) event_id: String,
    pub(super) bytes: Vec<u8>,
    pub(super) acknowledged: bool,
}

impl PgOfficeIdentityAdapter {
    pub(super) fn profile_content(&self, entry: &OfficeIdentityEmployee, name: &str) -> String {
        serde_json::json!({
            "name": name, "display_name": name, "bot": true,
            "ortak_employee_id": entry.employee_id,
        })
        .to_string()
    }

    pub(super) fn profile_hash(&self, entry: &OfficeIdentityEmployee, name: &str) -> [u8; 32] {
        Sha256::digest(
            serde_json::json!({
                "schema": "ortak-office-profile-v1", "company_id": self.config.company_id,
                "community_id": self.config.community_id, "origin": self.config.origin,
                "employee": entry, "content": self.profile_content(entry, name),
            })
            .to_string()
            .as_bytes(),
        )
        .into()
    }

    pub(super) fn sign_profile(
        &self,
        entry: &OfficeIdentityEmployee,
        name: &str,
        timestamp: u64,
    ) -> Result<FrozenProfile, OfficeIdentityError> {
        let keys = self.keys(entry)?;
        let event = nostr::UnsignedEvent::new(
            keys.public_key(),
            nostr::Timestamp::from(timestamp),
            nostr::Kind::from_u16(buzz_core::kind::KIND_PROFILE as u16),
            [],
            self.profile_content(entry, name),
        )
        .sign_with_keys(keys)
        .map_err(|_| rejected("office_profile_signing_failed"))?;
        let profile = FrozenProfile {
            event_id: event.id.to_hex(),
            bytes: event.as_json().into_bytes(),
            acknowledged: false,
        };
        self.validate_profile(entry, name, &profile)?;
        Ok(profile)
    }

    pub(super) fn validate_profile(
        &self,
        entry: &OfficeIdentityEmployee,
        name: &str,
        profile: &FrozenProfile,
    ) -> Result<(), OfficeIdentityError> {
        if profile.bytes.len() > 16_384 {
            return Err(rejected("office_profile_receipt_invalid"));
        }
        let event = nostr::Event::from_json(&profile.bytes)
            .map_err(|_| rejected("office_profile_receipt_invalid"))?;
        event
            .verify()
            .map_err(|_| rejected("office_profile_receipt_invalid"))?;
        if event.pubkey.to_hex() != entry.office.public_key
            || event.id.to_hex() != profile.event_id
            || event.kind.as_u16() != buzz_core::kind::KIND_PROFILE as u16
            || !event.tags.is_empty()
            || event.content != self.profile_content(entry, name)
        {
            return Err(rejected("office_profile_receipt_invalid"));
        }
        Ok(())
    }

    pub(super) async fn send_profile(
        &self,
        entry: &OfficeIdentityEmployee,
        profile: &FrozenProfile,
    ) -> Result<(), OfficeIdentityError> {
        let url = format!("{}/events", self.config.origin);
        let auth = crate::transport::http_authorization(self.keys(entry)?, &profile.bytes, &url)
            .map_err(|_| rejected("office_profile_authentication_failed"))?;
        let mut response = self
            .client
            .post(&url)
            .header(reqwest::header::AUTHORIZATION, auth)
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(profile.bytes.clone())
            .send()
            .await
            .map_err(|_| unavailable("office_profile_http_failed"))?;
        if !response.status().is_success() {
            return Err(
                if response.status().is_server_error() || response.status().as_u16() == 429 {
                    unavailable("office_profile_http_unavailable")
                } else {
                    rejected("office_profile_http_rejected")
                },
            );
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| unavailable("office_profile_response_interrupted"))?
        {
            if bytes.len() + chunk.len() > 8192 {
                return Err(rejected("office_profile_response_too_large"));
            }
            bytes.extend_from_slice(&chunk);
        }
        #[derive(Deserialize)]
        struct Ack {
            event_id: String,
            accepted: bool,
        }
        let ack: Ack =
            serde_json::from_slice(&bytes).map_err(|_| rejected("office_profile_ack_malformed"))?;
        if !ack.accepted || ack.event_id != profile.event_id {
            return Err(rejected("office_profile_ack_rejected"));
        }
        Ok(())
    }
}
