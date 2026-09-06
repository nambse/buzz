use base64::Engine;
use nostr::nips::nip44;
use sha2::{Digest, Sha256};

use super::*;

fn decrypt(
    key: &DmDecryptKey<'_>,
    sender: PublicKey,
    ciphertext: &str,
    maximum: usize,
) -> Result<Zeroizing<Vec<u8>>, DecodeError> {
    // signed() already enforces the stage-specific ciphertext bound. Verify
    // the version with a fixed three-byte decode before the library allocates
    // or authenticates the complete payload. Never try a different cipher/key.
    let prefix = ciphertext
        .as_bytes()
        .get(..4)
        .ok_or(DecodeError::Decryption)?;
    let prefix = base64::engine::general_purpose::STANDARD
        .decode(prefix)
        .map_err(|_| DecodeError::Decryption)?;
    if prefix.first() != Some(&2) {
        return Err(DecodeError::Decryption);
    }
    let bytes = Zeroizing::new(
        nip44::decrypt_to_bytes(key.keys.secret_key(), &sender, ciphertext)
            .map_err(|_| DecodeError::Decryption)?,
    );
    if bytes.len() > maximum {
        return Err(DecodeError::Bounds);
    }
    Ok(bytes)
}

pub(super) fn decode(
    key: &DmDecryptKey<'_>,
    expected: &ExpectedEnvelope,
    outer_bytes: &[u8],
) -> Result<VerifiedDmRumor, DecodeError> {
    if key.recipient != expected.recipient {
        return Err(DecodeError::KeyMismatch);
    }
    let outer = wire::signed(outer_bytes, MAX_OUTER_BYTES, 1059)?;
    if outer.id != expected.outer_id
        || outer.pubkey != expected.outer_author
        || outer.created_at.as_secs() != wire::partition_seconds(expected.partition_at)?
    {
        return Err(DecodeError::SourceMismatch);
    }
    wire::outer_recipient(&outer, expected.recipient)?;
    let seal_bytes = decrypt(key, outer.pubkey, &outer.content, MAX_SEAL_BYTES)?;
    let seal = wire::signed(&seal_bytes, MAX_SEAL_BYTES, 13)?;
    if seal.pubkey != expected.human {
        return Err(DecodeError::Sender);
    }
    let rumor_bytes = decrypt(key, seal.pubkey, &seal.content, MAX_RUMOR_BYTES)?;
    let rumor = wire::rumor(&rumor_bytes, expected)?;
    Ok(VerifiedDmRumor {
        source: *expected,
        outer_hash: Sha256::digest(outer_bytes).into(),
        seal_id: seal.id,
        seal_created_at: wire::timestamp(seal.created_at.as_secs())?,
        rumor_id: rumor.id,
        rumor_created_at: rumor.created_at,
        rumor_hash: Sha256::digest(rumor_bytes.as_slice()).into(),
        reply_to: rumor.reply_to,
        text: rumor.text,
        rumor_bytes,
    })
}
