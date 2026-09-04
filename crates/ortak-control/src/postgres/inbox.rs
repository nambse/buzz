use std::time::Duration;

use chrono::{DateTime, Utc};
use sqlx::postgres::PgRow;
use sqlx::{PgConnection, Row};
use uuid::Uuid;

use super::{bytes32, interval_seconds, PgControlPlane};
use crate::error::{ControlError, Result};
use crate::ids::{ClaimGeneration, CompanyScope, MessageId};
use crate::inbox::{
    InboxClaim, InboxEvent, InboxInsertOutcome, InboxReleaseOutcome, InboxRow, InboxState,
};
use crate::ports::InboxRepository;

/// Inserts the inbox row on a caller-owned connection so the relay can commit
/// it in the same transaction as the signed `events` row.
///
/// The company is derived from the authenticated community's binding inside
/// the statement; an unbound community fails closed and writes nothing.
pub async fn insert_accepted_event_on(
    connection: &mut PgConnection,
    community_id: Uuid,
    event: &InboxEvent,
) -> Result<InboxInsertOutcome> {
    let row = sqlx::query(
        "WITH binding AS (
             SELECT company_id FROM office_company_bindings WHERE community_id = $1
         ),
         inserted AS (
             INSERT INTO office_inbox
                 (company_id, event_id, event_created_at, event_kind, author_pubkey, channel_id)
             SELECT company_id, $2, $3, $4, $5, $6 FROM binding
             ON CONFLICT (company_id, event_id) DO NOTHING
             RETURNING event_id
         )
         SELECT (SELECT count(*) FROM binding) AS bound,
                (SELECT count(*) FROM inserted) AS inserted",
    )
    .bind(community_id)
    .bind(event.event_id.as_bytes().as_slice())
    .bind(event.event_created_at)
    .bind(event.event_kind)
    .bind(event.author_pubkey.as_slice())
    .bind(event.channel_id)
    .fetch_one(&mut *connection)
    .await?;
    let bound: i64 = row.try_get("bound")?;
    let inserted: i64 = row.try_get("inserted")?;
    if bound == 0 {
        return Err(ControlError::UnknownCompanyBinding { community_id });
    }
    Ok(if inserted == 1 {
        InboxInsertOutcome::Inserted
    } else {
        InboxInsertOutcome::AlreadyPresent
    })
}

pub(crate) fn inbox_row(row: &PgRow) -> Result<InboxRow> {
    let event_id: Vec<u8> = row.try_get("event_id")?;
    let author_pubkey: Vec<u8> = row.try_get("author_pubkey")?;
    let state: String = row.try_get("state")?;
    Ok(InboxRow {
        event: InboxEvent {
            event_id: MessageId::try_from_slice(&event_id)?,
            event_created_at: row.try_get("event_created_at")?,
            event_kind: row.try_get("event_kind")?,
            author_pubkey: bytes32("author_pubkey", &author_pubkey)?,
            channel_id: row.try_get("channel_id")?,
        },
        state: InboxState::parse(&state).ok_or_else(|| {
            ControlError::InvalidData(format!("office_inbox.state holds {state:?}"))
        })?,
        claim_generation: ClaimGeneration(row.try_get("claim_generation")?),
        claimed_by: row.try_get("claimed_by")?,
        claim_expires_at: row.try_get("claim_expires_at")?,
        attempt_count: row.try_get("attempt_count")?,
        retry_after: row.try_get("retry_after")?,
        last_error: row.try_get("last_error")?,
        received_at: row.try_get("received_at")?,
        finalized_at: row.try_get("finalized_at")?,
    })
}

fn claim_from_row(row: &PgRow) -> Result<InboxClaim> {
    let company_id: Uuid = row.try_get("company_id")?;
    let inbox = inbox_row(row)?;
    let (Some(claimed_by), Some(claim_expires_at)) = (inbox.claimed_by, inbox.claim_expires_at)
    else {
        return Err(ControlError::InvalidData(
            "claimed inbox row lacks claim metadata".to_owned(),
        ));
    };
    Ok(InboxClaim {
        company_id,
        message_id: inbox.event.event_id,
        claim_generation: inbox.claim_generation,
        claimed_by,
        claim_expires_at,
        attempt_count: inbox.attempt_count,
        event: inbox.event,
    })
}

impl PgControlPlane {
    async fn claim(
        &self,
        scope: &CompanyScope,
        message_id: Option<MessageId>,
        worker_id: &str,
        lease: Duration,
        max_attempts: i32,
    ) -> Result<Option<InboxClaim>> {
        let mut tx = self.pool.begin().await?;

        // Expired claims that already used every attempt become terminal so
        // they stay visible to operators instead of cycling forever.
        sqlx::query(
            "UPDATE office_inbox
                SET state = 'failed',
                    finalized_at = now(),
                    last_error = coalesce(last_error, 'claim attempts exhausted')
              WHERE company_id = $1
                AND state = 'claimed'
                AND claim_expires_at <= now()
                AND attempt_count >= $2",
        )
        .bind(scope.company_id())
        .bind(max_attempts)
        .execute(&mut *tx)
        .await?;

        let row = sqlx::query(
            "WITH candidate AS (
                 SELECT company_id, event_id
                   FROM office_inbox
                  WHERE company_id = $1
                    AND ($5::bytea IS NULL OR event_id = $5)
                    AND attempt_count < $4
                    AND (
                        (state = 'pending' AND (retry_after IS NULL OR retry_after <= now()))
                        OR (state = 'claimed' AND claim_expires_at <= now())
                    )
                  ORDER BY received_at, event_id
                  FOR UPDATE SKIP LOCKED
                  LIMIT 1
             )
             UPDATE office_inbox i
                SET state = 'claimed',
                    claim_generation = i.claim_generation + 1,
                    claimed_by = $2,
                    claim_expires_at = now() + make_interval(secs => $3),
                    attempt_count = i.attempt_count + 1,
                    retry_after = NULL
               FROM candidate c
              WHERE i.company_id = c.company_id AND i.event_id = c.event_id
              RETURNING i.company_id, i.event_id, i.event_created_at, i.event_kind, i.author_pubkey, i.channel_id, i.state, i.claim_generation, i.claimed_by, i.claim_expires_at, i.attempt_count, i.retry_after, i.last_error, i.received_at, i.finalized_at",
        )
        .bind(scope.company_id())
        .bind(worker_id)
        .bind(interval_seconds(lease))
        .bind(max_attempts)
        .bind(message_id.map(|id| id.as_bytes().to_vec()))
        .fetch_optional(&mut *tx)
        .await?;

        let claim = row.as_ref().map(claim_from_row).transpose()?;
        tx.commit().await?;
        Ok(claim)
    }
}

impl InboxRepository for PgControlPlane {
    async fn insert_accepted_event(
        &self,
        community_id: Uuid,
        event: &InboxEvent,
    ) -> Result<InboxInsertOutcome> {
        let mut connection = self.pool.acquire().await?;
        insert_accepted_event_on(&mut connection, community_id, event).await
    }

    async fn claim_next(
        &self,
        scope: &CompanyScope,
        worker_id: &str,
        lease: Duration,
        max_attempts: i32,
    ) -> Result<Option<InboxClaim>> {
        self.claim(scope, None, worker_id, lease, max_attempts)
            .await
    }

    async fn claim_message(
        &self,
        scope: &CompanyScope,
        message_id: MessageId,
        worker_id: &str,
        lease: Duration,
        max_attempts: i32,
    ) -> Result<Option<InboxClaim>> {
        self.claim(scope, Some(message_id), worker_id, lease, max_attempts)
            .await
    }

    async fn release_for_retry(
        &self,
        scope: &CompanyScope,
        message_id: MessageId,
        claim_generation: ClaimGeneration,
        error: &str,
        retry_after: DateTime<Utc>,
        max_attempts: i32,
    ) -> Result<InboxReleaseOutcome> {
        let row = sqlx::query(
            "UPDATE office_inbox
                SET state = CASE WHEN attempt_count >= $6 THEN 'failed' ELSE 'pending' END,
                    finalized_at = CASE WHEN attempt_count >= $6 THEN now() ELSE NULL END,
                    retry_after = CASE WHEN attempt_count >= $6 THEN NULL ELSE $5 END,
                    last_error = $4
              WHERE company_id = $1
                AND event_id = $2
                AND state = 'claimed'
                AND claim_generation = $3
              RETURNING state",
        )
        .bind(scope.company_id())
        .bind(message_id.as_bytes().as_slice())
        .bind(claim_generation.0)
        .bind(error)
        .bind(retry_after)
        .bind(max_attempts)
        .fetch_optional(&self.pool)
        .await?;
        match row {
            None => Ok(InboxReleaseOutcome::Stale),
            Some(row) => {
                let state: String = row.try_get("state")?;
                Ok(if state == "failed" {
                    InboxReleaseOutcome::Failed
                } else {
                    InboxReleaseOutcome::Retrying
                })
            }
        }
    }

    async fn finalize_dropped(
        &self,
        scope: &CompanyScope,
        message_id: MessageId,
        claim_generation: ClaimGeneration,
        reason: &str,
    ) -> Result<bool> {
        let result = sqlx::query(
            "UPDATE office_inbox
                SET state = 'dropped', finalized_at = now(), last_error = $4
              WHERE company_id = $1
                AND event_id = $2
                AND state = 'claimed'
                AND claim_generation = $3",
        )
        .bind(scope.company_id())
        .bind(message_id.as_bytes().as_slice())
        .bind(claim_generation.0)
        .bind(reason)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() == 1)
    }

    async fn inbox_row(
        &self,
        scope: &CompanyScope,
        message_id: MessageId,
    ) -> Result<Option<InboxRow>> {
        let row = sqlx::query(
            "SELECT event_id, event_created_at, event_kind, author_pubkey, channel_id, state, claim_generation, claimed_by, claim_expires_at, attempt_count, retry_after, last_error, received_at, finalized_at
               FROM office_inbox WHERE company_id = $1 AND event_id = $2",
        )
        .bind(scope.company_id())
        .bind(message_id.as_bytes().as_slice())
        .fetch_optional(&self.pool)
        .await?;
        row.as_ref().map(inbox_row).transpose()
    }
}
