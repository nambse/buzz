//! Bounded stored-event replay. Cursor and inbox writes share one transaction.

use chrono::{DateTime, Utc};
use sqlx::{postgres::PgRow, PgConnection, Row};
use uuid::Uuid;

use super::{cohort, PgControlPlane};
use crate::cohort::{InboxReconciliationProgress, MAX_INBOX_RECONCILIATION_BATCH};
use crate::{CompanyScope, Result};

fn progress(row: &PgRow) -> Result<InboxReconciliationProgress> {
    Ok(InboxReconciliationProgress {
        capture_id: row.try_get("capture_id")?,
        channel_id: row.try_get("channel_id")?,
        scanned: row.try_get("scanned")?,
        inserted: row.try_get("inserted")?,
        completed: row
            .try_get::<Option<DateTime<Utc>>, _>("completed_at")?
            .is_some(),
    })
}

async fn selected(
    connection: &mut PgConnection,
    scope: &CompanyScope,
    capture_id: Uuid,
    channel_id: Uuid,
) -> Result<Uuid> {
    sqlx::query_scalar(
        "SELECT c.community_id FROM office_routing_cohorts c
         JOIN office_routing_channels s ON s.company_id=c.company_id AND s.community_id=c.community_id
         WHERE c.company_id=$1 AND c.capture_id=$2 AND s.channel_id=$3
           AND c.state IN ('capture','enabled') AND ($4::uuid IS NULL OR c.community_id=$4)",
    ).bind(scope.company_id()).bind(capture_id).bind(channel_id).bind(scope.community_id())
        .fetch_optional(connection).await?.ok_or_else(cohort::refused)
}

impl PgControlPlane {
    /// Pins a finite canonical stored-event window, including accepted events
    /// with future signed timestamps. Repeating this returns the original job.
    ///
    /// The new relay capture hook must already be active before capture begins:
    /// later accepted/backdated events enter the inbox atomically at ingress,
    /// while this scan repairs all rows visible at its pinned upper key.
    pub async fn start_inbox_reconciliation(
        &self,
        scope: &CompanyScope,
        capture_id: Uuid,
        channel_id: Uuid,
    ) -> Result<InboxReconciliationProgress> {
        let mut tx = self.pool.begin().await?;
        cohort::lock(&mut tx, scope).await?;
        let community = selected(&mut tx, scope, capture_id, channel_id).await?;
        // Index-backed maximum over stored rows, never wall-clock/received_at:
        // signed timestamps can be in the future and transaction start time
        // does not establish the order in which events committed.
        sqlx::query(
            "WITH upper_bound AS (
                 SELECT created_at,id FROM events WHERE community_id=$3 AND channel_id=$4
                 AND kind IN (9,40002) AND deleted_at IS NULL ORDER BY created_at DESC,id DESC LIMIT 1
             )
             INSERT INTO office_inbox_reconciliations
                (company_id,capture_id,community_id,channel_id,upper_created_at,upper_event_id,completed_at)
             SELECT $1,$2,$3,$4,u.created_at,u.id,
                    CASE WHEN u.id IS NULL THEN clock_timestamp() END
             FROM (VALUES (1)) AS anchor(value) LEFT JOIN upper_bound u ON true
             ON CONFLICT(company_id,capture_id,channel_id) DO NOTHING",
        ).bind(scope.company_id()).bind(capture_id).bind(community).bind(channel_id)
            .execute(&mut *tx).await?;
        let row = sqlx::query(
            "SELECT capture_id,channel_id,scanned,inserted,completed_at
                               FROM office_inbox_reconciliations
                               WHERE company_id=$1 AND capture_id=$2 AND channel_id=$3",
        )
        .bind(scope.company_id())
        .bind(capture_id)
        .bind(channel_id)
        .fetch_one(&mut *tx)
        .await?;
        let result = progress(&row)?;
        tx.commit().await?;
        Ok(result)
    }

    /// Reconciles at most 256 canonical event rows and durably advances the same
    /// keyset cursor. Concurrent batches serialize on this job; a failed or lost
    /// acknowledgement can retry without duplicate inbox rows or dispatch work.
    /// No event content, provider operation or model request enters this transaction.
    pub async fn reconcile_inbox_batch(
        &self,
        scope: &CompanyScope,
        capture_id: Uuid,
        channel_id: Uuid,
        limit: u16,
    ) -> Result<InboxReconciliationProgress> {
        if !(1..=MAX_INBOX_RECONCILIATION_BATCH).contains(&limit) {
            return Err(cohort::refused());
        }
        let mut tx = self.pool.begin().await?;
        cohort::lock(&mut tx, scope).await?;
        let community = selected(&mut tx, scope, capture_id, channel_id).await?;
        let job = sqlx::query("SELECT capture_id,channel_id,scanned,inserted,completed_at,
                                     upper_created_at,upper_event_id,cursor_created_at,cursor_event_id
                              FROM office_inbox_reconciliations
                              WHERE company_id=$1 AND capture_id=$2 AND channel_id=$3 FOR UPDATE")
            .bind(scope.company_id()).bind(capture_id).bind(channel_id)
            .fetch_optional(&mut *tx).await?.ok_or_else(cohort::refused)?;
        let previous = progress(&job)?;
        if previous.completed {
            tx.commit().await?;
            return Ok(previous);
        }
        let upper_at: DateTime<Utc> = job.try_get("upper_created_at")?;
        let upper_id: Vec<u8> = job.try_get("upper_event_id")?;
        let cursor_at: Option<DateTime<Utc>> = job.try_get("cursor_created_at")?;
        let cursor_id: Option<Vec<u8>> = job.try_get("cursor_event_id")?;
        let rows = sqlx::query(
            "SELECT id,created_at,pubkey,kind FROM events
             WHERE community_id=$1 AND channel_id=$2 AND kind IN (9,40002) AND deleted_at IS NULL
               AND (created_at,id)<=($3,$4)
               AND ($5::timestamptz IS NULL OR (created_at,id)>($5,$6))
             ORDER BY created_at,id LIMIT $7",
        )
        .bind(community)
        .bind(channel_id)
        .bind(upper_at)
        .bind(&upper_id)
        .bind(cursor_at)
        .bind(cursor_id.as_deref())
        .bind(i64::from(limit))
        .fetch_all(&mut *tx)
        .await?;
        let mut ids = Vec::<Vec<u8>>::with_capacity(rows.len());
        let mut dates = Vec::<DateTime<Utc>>::with_capacity(rows.len());
        let mut authors = Vec::<Vec<u8>>::with_capacity(rows.len());
        let mut kinds = Vec::<i32>::with_capacity(rows.len());
        for row in &rows {
            ids.push(row.try_get("id")?);
            dates.push(row.try_get("created_at")?);
            authors.push(row.try_get("pubkey")?);
            kinds.push(row.try_get("kind")?);
        }
        let inserted = sqlx::query(
            "INSERT INTO office_inbox(company_id,event_id,event_created_at,event_kind,author_pubkey,channel_id)
             SELECT $1,event_id,created_at,kind,pubkey,$6
             FROM UNNEST($2::bytea[],$3::timestamptz[],$4::int[],$5::bytea[])
               AS page(event_id,created_at,kind,pubkey)
             ON CONFLICT(company_id,event_id) DO NOTHING",
        ).bind(scope.company_id()).bind(&ids).bind(&dates).bind(&kinds).bind(&authors).bind(channel_id)
            .execute(&mut *tx).await?.rows_affected();
        let last_at = dates.last().copied().or(cursor_at);
        let last_id = ids.last().cloned().or(cursor_id);
        let completed = rows.len() < usize::from(limit)
            || (last_at == Some(upper_at) && last_id.as_ref() == Some(&upper_id));
        let row = sqlx::query(
            "UPDATE office_inbox_reconciliations SET cursor_created_at=$4,cursor_event_id=$5,
                scanned=scanned+$6,inserted=inserted+$7,
                completed_at=CASE WHEN $8 THEN clock_timestamp() END
             WHERE company_id=$1 AND capture_id=$2 AND channel_id=$3
             RETURNING capture_id,channel_id,scanned,inserted,completed_at",
        )
        .bind(scope.company_id())
        .bind(capture_id)
        .bind(channel_id)
        .bind(last_at)
        .bind(last_id)
        .bind(rows.len() as i64)
        .bind(inserted as i64)
        .bind(completed)
        .fetch_one(&mut *tx)
        .await?;
        let result = progress(&row)?;
        tx.commit().await?;
        Ok(result)
    }
}
