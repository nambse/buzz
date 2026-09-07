//! Human-approved project context has its own immutable attribution, never a fabricated run.
use super::*;
use chrono::{DateTime, Utc};
use sha2::{Digest, Sha256};

/// Exact approval/export identity selected under current database authority.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewedMemoryPin {
    /// D1 fact and remote record identity.
    pub fact_id: Uuid,
    /// Original retained owned memory target.
    pub target_id: Uuid,
    /// Immutable active approval version.
    pub fact_version: i64,
    /// Explicit runtime-consumption epoch; removing opt-in permanently retires old uses.
    pub consumption_epoch: i64,
    /// Original approved UTF-8 text hash.
    pub content_hash: String,
    /// Canonical source evidence hash.
    pub source_hash: String,
    /// Original creation receipt/native resource identity hash.
    pub binding_hash: String,
    /// Original human approval operation.
    pub approval_id: Uuid,
    /// Approving human public key.
    pub approved_by: String,
    /// Immutable permitted-use expiry.
    pub expires_at: DateTime<Utc>,
}

/// Selected remote text plus its exact locally verified approval pins.
#[derive(Clone, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewedMemoryRecord {
    /// Durable approval/export attribution.
    pub pin: ReviewedMemoryPin,
    /// At most 4 KiB of already approved text; deliberately absent from Debug.
    pub content: String,
}

/// Separate bounded reviewed-project portion of a version-three Work snapshot.
#[derive(Clone, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewedMemoryContext {
    /// At most eight records and 8 KiB before the combined snapshot budget.
    pub records: Vec<ReviewedMemoryRecord>,
    /// More selected matches existed than the finite remote budget allowed.
    pub truncated: bool,
}

impl ReviewedMemoryContext {
    pub(crate) fn validate(&self) -> Result<()> {
        let digest = |v: &str| {
            v.len() == 64
                && v.bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        };
        let mut ids = std::collections::BTreeSet::new();
        let mut bytes = 0;
        for record in &self.records {
            let p = &record.pin;
            bytes += record.content.len();
            if p.fact_id.is_nil()
                || p.target_id.is_nil()
                || p.approval_id.is_nil()
                || p.fact_version != 1
                || p.consumption_epoch < 0
                || !ids.insert(p.fact_id)
                || ![
                    &p.content_hash,
                    &p.source_hash,
                    &p.binding_hash,
                    &p.approved_by,
                ]
                .into_iter()
                .all(|v| digest(v))
                || record.content.trim().is_empty()
                || record.content.len() > 4096
                || record
                    .content
                    .chars()
                    .any(|c| c.is_control() && c != '\n' && c != '\t')
                || hex::encode(Sha256::digest(record.content.as_bytes())) != p.content_hash
            {
                return Err(rejected());
            }
        }
        if self.records.len() > 8 || bytes > 8192 {
            return Err(rejected());
        }
        Ok(())
    }

    pub(super) fn rendered(&self) -> Result<Vec<String>> {
        self.validate()?;
        self.records
            .iter()
            .map(|record| {
                serde_json::to_string(&serde_json::json!({
                    "type":"reviewed_project_memory","trust":"untrusted_data","record":record
                }))
                .map_err(|_| rejected())
            })
            .collect()
    }
}
