use chrono::{DateTime, Utc};
use ortak_control::{postgres::lock_office_authority_on, CompanyScope};
use serde::Serialize;
use sqlx::Row;
use uuid::Uuid;

use super::{ConfidentialAdmissionError as Error, PgConfidentialRuns, Result};

/// Bounded current-read public metadata for the future signed native facade.
/// Parsing/retaining this object never grants decrypt, admission or publication.
/// The facade must bind `human` to its authenticated signer before calling.
#[derive(Serialize)]
pub struct EncryptedDmAuthority {
    /// Exact version of the protected native authority observation.
    pub format: &'static str,
    /// Selected host company.
    pub company_id: Uuid,
    /// Selected host community.
    pub community_id: Uuid,
    /// Canonical current private two-member channel.
    pub channel_id: Uuid,
    /// Exact current human participant, lowercase hex.
    pub human_public_key: String,
    /// Durable selected employee ID.
    pub employee_id: String,
    /// Exact current employee Office public key, lowercase hex.
    pub employee_public_key: String,
    /// Existing sorted two-key participant hash, lowercase hex.
    pub pair_hash: String,
    /// Immutable explicit pair selection.
    pub selection_id: Uuid,
    /// Decimal string, never lossy JavaScript numeric authority.
    pub selection_generation: String,
    /// Carried Office mutation generation as a decimal string.
    pub office_generation: String,
    /// Version1 uses the exact Office generation; no model/session epoch.
    pub authority_epoch: String,
    /// Exact purpose-selected key version as a decimal string.
    pub key_version: String,
    /// Retained Office binding public identity, no credential reference/material.
    pub office_binding_id: Uuid,
    /// Database observation time.
    pub observed_at: DateTime<Utc>,
    /// At most five seconds; callers still need final current authority checks.
    pub valid_before: DateTime<Utc>,
}
impl PgConfidentialRuns {
    /// Current explicit selection for exactly the signer/human/channel. Starts
    /// no crypto/provider operation and reveals no opaque credential reference.
    pub async fn authority(
        &self,
        scope: &CompanyScope,
        channel: Uuid,
        human: &[u8; 32],
    ) -> Result<Option<EncryptedDmAuthority>> {
        let Some(community) = scope.community_id() else {
            return Ok(None);
        };
        if channel.is_nil() {
            return Ok(None);
        }
        let mut tx = self.pool.begin().await?;
        let office = lock_office_authority_on(&mut tx, scope).await?;
        let rows=sqlx::query("SELECT s.*,encode(ch.participant_hash,'hex') AS pair_hash,clock_timestamp() AS observed_at,least(clock_timestamp()+interval '5 seconds',ch.ttl_deadline,b.valid_until,$5::timestamptz) AS valid_before FROM encrypted_dm_selections s JOIN channels ch ON ch.community_id=s.community_id AND ch.id=s.channel_id JOIN employee_office_bindings b ON b.company_id=s.company_id AND b.id=s.office_binding_id JOIN employees e ON e.company_id=s.company_id AND e.id=s.employee_id JOIN office_routing_cohorts co ON co.company_id=s.company_id AND co.community_id=s.community_id AND co.state='enabled' JOIN office_routing_channels cc ON cc.company_id=co.company_id AND cc.community_id=co.community_id AND cc.channel_id=s.channel_id JOIN office_routing_employees ce ON ce.company_id=s.company_id AND ce.employee_id=s.employee_id WHERE s.company_id=$1 AND s.community_id=$2 AND s.channel_id=$3 AND s.human_public_key=$4 AND s.enabled AND ortak_encrypted_dm_pair_current(s) AND ortak_confidential_runtime_binding(s.company_id,e.active_revision_id) IS NOT NULL LIMIT 2")
            .bind(scope.company_id()).bind(community).bind(channel).bind(human.as_slice()).bind(office.valid_before())
            .fetch_all(&mut *tx).await?;
        if rows.is_empty() {
            tx.commit().await?;
            return Ok(None);
        }
        if rows.len() != 1 {
            return Err(Error::Refused);
        }
        let row = &rows[0];
        let observed_at: DateTime<Utc> = row.try_get("observed_at")?;
        let valid_before: DateTime<Utc> = row.try_get("valid_before")?;
        let result = EncryptedDmAuthority {
            format: "ortak-native-encrypted-dm-authority/1",
            company_id: scope.company_id(),
            community_id: community,
            channel_id: channel,
            human_public_key: hex::encode(human),
            employee_id: row.try_get("employee_id")?,
            employee_public_key: hex::encode(row.try_get::<Vec<u8>, _>("employee_public_key")?),
            pair_hash: row.try_get("pair_hash")?,
            selection_id: row.try_get("selection_id")?,
            selection_generation: row.try_get::<i64, _>("generation")?.to_string(),
            office_generation: office.generation().to_string(),
            authority_epoch: office.generation().to_string(),
            key_version: row.try_get::<i64, _>("key_version")?.to_string(),
            office_binding_id: row.try_get("office_binding_id")?,
            observed_at,
            valid_before: valid_before.min(observed_at + chrono::Duration::seconds(5)),
        };
        if result.valid_before <= result.observed_at {
            return Err(Error::Refused);
        }
        tx.commit().await?;
        Ok(Some(result))
    }
}
