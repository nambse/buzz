use base64::{engine::general_purpose::STANDARD, Engine as _};
use nostr::{nips::nip44, Keys};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use zeroize::Zeroizing;

use super::{
    DmKeyError, DmKeySelection, UnwrappedMasterKey, WrappedMasterKey, MAX_KEY_CIPHERTEXT_BYTES,
    MAX_KEY_PLAINTEXT_BYTES, MAX_WRAPPED_MASTER_BYTES,
};

const OUTER_FORMAT: &str = "ortak-confidential-key-envelope/1";
const INNER_FORMAT: &str = "ortak-confidential-key/1";
const PURPOSE: &str = "confidential_master";

// Declaration order is lexical. Re-encoding enforces canonical object-only JSON,
// including whitespace/escaping, in addition to Serde's duplicate/unknown checks.
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Outer {
    ciphertext: String,
    format: String,
    identity: String,
    purpose: String,
    signer_ref: String,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Inner {
    format: String,
    identity: String,
    identity_hash: String,
    key_id: String,
    master_key: SecretText,
    purpose: String,
    signer_ref: String,
}

// Own the only secret JSON field in a wiping buffer, including error/drop paths.
struct SecretText(Zeroizing<String>);

impl Serialize for SecretText {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.0.as_str())
    }
}

impl<'de> Deserialize<'de> for SecretText {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        String::deserialize(deserializer).map(|value| Self(Zeroizing::new(value)))
    }
}

pub(super) fn parse_outer(bytes: &[u8]) -> Result<WrappedMasterKey, DmKeyError> {
    object(bytes, MAX_WRAPPED_MASTER_BYTES)?;
    let outer: Outer = serde_json::from_slice(bytes).map_err(|_| DmKeyError::Envelope)?;
    if outer.format != OUTER_FORMAT
        || outer.purpose != PURPOSE
        || serde_json::to_vec(&outer).map_err(|_| DmKeyError::Envelope)? != bytes
    {
        return Err(DmKeyError::Envelope);
    }
    let identity = ortak_control::confidential::ValidatedIdentity::parse(outer.identity.as_bytes())
        .map_err(|_| DmKeyError::Envelope)?;
    let signer_ref =
        ortak_domain::CredentialRef::parse(&outer.signer_ref).map_err(|_| DmKeyError::Envelope)?;
    ciphertext(&outer.ciphertext)?;
    Ok(WrappedMasterKey {
        identity,
        signer_ref,
        ciphertext: outer.ciphertext,
        bytes: bytes.to_vec(),
    })
}

pub(super) fn wrap(
    keys: &Keys,
    selection: &DmKeySelection,
    master: &Zeroizing<[u8; 32]>,
) -> Result<WrappedMasterKey, DmKeyError> {
    let identity = std::str::from_utf8(selection.identity.canonical_bytes())
        .map_err(|_| DmKeyError::Envelope)?
        .to_owned();
    let inner = Inner {
        format: INNER_FORMAT.into(),
        identity: identity.clone(),
        identity_hash: hex::encode(selection.identity.sha256()),
        key_id: selection.identity.key_claims().key_id.into(),
        master_key: SecretText(Zeroizing::new(STANDARD.encode(master.as_slice()))),
        purpose: PURPOSE.into(),
        signer_ref: selection.signer_ref.as_str().into(),
    };
    let plaintext = Zeroizing::new(serde_json::to_vec(&inner).map_err(|_| DmKeyError::Envelope)?);
    object(&plaintext, MAX_KEY_PLAINTEXT_BYTES)?;
    // Pinned NIP-44 v2 draws its own fresh nonce. There is no caller nonce API.
    let ciphertext = nip44::encrypt(
        keys.secret_key(),
        &keys.public_key(),
        plaintext.as_slice(),
        nip44::Version::V2,
    )
    .map_err(|_| DmKeyError::Authentication)?;
    let outer = Outer {
        ciphertext,
        format: OUTER_FORMAT.into(),
        identity,
        purpose: PURPOSE.into(),
        signer_ref: selection.signer_ref.as_str().into(),
    };
    let bytes = serde_json::to_vec(&outer).map_err(|_| DmKeyError::Envelope)?;
    parse_outer(&bytes)
}

pub(super) fn unwrap(
    keys: &Keys,
    selection: &DmKeySelection,
    wrapped: &WrappedMasterKey,
) -> Result<UnwrappedMasterKey, DmKeyError> {
    // Retained metadata was checked before key resolution. This repeats only the
    // bounded cipher check before the library decodes/allocates its own buffers.
    ciphertext(&wrapped.ciphertext)?;
    let plaintext = Zeroizing::new(
        nip44::decrypt_to_bytes(keys.secret_key(), &keys.public_key(), &wrapped.ciphertext)
            .map_err(|_| DmKeyError::Authentication)?,
    );
    object(&plaintext, MAX_KEY_PLAINTEXT_BYTES).map_err(|_| DmKeyError::Authentication)?;
    let inner: Inner =
        serde_json::from_slice(&plaintext).map_err(|_| DmKeyError::Authentication)?;
    let canonical =
        Zeroizing::new(serde_json::to_vec(&inner).map_err(|_| DmKeyError::Authentication)?);
    if canonical.as_slice() != plaintext.as_slice()
        || inner.format != INNER_FORMAT
        || inner.purpose != PURPOSE
        || inner.identity.as_bytes() != selection.identity.canonical_bytes()
        || inner.identity_hash != hex::encode(selection.identity.sha256())
        || inner.key_id != selection.identity.key_claims().key_id
        || inner.signer_ref != selection.signer_ref.as_str()
        || inner.master_key.0.len() != 44
    {
        return Err(DmKeyError::Authentication);
    }
    let master = Zeroizing::new(
        STANDARD
            .decode(inner.master_key.0.as_bytes())
            .map_err(|_| DmKeyError::Authentication)?,
    );
    let canonical_master = Zeroizing::new(STANDARD.encode(master.as_slice()));
    if master.len() != 32 || canonical_master.as_str() != inner.master_key.0.as_str() {
        return Err(DmKeyError::Authentication);
    }
    let mut owned = Zeroizing::new([0u8; 32]);
    owned.copy_from_slice(&master);
    Ok(UnwrappedMasterKey(owned))
}

fn object(bytes: &[u8], maximum: usize) -> Result<(), DmKeyError> {
    if bytes.len() > maximum || bytes.first() != Some(&b'{') {
        return Err(DmKeyError::Envelope);
    }
    Ok(())
}

fn ciphertext(value: &str) -> Result<(), DmKeyError> {
    // 99 bytes is NIP-44 v2's minimum payload. The encoded upper bound is checked
    // before allocating, and canonical base64 rejects whitespace/padding aliases.
    if value.len() < 132 || value.len() > MAX_KEY_CIPHERTEXT_BYTES {
        return Err(DmKeyError::Envelope);
    }
    let raw = STANDARD
        .decode(value.as_bytes())
        .map_err(|_| DmKeyError::Envelope)?;
    if raw.len() < 99 || raw.first() != Some(&2) || STANDARD.encode(&raw) != value {
        return Err(DmKeyError::Envelope);
    }
    Ok(())
}
