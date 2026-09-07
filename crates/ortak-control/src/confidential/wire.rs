use super::{ConfidentialWireError as Error, MAX_HEADER_BYTES};
use chrono::{DateTime, Datelike, SecondsFormat, Utc};
use ortak_domain::EmployeeId;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use uuid::Uuid;

// Field order is the cross-language lexical JSON contract, not incidental.
#[derive(Clone, Deserialize, Serialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub(super) struct IdentityWire {
    authority_epoch: String,
    community_id: String,
    company_id: String,
    conversation_id: String,
    employee_id: String,
    employee_lifecycle_epoch: String,
    employee_public_key: String,
    employee_revision_id: String,
    human_public_key: String,
    key_id: String,
    key_version: String,
    office_binding_id: String,
    rumor_id: String,
    run_id: String,
    source_evidence_hash: String,
    source_outer_created_at: String,
    source_outer_id: String,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct HeaderWire {
    pub algorithm: String,
    pub format: String,
    pub identity: IdentityWire,
    pub ordinal: u32,
    pub plaintext_bytes: usize,
    pub purpose: String,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct EnvelopeWire {
    pub ciphertext: String,
    pub header: HeaderWire,
    pub nonce: String,
}

impl IdentityWire {
    pub(super) fn key_claims(&self) -> super::ConfidentialKeyClaims<'_> {
        super::ConfidentialKeyClaims {
            company_id: &self.company_id,
            employee_id: &self.employee_id,
            employee_revision_id: &self.employee_revision_id,
            employee_lifecycle_epoch: &self.employee_lifecycle_epoch,
            office_binding_id: &self.office_binding_id,
            employee_public_key: &self.employee_public_key,
            key_id: &self.key_id,
            key_version: &self.key_version,
        }
    }

    pub(super) fn validate(&self) -> Result<(), Error> {
        for value in [
            &self.authority_epoch,
            &self.employee_lifecycle_epoch,
            &self.key_version,
        ] {
            let parsed = value.parse::<i64>().map_err(|_| Error::Identity)?;
            if parsed < 0 || parsed.to_string() != *value {
                return Err(Error::Identity);
            }
        }
        for value in [
            &self.company_id,
            &self.community_id,
            &self.conversation_id,
            &self.employee_revision_id,
            &self.office_binding_id,
            &self.key_id,
            &self.run_id,
        ] {
            let id = Uuid::parse_str(value).map_err(|_| Error::Identity)?;
            if id.is_nil() || id.to_string() != *value {
                return Err(Error::Identity);
            }
        }
        EmployeeId::parse(self.employee_id.clone()).map_err(|_| Error::Identity)?;
        for value in [
            &self.employee_public_key,
            &self.human_public_key,
            &self.rumor_id,
            &self.source_outer_id,
            &self.source_evidence_hash,
        ] {
            if value.len() != 64
                || !value
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            {
                return Err(Error::Identity);
            }
        }
        if self.employee_public_key == self.human_public_key {
            return Err(Error::Identity);
        }
        let at = DateTime::parse_from_rfc3339(&self.source_outer_created_at)
            .map_err(|_| Error::Identity)?
            .with_timezone(&Utc);
        if !(1970..=9999).contains(&at.year())
            || at.timestamp_subsec_nanos() != 0
            || at.to_rfc3339_opts(SecondsFormat::Micros, true) != self.source_outer_created_at
        {
            return Err(Error::Identity);
        }
        // A later header includes this object; the separate outer limit is also checked.
        encode(self, MAX_HEADER_BYTES)?;
        Ok(())
    }
}

pub(super) fn parse<T: DeserializeOwned>(bytes: &[u8], max: usize) -> Result<T, Error> {
    if bytes.is_empty() || bytes.len() > max {
        return Err(Error::Bound);
    }
    // Canonical objects start immediately with '{'. Derived structs also accept
    // positional arrays, so do not rely on deny_unknown_fields for object shape.
    if bytes.first() != Some(&b'{') {
        return Err(Error::Encoding);
    }
    serde_json::from_slice(bytes).map_err(|_| Error::Encoding)
}

pub(super) fn encode(value: &impl Serialize, max: usize) -> Result<Vec<u8>, Error> {
    let bytes = serde_json::to_vec(value).map_err(|_| Error::Encoding)?;
    if bytes.len() > max {
        return Err(Error::Bound);
    }
    Ok(bytes)
}
