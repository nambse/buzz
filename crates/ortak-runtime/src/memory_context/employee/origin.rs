use super::*;
use chrono::{DateTime, Datelike, SecondsFormat, Utc};
use ortak_control::MessageId;
use ortak_control::memory::employee::{EmployeeMemoryDigest, EmployeeMemorySourceV1};
use ortak_control::office_identity::OfficePublicKey;
use ortak_domain::EmployeeId;

/// Canonical historical run requester/source observed by the SQL resolver.
/// It contains no sharing approval and grants no current permission by itself.
#[derive(Clone, Deserialize, Eq, PartialEq, Serialize)]
#[serde(try_from = "String", into = "String")]
pub struct EmployeeMemoryOrigin(String);

// Lexicographic field order at each object level is part of the byte contract.
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct OriginWire {
    pub company_id: Uuid,
    pub destination_authority_epoch: i64,
    pub destination_channel_id: Uuid,
    pub employee_id: EmployeeId,
    pub format: String,
    pub requester_public_key: String,
    pub source: SourceWire,
    pub source_authority_epoch: i64,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SourceWire {
    pub author_public_key: String,
    pub channel_id: Uuid,
    pub community_id: Uuid,
    pub event_created_at: String,
    pub event_id: String,
    pub evidence_hash: String,
}

impl TryFrom<String> for EmployeeMemoryOrigin {
    type Error = crate::RunSupervisionError;
    fn try_from(value: String) -> Result<Self> {
        let origin = Self(value);
        origin.parsed()?;
        Ok(origin)
    }
}

impl From<EmployeeMemoryOrigin> for String {
    fn from(value: EmployeeMemoryOrigin) -> Self {
        value.0
    }
}

impl EmployeeMemoryOrigin {
    /// Accepts only bounded canonical SQL observation bytes. This constructor
    /// checks structure; the calling transaction supplies concurrency authority.
    pub(crate) fn from_observation(bytes: &[u8]) -> Result<Self> {
        if bytes.is_empty() || bytes.len() > 2048 {
            return Err(rejected());
        }
        Self::try_from(String::from_utf8(bytes.to_vec()).map_err(|_| rejected())?)
    }

    /// Exact canonical JSON string retained in snapshot format five.
    pub fn canonical_bytes(&self) -> &[u8] {
        self.0.as_bytes()
    }

    /// Historical actual human key; a fresh resolver still checks current access.
    pub fn requester_public_key(&self) -> Result<String> {
        Ok(self.parsed()?.requester_public_key)
    }

    /// Exact historical destination, without authorizing another channel.
    pub fn destination_channel_id(&self) -> Result<Uuid> {
        Ok(self.parsed()?.destination_channel_id)
    }

    pub(crate) fn parsed(&self) -> Result<OriginWire> {
        if self.0.is_empty() || self.0.len() > 2048 {
            return Err(rejected());
        }
        let wire: OriginWire = serde_json::from_str(&self.0).map_err(|_| rejected())?;
        if wire.format != "ortak-reviewed-employee-run-origin/1"
            || wire.company_id.is_nil()
            || wire.destination_channel_id.is_nil()
            || wire.source_authority_epoch < 0
            || wire.destination_authority_epoch < 0
            || wire.requester_public_key != wire.source.author_public_key
            || serde_json::to_vec(&wire).map_err(|_| rejected())? != self.0.as_bytes()
        {
            return Err(rejected());
        }
        wire.source.value()?;
        Ok(wire)
    }

    pub(crate) fn validate_for(&self, authority: &DispatchAuthority) -> Result<()> {
        let wire = self.parsed()?;
        if authority.memory_binding().is_none()
            || wire.company_id != authority.company_id()
            || &wire.employee_id != authority.employee_id()
            || Some(wire.destination_channel_id) != authority.input().channel_id
        {
            return Err(rejected());
        }
        if authority.work_origin().is_none()
            && (!matches!(authority.input().event_kind, 9 | 40002)
                || authority.routing_decision_id().is_none()
                || Some(wire.source.value()?.event_id()) != authority.message_id())
        {
            return Err(rejected());
        }
        // Promoted Work's actual requester/source are deliberately checked by
        // the SQL resolver, not inferred from this structural WorkRunOrigin.
        Ok(())
    }
}

impl SourceWire {
    fn value(&self) -> Result<EmployeeMemorySourceV1> {
        let at = DateTime::parse_from_rfc3339(&self.event_created_at)
            .map_err(|_| rejected())?
            .with_timezone(&Utc);
        if !(1970..=9999).contains(&at.year())
            || at.timestamp_subsec_nanos() % 1000 != 0
            || at.to_rfc3339_opts(SecondsFormat::Micros, true) != self.event_created_at
        {
            return Err(rejected());
        }
        EmployeeMemoryDigest::parse_hex(&self.author_public_key).map_err(|_| rejected())?;
        EmployeeMemorySourceV1::new(
            self.community_id,
            self.channel_id,
            MessageId::from_bytes(
                *EmployeeMemoryDigest::parse_hex(&self.event_id)
                    .map_err(|_| rejected())?
                    .as_bytes(),
            ),
            at,
            OfficePublicKey::parse_hex(&self.author_public_key).map_err(|_| rejected())?,
            EmployeeMemoryDigest::parse_hex(&self.evidence_hash).map_err(|_| rejected())?,
        )
        .map_err(|_| rejected())
    }
}
