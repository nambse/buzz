//! Only two derived-purpose keys enter a protected runtime start. This pure
//! encoding port does not establish a current lease, source or employee right.
use base64::{Engine, engine::general_purpose::STANDARD};
use ortak_control::confidential::{
    ConfidentialEnvelope, ConfidentialWireError, PayloadPurpose, ValidatedIdentity,
};
use serde::Deserialize;
use uuid::Uuid;
use zeroize::Zeroizing;

use super::{ConfidentialCryptoError, ConfidentialMasterKey, derive};

/// Volatile exact start body. No Debug, Clone, Serde or raw derived-key getter.
/// Its owner must recheck current admission immediately before sending it.
pub struct ConfidentialStartBody {
    pub(crate) bytes: Zeroizing<Vec<u8>>,
    pub(crate) company_id: Uuid,
    pub(crate) run_id: Uuid,
}

#[derive(Deserialize)]
pub(super) struct TransportSelection {
    pub company_id: Uuid,
    pub run_id: Uuid,
}

pub(super) fn selection(
    identity: &ValidatedIdentity,
) -> Result<TransportSelection, ConfidentialCryptoError> {
    // The full canonical identity has already rejected unknown/malformed fields.
    serde_json::from_slice(identity.canonical_bytes())
        .map_err(|_| ConfidentialWireError::Identity.into())
}

/// Encode one current-authorized start for the exact immutable snapshot.
/// The master and Office key never enter the body. Lookup/replay/cancellation
/// use distinct keyless methods and must not call this function.
pub fn prepare_start_body(
    master: &ConfidentialMasterKey,
    identity: &ValidatedIdentity,
    snapshot: &ConfidentialEnvelope,
) -> Result<ConfidentialStartBody, ConfidentialCryptoError> {
    snapshot
        .header()
        .require_expected(identity, PayloadPurpose::Snapshot, 0)?;
    let selected = selection(identity)?;
    let snapshot_key = derive(master, identity, PayloadPurpose::Snapshot)?;
    let event_key = derive(master, identity, PayloadPurpose::RuntimeEvent)?;
    let snapshot64 = Zeroizing::new(STANDARD.encode(snapshot_key.as_ref()));
    let event64 = Zeroizing::new(STANDARD.encode(event_key.as_ref()));
    // All interpolated fields are canonical UUID/base64 or previously validated
    // canonical ciphertext JSON. No arbitrary string bypasses JSON escaping.
    let mut bytes = Zeroizing::new(Vec::with_capacity(snapshot.canonical_bytes().len() + 256));
    bytes.extend_from_slice(b"{\"company_id\":\"");
    bytes.extend_from_slice(selected.company_id.to_string().as_bytes());
    bytes.extend_from_slice(b"\",\"keys\":{\"runtime_event\":\"");
    bytes.extend_from_slice(event64.as_bytes());
    bytes.extend_from_slice(b"\",\"snapshot\":\"");
    bytes.extend_from_slice(snapshot64.as_bytes());
    bytes.extend_from_slice(b"\"},\"snapshot\":");
    bytes.extend_from_slice(snapshot.canonical_bytes());
    bytes.push(b'}');
    if bytes.len() > 112 * 1024 {
        return Err(ConfidentialWireError::Bound.into());
    }
    Ok(ConfidentialStartBody {
        bytes,
        company_id: selected.company_id,
        run_id: selected.run_id,
    })
}
