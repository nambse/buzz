use sqlx::{PgConnection, Row};
use uuid::Uuid;
use ortak_control::{CompanyScope, postgres::{direct_channel_on, lock_office_authority_on}};

use super::{ConfiguredDmPair, DmJobError, PgDecryptJobs};

impl PgDecryptJobs {
    /// Explicitly registers one configured pair for local decryption only. No
    /// application composition invokes this port before the full confidential gate.
    /// An existing disabled ID is not re-enabled by replay; use set_enabled.
    pub async fn register_pair(&self, scope: &CompanyScope, pair: &ConfiguredDmPair) -> Result<i64,DmJobError> {
        let community=scope.community_id().ok_or(DmJobError::Refused)?;
        if pair.selection_id.is_nil() || pair.channel_id.is_nil() || pair.office_binding_id.is_nil()
            || pair.key_version<0 || pair.human_public_key==pair.employee_public_key {
            return Err(DmJobError::Invalid);
        }
        let mut tx=self.pool.begin().await?;
        lock_office_authority_on(&mut tx,scope).await?;
        // Rust uses the shared current pair resolver. SQL independently guards
        // registration, absent-row races and the exact immutable selected tuple.
        let direct=direct_channel_on(&mut tx,scope.company_id(),Some(community),pair.channel_id)
            .await?.ok_or(DmJobError::Refused)?;
        if !direct.permits_execution() || direct.employee_id!=pair.employee_id
            || direct.human_public_key!=*pair.human_public_key.as_bytes()
            || direct.employee_public_key!=*pair.employee_public_key.as_bytes() {
            return Err(DmJobError::Refused);
        }
        // Serializes the retained cap and enabled-only uniqueness before insert.
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended('ortak-encrypted-dm-selection:'||$1::text,0))")
            .bind(scope.company_id()).execute(&mut *tx).await?;
        if let Some(row)=sqlx::query("SELECT s.*,ortak_encrypted_dm_pair_current(s) AS current FROM encrypted_dm_selections s WHERE company_id=$1 AND selection_id=$2 FOR SHARE")
            .bind(scope.company_id()).bind(pair.selection_id).fetch_optional(&mut *tx).await? {
            let same=row.try_get::<Uuid,_>("community_id")?==community
                && row.try_get::<Uuid,_>("channel_id")?==pair.channel_id
                && row.try_get::<String,_>("employee_id")?==pair.employee_id.as_str()
                && row.try_get::<Vec<u8>,_>("human_public_key")?.as_slice()==pair.human_public_key.as_bytes().as_slice()
                && row.try_get::<Vec<u8>,_>("employee_public_key")?.as_slice()==pair.employee_public_key.as_bytes().as_slice()
                && row.try_get::<Uuid,_>("office_binding_id")?==pair.office_binding_id
                && row.try_get::<i64,_>("key_version")?==pair.key_version
                && row.try_get::<String,_>("decrypt_ref")?==pair.decrypt_ref.as_str();
            if !same || !row.try_get::<bool,_>("enabled")? || !row.try_get::<bool,_>("current")? { return Err(DmJobError::Refused); }
            let generation=row.try_get("generation")?;
            tx.commit().await?;
            return Ok(generation);
        }
        let generation=sqlx::query_scalar("INSERT INTO encrypted_dm_selections(company_id,selection_id,community_id,channel_id,employee_id,human_public_key,employee_public_key,office_binding_id,key_version,decrypt_ref,enabled) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,true) RETURNING generation")
            .bind(scope.company_id()).bind(pair.selection_id).bind(community).bind(pair.channel_id).bind(pair.employee_id.as_str())
            .bind(pair.human_public_key.as_bytes().as_slice()).bind(pair.employee_public_key.as_bytes().as_slice())
            .bind(pair.office_binding_id).bind(pair.key_version).bind(pair.decrypt_ref.as_str()).fetch_one(&mut *tx).await?;
        tx.commit().await?;
        Ok(generation)
    }

    /// Explicit generation-CAS opt-out/re-enable, preserving the original tuple.
    /// Registration alone never silently revives a disabled selection.
    pub async fn set_enabled(&self,scope:&CompanyScope,selection:Uuid,expected_generation:i64,enabled:bool)->Result<i64,DmJobError>{
        let mut tx=self.pool.begin().await?;
        lock_office_authority_on(&mut tx,scope).await?;
        let generation=sqlx::query_scalar("UPDATE encrypted_dm_selections SET enabled=$5 WHERE company_id=$1 AND community_id=$2 AND selection_id=$3 AND generation=$4 RETURNING generation")
            .bind(scope.company_id()).bind(scope.community_id()).bind(selection).bind(expected_generation).bind(enabled)
            .fetch_optional(&mut *tx).await?.ok_or(DmJobError::Stale)?;
        tx.commit().await?;
        Ok(generation)
    }
}

pub(super) async fn lock_selection(c:&mut PgConnection,scope:&CompanyScope,id:Uuid)->Result<(),DmJobError>{
    let found:Option<Uuid>=sqlx::query_scalar("SELECT selection_id FROM encrypted_dm_selections WHERE company_id=$1 AND community_id=$2 AND selection_id=$3 FOR SHARE")
        .bind(scope.company_id()).bind(scope.community_id()).bind(id).fetch_optional(c).await?;
    found.ok_or(DmJobError::Refused).map(|_|())
}
