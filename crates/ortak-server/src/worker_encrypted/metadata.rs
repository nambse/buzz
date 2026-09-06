use super::WorkerEncrypted;
use ortak_control::{postgres::lock_office_authority_on, CompanyScope};
use ortak_domain::{Employee, PermissionPolicy};
use sqlx::Row;
use uuid::Uuid;

impl WorkerEncrypted {
    pub(super) async fn allowed_pairs(
        &self,
        scope: &CompanyScope,
    ) -> Result<Vec<Uuid>, &'static str> {
        if !self.capable {
            return Ok(Vec::new());
        }
        let Some(config) = &self.selected else {
            return Ok(Vec::new());
        };
        // At most 16 explicit IDs; no employee/key discovery. Full metadata and
        // truly empty selected policy are verified before even enqueueing work.
        let rows = sqlx::query("SELECT s.selection_id,s.employee_id,s.employee_public_key,
            s.office_binding_id,s.key_version,s.decrypt_ref,r.manifest
            FROM encrypted_dm_selections s JOIN employees e ON e.company_id=s.company_id AND e.id=s.employee_id
            JOIN employee_revisions r ON r.company_id=e.company_id AND r.employee_id=e.id AND r.id=e.active_revision_id
            JOIN office_routing_cohorts co ON co.company_id=s.company_id AND co.community_id=s.community_id AND co.state='enabled'
            JOIN office_routing_channels ch ON ch.company_id=co.company_id AND ch.community_id=co.community_id AND ch.channel_id=s.channel_id
            JOIN office_routing_employees ce ON ce.company_id=co.company_id AND ce.employee_id=e.id
            WHERE s.company_id=$1 AND s.community_id=$2 AND s.selection_id=ANY($3)
              AND s.enabled AND ortak_encrypted_dm_pair_current(s) ORDER BY s.selection_id LIMIT 16")
            .bind(scope.company_id()).bind(scope.community_id()).bind(&config.pairs)
            .fetch_all(self.control.pool()).await.map_err(|_| "encrypted selection read failed")?;
        let mut allowed = Vec::new();
        for row in rows {
            let employee: Employee = serde_json::from_value(
                row.try_get("manifest")
                    .map_err(|_| "encrypted manifest unavailable")?,
            )
            .map_err(|_| "encrypted manifest invalid")?;
            if employee.validate_definition().is_err()
                || employee.permissions != PermissionPolicy::default()
                || employee.runtime.adapter != "hermes"
            {
                continue;
            }
            let employee_id: String = row
                .try_get("employee_id")
                .map_err(|_| "encrypted selection invalid")?;
            let public_key: Vec<u8> = row
                .try_get("employee_public_key")
                .map_err(|_| "encrypted selection invalid")?;
            let binding: Uuid = row
                .try_get("office_binding_id")
                .map_err(|_| "encrypted selection invalid")?;
            let version: i64 = row
                .try_get("key_version")
                .map_err(|_| "encrypted selection invalid")?;
            let reference: String = row
                .try_get("decrypt_ref")
                .map_err(|_| "encrypted selection invalid")?;
            if config.bindings.iter().any(|b| {
                b.signer.company_id == scope.company_id()
                    && b.signer.employee_id.as_str() == employee_id
                    && b.signer.public_key.as_bytes().as_slice() == public_key
                    && b.office_binding_id == binding
                    && i64::try_from(b.key_version) == Ok(version)
                    && b.signer.signer_ref.as_str() == reference
            }) {
                allowed.push(
                    row.try_get("selection_id")
                        .map_err(|_| "encrypted selection invalid")?,
                );
            }
        }
        Ok(allowed)
    }

    pub(super) async fn stop_unselected(
        &self,
        scope: &CompanyScope,
        allowed: &[Uuid],
    ) -> Result<bool, &'static str> {
        let run: Option<Uuid> = sqlx::query_scalar("SELECT c.run_id FROM confidential_runs c
            JOIN runs r ON r.company_id=c.company_id AND r.id=c.run_id
            JOIN confidential_run_dispatches d ON d.company_id=c.company_id AND d.run_id=c.run_id
            LEFT JOIN confidential_execution_leases x ON x.company_id=c.company_id AND x.run_id=c.run_id
            WHERE c.company_id=$1 AND c.community_id=$2 AND NOT(c.selection_id=ANY($3))
              AND NOT EXISTS(SELECT 1 FROM runtime_cancellations stop WHERE stop.company_id=c.company_id AND stop.run_id=c.run_id)
              AND (r.status IN('queued','running','waiting') OR d.state='pending' OR x.state IN('observing','sealing')
                OR EXISTS(SELECT 1 FROM confidential_reply_outbox o WHERE o.company_id=c.company_id AND o.run_id=c.run_id AND o.state='pending'))
            ORDER BY c.run_id LIMIT 1")
            .bind(scope.company_id()).bind(scope.community_id()).bind(allowed)
            .fetch_optional(self.control.pool()).await.map_err(|_| "encrypted retired selection read failed")?;
        if let Some(run) = run {
            self.runs
                .cancel(scope, run)
                .await
                .map_err(|_| "encrypted retired selection stop unresolved")?;
            return Ok(true);
        }
        Ok(false)
    }

    pub(super) async fn retire_cancelled_copy(
        &self,
        scope: &CompanyScope,
    ) -> Result<(), &'static str> {
        let mut tx = self
            .control
            .pool()
            .begin()
            .await
            .map_err(|_| "encrypted receipt transaction failed")?;
        lock_office_authority_on(&mut tx, scope)
            .await
            .map_err(|_| "encrypted receipt fence failed")?;
        // Existing cancellation permanently denies fresh effects; this is only
        // retirement of an unowned/expired frozen copy, with no ACK invention.
        sqlx::query("WITH candidate AS (SELECT o.company_id,o.run_id,o.copy FROM confidential_reply_outbox o
            JOIN runtime_cancellations stop ON stop.company_id=o.company_id AND stop.run_id=o.run_id
            WHERE o.company_id=$1 AND o.community_id=$2 AND o.state='pending'
              AND (o.lease_expires_at IS NULL OR o.lease_expires_at+interval '5 seconds'<=clock_timestamp())
            ORDER BY o.next_attempt_at,o.run_id,o.copy FOR UPDATE OF o SKIP LOCKED LIMIT 1)
            UPDATE confidential_reply_outbox o SET state='retired',error_code='authority_changed',
              finished_at=clock_timestamp(),lease_token=NULL,lease_expires_at=NULL FROM candidate c
            WHERE o.company_id=c.company_id AND o.run_id=c.run_id AND o.copy=c.copy")
            .bind(scope.company_id()).bind(scope.community_id()).execute(&mut *tx).await
            .map_err(|_| "encrypted frozen copy retirement failed")?;
        tx.commit()
            .await
            .map_err(|_| "encrypted frozen copy retirement uncertain")
    }

    pub(super) async fn finalize_failed_job(
        &self,
        scope: &CompanyScope,
    ) -> Result<(), &'static str> {
        let mut tx = self
            .control
            .pool()
            .begin()
            .await
            .map_err(|_| "encrypted failure transaction failed")?;
        lock_office_authority_on(&mut tx, scope)
            .await
            .map_err(|_| "encrypted failure fence failed")?;
        sqlx::query("WITH candidate AS (SELECT i.company_id,i.event_id,j.error_code FROM encrypted_dm_decrypt_jobs j
            JOIN office_inbox i ON i.company_id=j.company_id AND i.event_id=j.source_id
              AND i.event_created_at=j.source_created_at AND i.author_pubkey=j.source_author AND i.received_at=j.source_received_at
            WHERE j.company_id=$1 AND j.community_id=$2 AND j.state IN('failed','cancelled') AND j.terminal_at IS NOT NULL
              AND NOT ortak_encrypted_dm_job_consumed(j.company_id,j.source_id)
              AND i.event_kind=1059 AND i.channel_id IS NULL AND i.state='pending'
              AND i.claim_generation=0 AND i.attempt_count=0 AND i.finalized_at IS NULL
              AND i.claimed_by IS NULL AND i.claim_expires_at IS NULL AND i.retry_after IS NULL
            ORDER BY j.source_received_at,j.source_id FOR UPDATE OF i SKIP LOCKED LIMIT 1)
            UPDATE office_inbox i SET state='failed',finalized_at=clock_timestamp(),
              last_error='encrypted_dm_'||c.error_code FROM candidate c
            WHERE i.company_id=c.company_id AND i.event_id=c.event_id")
            .bind(scope.company_id()).bind(scope.community_id()).execute(&mut *tx).await
            .map_err(|_| "encrypted terminal job settlement failed")?;
        tx.commit()
            .await
            .map_err(|_| "encrypted terminal job settlement uncertain")
    }
}
