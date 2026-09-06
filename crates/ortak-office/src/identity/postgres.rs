use chrono::{DateTime, Utc};
use sqlx::{PgConnection, Row};

use super::profile::FrozenProfile;
use super::{
    rejected, unavailable, OfficeIdentityEmployee, OfficeIdentityError, OfficePublicKey,
    PgOfficeIdentityAdapter,
};

fn database_error(_: sqlx::Error) -> OfficeIdentityError {
    // Never retain a PostgreSQL diagnostic: it may contain row contents.
    unavailable("office_identity_database_unavailable")
}

impl PgOfficeIdentityAdapter {
    async fn begin(&self) -> Result<sqlx::Transaction<'_, sqlx::Postgres>, OfficeIdentityError> {
        let mut tx = self.control.pool().begin().await.map_err(database_error)?;
        sqlx::query("SELECT set_config('lock_timeout','500ms',true), set_config('statement_timeout','2s',true), set_config('idle_in_transaction_session_timeout','5s',true)")
            .execute(&mut *tx).await.map_err(database_error)?;
        sqlx::query("SELECT ortak_lock_office_authority($1)")
            .bind(self.config.company_id)
            .execute(&mut *tx)
            .await
            .map_err(database_error)?;
        Ok(tx)
    }

    async fn membership_on(
        &self,
        connection: &mut PgConnection,
        entry: &OfficeIdentityEmployee,
    ) -> Result<(), OfficeIdentityError> {
        let public_key = OfficePublicKey::parse_hex(&entry.office.public_key)?;
        let valid: bool = sqlx::query_scalar(
            "SELECT EXISTS (
               SELECT 1 FROM companies co
               JOIN office_company_bindings b ON b.company_id=co.id AND b.community_id=$2
               JOIN communities c ON c.id=b.community_id AND c.deletion_state='active'
                   AND c.deleted_at IS NULL AND lower(c.host)=$3
               JOIN employees e ON e.company_id=co.id AND e.id=$4
               JOIN relay_members rm ON rm.community_id=c.id AND rm.pubkey=$5
               WHERE co.id=$1 AND co.status='active'
                 AND NOT EXISTS (SELECT 1 FROM users u WHERE u.community_id=c.id
                                 AND u.pubkey=$6 AND u.deactivated_at IS NOT NULL)
                 AND NOT EXISTS (
                   SELECT 1 FROM unnest($7::uuid[]) required(channel_id)
                   WHERE NOT EXISTS (
                     SELECT 1 FROM channels ch JOIN channel_members m
                       ON m.community_id=ch.community_id AND m.channel_id=ch.id
                       AND m.pubkey=$6 AND m.removed_at IS NULL
                     WHERE ch.community_id=c.id AND ch.id=required.channel_id
                       AND ch.channel_type IN ('stream','forum','dm')
                       AND ch.deleted_at IS NULL AND ch.archived_at IS NULL
                       AND (ch.ttl_deadline IS NULL OR ch.ttl_deadline>clock_timestamp()))))",
        )
        .bind(self.config.company_id)
        .bind(self.config.community_id)
        .bind(buzz_core::tenant::relay_url_authority(&self.config.origin))
        .bind(entry.employee_id.as_str())
        .bind(&entry.office.public_key)
        .bind(public_key.as_bytes().as_slice())
        .bind(&entry.channels)
        .fetch_one(&mut *connection)
        .await
        .map_err(database_error)?;
        if !valid {
            return Err(rejected("office_current_membership_unavailable"));
        }
        let direct_channels: Vec<uuid::Uuid> = sqlx::query_scalar(
            "SELECT id FROM channels WHERE community_id=$1 AND id=ANY($2) AND channel_type='dm'",
        )
        .bind(self.config.community_id)
        .bind(&entry.channels)
        .fetch_all(&mut *connection)
        .await
        .map_err(database_error)?;
        for channel in direct_channels {
            let direct = ortak_control::postgres::direct_channel_on(
                connection,
                self.config.company_id,
                Some(self.config.community_id),
                channel,
            )
            .await
            .map_err(|_| unavailable("office_identity_database_unavailable"))?;
            if !direct.is_some_and(|direct| {
                direct.permits_execution()
                    && direct.employee_id == entry.employee_id
                    && direct.employee_public_key == *public_key.as_bytes()
            }) {
                return Err(rejected("office_current_membership_unavailable"));
            }
        }
        Ok(())
    }

    pub(super) async fn check_membership(
        &self,
        entry: &OfficeIdentityEmployee,
    ) -> Result<(), OfficeIdentityError> {
        let mut tx = self.begin().await?;
        self.membership_on(&mut tx, entry).await?;
        tx.commit().await.map_err(database_error)
    }

    async fn operation_on(
        &self,
        connection: &mut PgConnection,
        entry: &OfficeIdentityEmployee,
        name: &str,
        key: &str,
    ) -> Result<(), OfficeIdentityError> {
        // The sole mutation authority is an already-running, non-dry-run
        // provisioning step whose immutable manifest names exactly these fields.
        let office = serde_json::to_value(&entry.office)
            .map_err(|_| rejected("office_profile_invalid_request"))?;
        let row = sqlx::query(
            "SELECT s.operation_id FROM provisioning_operation_steps s
             JOIN provisioning_operations o ON o.company_id=s.company_id AND o.id=s.operation_id
             WHERE s.company_id=$1 AND s.idempotency_key=$2
               AND s.step_name='publish_office_profile' AND s.state='running'
               AND o.status='running' AND NOT o.dry_run AND o.employee_id=$3
               AND o.mode IN ('adopt','update') AND o.manifest->>'provisioning'='adopt'
               AND o.manifest #>> '{employee,id}'=$3 AND o.manifest #>> '{employee,name}'=$4
               AND o.manifest #> '{employee,office}'=$5
             FOR SHARE OF o,s",
        )
        .bind(self.config.company_id)
        .bind(key)
        .bind(entry.employee_id.as_str())
        .bind(name)
        .bind(office)
        .fetch_optional(connection)
        .await
        .map_err(database_error)?;
        if row.is_none() {
            return Err(rejected("office_profile_step_not_authorized"));
        }
        Ok(())
    }

    pub(super) async fn freeze_profile(
        &self,
        entry: &OfficeIdentityEmployee,
        name: &str,
        key: &str,
    ) -> Result<FrozenProfile, OfficeIdentityError> {
        let mut tx = self.begin().await?;
        self.membership_on(&mut tx, entry).await?;
        self.operation_on(&mut tx, entry, name, key).await?;
        let hash = self.profile_hash(entry, name);
        // Serialize first-writer creation per step so restarts never mint a
        // second timestamp or signature. The lock is released before HTTP.
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))")
            .bind(format!(
                "ortak-office-profile-v1:{}:{key}",
                self.config.company_id
            ))
            .execute(&mut *tx)
            .await
            .map_err(database_error)?;
        let existing = sqlx::query(
            "SELECT community_id,employee_id,request_hash,event_id,signed_event_bytes,
                    acknowledged_at IS NOT NULL AS acknowledged
             FROM office_identity_profiles WHERE company_id=$1 AND idempotency_key=$2",
        )
        .bind(self.config.company_id)
        .bind(key)
        .fetch_optional(&mut *tx)
        .await
        .map_err(database_error)?;
        let profile = if let Some(row) = existing {
            if row
                .try_get::<uuid::Uuid, _>("community_id")
                .map_err(database_error)?
                != self.config.community_id
                || row
                    .try_get::<String, _>("employee_id")
                    .map_err(database_error)?
                    != entry.employee_id.as_str()
                || row
                    .try_get::<Vec<u8>, _>("request_hash")
                    .map_err(database_error)?
                    != hash
            {
                return Err(rejected("office_profile_idempotency_conflict"));
            }
            FrozenProfile {
                event_id: hex::encode(
                    row.try_get::<Vec<u8>, _>("event_id")
                        .map_err(database_error)?,
                ),
                bytes: row.try_get("signed_event_bytes").map_err(database_error)?,
                acknowledged: row.try_get("acknowledged").map_err(database_error)?,
            }
        } else {
            let public_key = OfficePublicKey::parse_hex(&entry.office.public_key)?;
            let (now, latest): (DateTime<Utc>, Option<DateTime<Utc>>) = sqlx::query_as(
                "SELECT clock_timestamp(),(SELECT created_at FROM events WHERE community_id=$1
                 AND pubkey=$2 AND kind=0 AND channel_id IS NULL AND deleted_at IS NULL
                 ORDER BY created_at DESC,id ASC LIMIT 1)",
            )
            .bind(self.config.community_id)
            .bind(public_key.as_bytes().as_slice())
            .fetch_one(&mut *tx)
            .await
            .map_err(database_error)?;
            // A same-second update must be newer independent of the event-id
            // tie break. Refuse pathological future heads rather than stamp an
            // unbounded future event; this never changes frozen retry bytes.
            let timestamp = latest.map_or(now.timestamp(), |latest| {
                now.timestamp().max(latest.timestamp().saturating_add(1))
            });
            if timestamp < 0 || timestamp > now.timestamp().saturating_add(30) {
                return Err(rejected("office_profile_future_head"));
            }
            let profile = self.sign_profile(entry, name, timestamp as u64)?;
            let event_id = hex::decode(&profile.event_id)
                .map_err(|_| rejected("office_profile_receipt_invalid"))?;
            sqlx::query(
                "INSERT INTO office_identity_profiles (company_id,idempotency_key,community_id,
                 employee_id,request_hash,event_id,signed_event_bytes) VALUES ($1,$2,$3,$4,$5,$6,$7)",
            ).bind(self.config.company_id).bind(key).bind(self.config.community_id)
                .bind(entry.employee_id.as_str()).bind(hash.as_slice()).bind(event_id).bind(&profile.bytes)
                .execute(&mut *tx).await.map_err(database_error)?;
            profile
        };
        self.validate_profile(entry, name, &profile)?;
        tx.commit().await.map_err(database_error)?;
        Ok(profile)
    }

    pub(super) async fn authorize_profile(
        &self,
        entry: &OfficeIdentityEmployee,
        name: &str,
        key: &str,
        profile: &FrozenProfile,
    ) -> Result<(), OfficeIdentityError> {
        let mut tx = self.begin().await?;
        self.membership_on(&mut tx, entry).await?;
        self.operation_on(&mut tx, entry, name, key).await?;
        self.frozen_on(&mut tx, key, profile).await?;
        tx.commit().await.map_err(database_error)
    }

    async fn frozen_on(
        &self,
        connection: &mut PgConnection,
        key: &str,
        profile: &FrozenProfile,
    ) -> Result<(), OfficeIdentityError> {
        let matches: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM office_identity_profiles WHERE company_id=$1 AND idempotency_key=$2
             AND encode(event_id,'hex')=$3 AND signed_event_bytes=$4)",
        ).bind(self.config.company_id).bind(key).bind(&profile.event_id).bind(&profile.bytes)
            .fetch_one(connection).await.map_err(database_error)?;
        if !matches {
            return Err(rejected("office_profile_receipt_invalid"));
        }
        Ok(())
    }

    pub(super) async fn acknowledge_profile(
        &self,
        entry: &OfficeIdentityEmployee,
        name: &str,
        key: &str,
        profile: &FrozenProfile,
    ) -> Result<(), OfficeIdentityError> {
        let mut tx = self.begin().await?;
        self.membership_on(&mut tx, entry).await?;
        self.operation_on(&mut tx, entry, name, key).await?;
        self.frozen_on(&mut tx, key, profile).await?;
        let public_key = OfficePublicKey::parse_hex(&entry.office.public_key)?;
        // A remote ACK alone cannot establish that this relay accepted this
        // exact profile as its live head or that the visible name was projected.
        let current: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM events e JOIN users u
             ON u.community_id=e.community_id AND u.pubkey=e.pubkey
             WHERE e.community_id=$1 AND e.pubkey=$2 AND e.kind=0 AND e.channel_id IS NULL
               AND e.deleted_at IS NULL AND encode(e.id,'hex')=$3 AND u.display_name=$4
               AND e.content=$5 AND u.deactivated_at IS NULL
               AND NOT EXISTS (SELECT 1 FROM events newer WHERE newer.community_id=e.community_id
                 AND newer.pubkey=e.pubkey AND newer.kind=0 AND newer.channel_id IS NULL AND newer.deleted_at IS NULL
                 AND (newer.created_at>e.created_at OR (newer.created_at=e.created_at AND newer.id<e.id))))",
        ).bind(self.config.community_id).bind(public_key.as_bytes().as_slice()).bind(&profile.event_id)
            .bind(name).bind(self.profile_content(entry, name)).fetch_one(&mut *tx).await.map_err(database_error)?;
        if !current {
            return Err(unavailable("office_profile_canonical_receipt_missing"));
        }
        sqlx::query("UPDATE office_identity_profiles SET acknowledged_at=coalesce(acknowledged_at,clock_timestamp()) WHERE company_id=$1 AND idempotency_key=$2")
            .bind(self.config.company_id).bind(key).execute(&mut *tx).await.map_err(database_error)?;
        tx.commit().await.map_err(database_error)
    }
}
