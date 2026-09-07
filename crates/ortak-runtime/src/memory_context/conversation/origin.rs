use super::*;
use ortak_control::memory::conversation::{
    ConversationAudienceKind, ConversationMemoryDigest, ConversationProvenanceV1,
};

/// Historical requester and canonical source of a v4 run, never live authority.
/// Only a database observation may construct this value inside the runtime.
#[derive(Clone, Deserialize, Eq, PartialEq, Serialize)]
#[serde(try_from = "OriginWire")]
pub struct ConversationMemoryOrigin {
    requester_public_key: String,
    provenance: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OriginWire {
    requester_public_key: String,
    provenance: String,
}

impl TryFrom<OriginWire> for ConversationMemoryOrigin {
    type Error = crate::RunSupervisionError;

    fn try_from(wire: OriginWire) -> Result<Self> {
        let value = Self {
            requester_public_key: wire.requester_public_key,
            provenance: wire.provenance,
        };
        value.parsed_provenance()?;
        Ok(value)
    }
}

impl ConversationMemoryOrigin {
    /// Parse the exact requester/source returned by the canonical SQL resolver.
    /// Structural validation neither authenticates the human nor grants access.
    pub(crate) fn from_observation(requester: &[u8], canonical_provenance: &[u8]) -> Result<Self> {
        if requester.len() != 32 {
            return Err(rejected());
        }
        // The canonical parser checks its byte ceiling before allocation here.
        let parsed = ConversationProvenanceV1::from_canonical_bytes(canonical_provenance)
            .map_err(|_| rejected())?;
        if parsed.audience().kind() != ConversationAudienceKind::Thread {
            return Err(rejected());
        }
        Ok(Self {
            requester_public_key: hex::encode(requester),
            provenance: String::from_utf8(canonical_provenance.to_vec()).map_err(|_| rejected())?,
        })
    }

    /// Canonical lowercase public key of the database-observed requesting human.
    pub fn requester_public_key(&self) -> &str {
        &self.requester_public_key
    }

    /// Exact canonical v1 provenance JSON string, preserved without reserialization.
    pub fn provenance(&self) -> &str {
        &self.provenance
    }

    /// Parse historical canonical fields; this is not a current source/ACL lookup.
    pub fn parsed_provenance(&self) -> Result<ConversationProvenanceV1> {
        ConversationMemoryDigest::parse_hex(&self.requester_public_key).map_err(|_| rejected())?;
        let parsed = ConversationProvenanceV1::from_canonical_bytes(self.provenance.as_bytes())
            .map_err(|_| rejected())?;
        if parsed.audience().kind() != ConversationAudienceKind::Thread {
            return Err(rejected());
        }
        Ok(parsed)
    }

    pub(super) fn validate_for(&self, authority: &DispatchAuthority) -> Result<()> {
        let parsed = self.parsed_provenance()?;
        let audience = parsed.audience();
        if authority.memory_binding().is_none()
            || audience.company_id() != authority.company_id()
            || audience.employee_id() != authority.employee_id()
            || Some(audience.channel_id()) != authority.input().channel_id
        {
            return Err(rejected());
        }
        if let Some(work) = authority.work_origin() {
            if audience.project_id() != work.project_id {
                return Err(rejected());
            }
            // WorkRunOrigin has no source message or requesting human. Their
            // exact retained identity must be re-resolved by the SQL boundary.
        } else if !matches!(authority.input().event_kind, 9 | 40002)
            || authority.routing_decision_id().is_none()
            || Some(parsed.source().event_id()) != authority.message_id()
        {
            return Err(rejected());
        }
        // Never compare the canonical root with the unrelated delivery-chain root.
        Ok(())
    }
}
