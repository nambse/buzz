//! Operation-specific key ports. Mount only with the two new closed purposes.
use super::{
    read_exact_env, resolve_exact, DmKeyError, DmKeySelection, EnvDmKeyProvider, OfficeKeyPurpose,
};
use crate::encrypted::{
    decode, jobs::DmDecryptClaim, DmDecryptKey, VerifiedDmRumor, MAX_OUTER_BYTES, MAX_TEXT_BYTES,
};
use chrono::Utc;
use nostr::{EventBuilder, EventId, JsonUtil, PublicKey, RelayUrl, Tag};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use zeroize::{Zeroize, Zeroizing};

/// Immutable two-copy output; only a purpose-selected provider can construct it.
/// Both envelopes carry exactly one shared rumor and must freeze atomically.
pub struct SealedDmReply {
    rumor_id: [u8; 32],
    rumor_hash: [u8; 32],
    copies: [SealedDmCopy; 2],
}
/// Signed ciphertext only, without plaintext, Office key or generic signing port.
pub struct SealedDmCopy {
    id: [u8; 32],
    bytes: Vec<u8>,
}
impl SealedDmReply {
    /// Shared unsigned rumor identity, independent of wrapper IDs.
    pub fn rumor_id(&self) -> &[u8; 32] {
        &self.rumor_id
    }
    /// Digest of the exact one serialized unsigned rumor supplied to both wraps.
    pub fn rumor_hash(&self) -> &[u8; 32] {
        &self.rumor_hash
    }
    /// Recipient copy followed by employee sender-history copy.
    pub fn copies(&self) -> &[SealedDmCopy; 2] {
        &self.copies
    }
}
impl SealedDmCopy {
    /// Exact signed outer ID checked before publication.
    pub fn id(&self) -> &[u8; 32] {
        &self.id
    }
    /// Frozen signed JSON. Retries never rewrap or resign it.
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}
#[derive(Deserialize)]
struct ReplyIdentity {
    human_public_key: String,
    employee_public_key: String,
    rumor_id: String,
}
struct SecretRumor(nostr::UnsignedEvent);
impl Drop for SecretRumor {
    fn drop(&mut self) {
        self.0.content.zeroize();
    }
}

impl EnvDmKeyProvider {
    pub(crate) fn validate_reply_copy(
        &self,
        selection: &DmKeySelection,
        ordinal: u8,
        bytes: &[u8],
    ) -> Result<(), DmKeyError> {
        self.selected(selection, OfficeKeyPurpose::DmSeal)?;
        let id: ReplyIdentity = serde_json::from_slice(selection.identity.canonical_bytes())
            .map_err(|_| DmKeyError::Envelope)?;
        let target = match ordinal {
            0 => id.human_public_key,
            1 => id.employee_public_key,
            _ => return Err(DmKeyError::Refused),
        };
        let target = PublicKey::from_hex(&target).map_err(|_| DmKeyError::Envelope)?;
        let event = crate::encrypted::wire::signed(bytes, MAX_OUTER_BYTES, 1059)
            .map_err(|_| DmKeyError::Envelope)?;
        crate::encrypted::wire::outer_recipient(&event, target).map_err(|_| DmKeyError::Refused)
    }
    /// Resolves only the exact job-selected dm_decrypt entry. The repository
    /// must check the same claim immediately before and after this bounded call.
    /// A provider allowlist/crypto success is never current database authority.
    pub fn decrypt_claim(&self, claim: &DmDecryptClaim) -> Result<VerifiedDmRumor, DmKeyError> {
        if Utc::now() >= claim.crypto_deadline() {
            return Err(DmKeyError::Refused);
        }
        let id = claim.identity();
        let binding = self
            .bindings
            .iter()
            .find(|b| {
                b.purposes.contains(&OfficeKeyPurpose::DmDecrypt)
                    && b.signer.company_id == id.company_id
                    && b.signer.employee_id == id.employee_id
                    && b.office_binding_id == id.office_binding_id
                    && b.key_version.to_string() == id.key_version.to_string()
                    && b.signer.signer_ref == id.decrypt_ref
                    && *b.signer.public_key.as_bytes() == claim.expected().recipient().to_bytes()
            })
            .ok_or(DmKeyError::Refused)?;
        let keys = resolve_exact(binding, &mut read_exact_env)?;
        let key = DmDecryptKey::for_recipient(&keys, claim.expected().recipient())
            .map_err(|_| DmKeyError::Refused)?;
        let verified = decode(&key, claim.expected(), claim.outer_bytes())
            .map_err(|_| DmKeyError::Authentication)?;
        if Utc::now() >= claim.crypto_deadline() {
            return Err(DmKeyError::Refused);
        }
        Ok(verified)
    }

    /// Produces one rumor and two frozen NIP-59 copies under the exact dm_seal
    /// entry. Source/revision/pair and effect authority remain caller duties.
    /// Call once before atomic persistence, never as a publication retry.
    pub async fn seal_reply(
        &self,
        selection: &DmKeySelection,
        text: &str,
    ) -> Result<SealedDmReply, DmKeyError> {
        self.seal_with_reader(selection, text, read_exact_env).await
    }
    async fn seal_with_reader(
        &self,
        selection: &DmKeySelection,
        text: &str,
        mut read: impl FnMut(&str) -> Result<String, DmKeyError>,
    ) -> Result<SealedDmReply, DmKeyError> {
        if text.is_empty()
            || text.len() > MAX_TEXT_BYTES
            || text
                .chars()
                .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
        {
            return Err(DmKeyError::Refused);
        }
        let binding = self.selected(selection, OfficeKeyPurpose::DmSeal)?;
        let id: ReplyIdentity = serde_json::from_slice(selection.identity.canonical_bytes())
            .map_err(|_| DmKeyError::Envelope)?;
        let human = PublicKey::from_hex(&id.human_public_key).map_err(|_| DmKeyError::Envelope)?;
        let employee =
            PublicKey::from_hex(&id.employee_public_key).map_err(|_| DmKeyError::Envelope)?;
        let parent = EventId::from_hex(&id.rumor_id).map_err(|_| DmKeyError::Envelope)?;
        let keys = resolve_exact(binding, &mut read)?;
        let tag =
            Tag::parse(["e", &parent.to_hex(), "", "reply"]).map_err(|_| DmKeyError::Envelope)?;
        let mut rumor = SecretRumor(
            EventBuilder::private_msg_rumor(human, text)
                .tags([tag])
                .build(employee),
        );
        rumor.0.ensure_id();
        let rumor_id = rumor.0.id.ok_or(DmKeyError::Envelope)?.to_bytes();
        let rumor_json = Zeroizing::new(rumor.0.as_json());
        if rumor_json.len() > crate::encrypted::MAX_RUMOR_BYTES {
            return Err(DmKeyError::Envelope);
        }
        let rumor_hash = Sha256::digest(rumor_json.as_bytes()).into();
        let recipient = EventBuilder::gift_wrap(&keys, &human, rumor.0.clone(), [])
            .await
            .map_err(|_| DmKeyError::Authentication)?;
        let history = EventBuilder::gift_wrap(&keys, &employee, rumor.0.clone(), [])
            .await
            .map_err(|_| DmKeyError::Authentication)?;
        let copies = [
            SealedDmCopy {
                id: recipient.id.to_bytes(),
                bytes: recipient.as_json().into_bytes(),
            },
            SealedDmCopy {
                id: history.id.to_bytes(),
                bytes: history.as_json().into_bytes(),
            },
        ];
        if copies.iter().any(|c| c.bytes.len() > MAX_OUTER_BYTES) || copies[0].id == copies[1].id {
            return Err(DmKeyError::Envelope);
        }
        Ok(SealedDmReply {
            rumor_id,
            rumor_hash,
            copies,
        })
    }

    /// Signs only a bounded NIP-42 AUTH challenge for the caller's exact selected
    /// relay. No generic signing callback/key getter is exposed. The encrypted
    /// publisher must retain current effect authority through this operation.
    pub(crate) fn auth_challenge(
        &self,
        selection: &DmKeySelection,
        relay: &RelayUrl,
        challenge: &str,
    ) -> Result<nostr::Event, DmKeyError> {
        if challenge.is_empty() || challenge.len() > 256 || challenge.chars().any(char::is_control)
        {
            return Err(DmKeyError::Refused);
        }
        let binding = self.selected(selection, OfficeKeyPurpose::DmSeal)?;
        let keys = resolve_exact(binding, &mut read_exact_env)?;
        EventBuilder::auth(challenge, relay.clone())
            .sign_with_keys(&keys)
            .map_err(|_| DmKeyError::Authentication)
    }
}

#[cfg(test)]
mod tests;
