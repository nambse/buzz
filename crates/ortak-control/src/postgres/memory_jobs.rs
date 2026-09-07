//! Bounded memory job persistence. No remote service is called here.

use std::time::Duration;

use chrono::{DateTime, Utc};
use ortak_domain::{EmployeeId, MemoryBinding};
use sqlx::{PgConnection, Postgres, Row, Transaction};
use uuid::Uuid;

use super::{interval_seconds, office_authority_matches_on, PgControlPlane};
use crate::memory::{
    MemoryFact, MemoryProvenance, MemoryScope, MemoryWriteReceipt, MemoryWriteRequest,
};
use crate::memory_jobs::{MemoryWriteJobLease, MemoryWriteJobOutcome, MemoryWriteJobRepository};
use crate::office_authority::OfficeAuthority;
use crate::{CompanyScope, ControlError, Result};

mod sql;

async fn begin(control: &PgControlPlane) -> Result<Transaction<'_, Postgres>> {
    let mut tx = control.pool.begin().await?;
    bounds(&mut tx).await?;
    Ok(tx)
}

async fn bounds(connection: &mut PgConnection) -> Result<()> {
    sqlx::query("SELECT set_config('lock_timeout','500ms',true), set_config('statement_timeout','2s',true), set_config('idle_in_transaction_session_timeout','5s',true)")
        .execute(connection).await?;
    Ok(())
}

/// Builds an immutable request after the caller has revalidated the original
/// canonical Office draft/source, under this same shared authority fence.
///
/// The caller acquires the fence before run→output→outbox/job locks, uses READ
/// COMMITTED, and commits before bounded adapter I/O. A lease is not authority:
/// this helper additionally checks current lifecycle, pinned/current memory
/// binding, delivered output and cancellation. `None` is only a stale lease;
/// authority refusals propagate and must be durably failed by the worker.
/// The deferred trigger catches expiry even while commit waits on row locks.
pub async fn prepare_memory_write_on(
    connection: &mut PgConnection,
    scope: &CompanyScope,
    lease: &MemoryWriteJobLease,
    witness: &OfficeAuthority,
) -> Result<Option<MemoryWriteRequest>> {
    bounds(connection).await?;
    if !office_authority_matches_on(connection, scope, witness).await? {
        return Err(refused());
    }
    // Root first: cancellation uses run→queue; claims/receipts only lock jobs.
    sqlx::query("SELECT id FROM runs WHERE company_id=$1 AND id=$2 FOR UPDATE")
        .bind(scope.company_id())
        .bind(lease.run_id)
        .fetch_optional(&mut *connection)
        .await?;
    let row = sqlx::query(sql::PREPARE)
        .bind(scope.company_id())
        .bind(lease.run_id)
        .bind(lease.lease_token)
        .fetch_optional(&mut *connection)
        .await?;
    let Some(row) = row else { return Ok(None) };
    if row.try_get::<Option<bool>, _>("authorized")? != Some(true) {
        return Err(refused());
    }
    let request = request_from_row(&row)?;
    let lease_deadline: DateTime<Utc> = row.try_get("lease_expires_at")?;
    let admission_deadline = witness
        .valid_before()
        .map_or(lease_deadline, |office| office.min(lease_deadline));
    let updated = sqlx::query("UPDATE runtime_memory_writes SET admission_generation=$4,admission_valid_before=$5,admission_token=$6 WHERE company_id=$1 AND run_id=$2 AND state='pending' AND lease_token=$3 AND lease_expires_at>clock_timestamp()")
        .bind(scope.company_id()).bind(lease.run_id).bind(lease.lease_token)
        .bind(witness.generation()).bind(admission_deadline).bind(Uuid::new_v4())
        .execute(&mut *connection).await?.rows_affected();
    Ok((updated == 1).then_some(request))
}

fn refused() -> ControlError {
    ControlError::InvalidData("memory write authority changed or is unavailable".to_owned())
}

fn request_from_row(row: &sqlx::postgres::PgRow) -> Result<MemoryWriteRequest> {
    let employee = EmployeeId::parse(row.try_get::<&str, _>("employee_id")?)?;
    let run_id = row.try_get("run_id")?;
    let binding: MemoryBinding = serde_json::from_value(row.try_get("binding")?)?;
    let recorded_at: DateTime<Utc> = row.try_get("recorded_at")?;
    let event_id: Vec<u8> = row.try_get("signed_event_id")?;
    let source = format!("office:{}", hex::encode(event_id));
    let content: String = row.try_get("content")?;
    let request = MemoryWriteRequest {
        employee_id: employee.clone(),
        binding,
        scope: MemoryScope::RunScratch { run_id },
        facts: chunks(&content)?
            .into_iter()
            .map(|content| MemoryFact {
                content: content.to_owned(),
                provenance: MemoryProvenance {
                    employee_id: employee.clone(),
                    run_id: Some(run_id),
                    source: source.clone(),
                    recorded_at,
                },
            })
            .collect(),
        idempotency_key: row.try_get("idempotency_key")?,
    };
    request.validate().map_err(|_| {
        ControlError::InvalidData("frozen memory output violates bounds".to_owned())
    })?;
    Ok(request)
}

// Preserve every byte. Prefer two facts; the rare UTF-8 boundary that needs
// three facts uses a linear scan of at most 16KiB of character boundaries.
// Each fact must contain nonblank text, including across long whitespace spans.
fn chunks(text: &str) -> Result<Vec<&str>> {
    const MAX: usize = 16384;
    let invalid = || {
        ControlError::InvalidData(
            "Office output cannot form bounded nonblank memory facts".to_owned(),
        )
    };
    if text.is_empty() || text.len() > 32768 {
        return Err(invalid());
    }
    let first_end = text
        .char_indices()
        .find(|(_, c)| !c.is_whitespace())
        .map(|(start, c)| start + c.len_utf8())
        .ok_or_else(invalid)?;
    if text.len() <= MAX {
        return Ok(vec![text]);
    }
    let last_start = text
        .char_indices()
        .rfind(|(_, c)| !c.is_whitespace())
        .map(|(start, _)| start)
        .ok_or_else(invalid)?;
    if let Some(cut) = pair_cut(text, 0, first_end, last_start) {
        return Ok(vec![&text[..cut], &text[cut..]]);
    }
    let mut nonblank = text
        .char_indices()
        .filter(|(_, c)| !c.is_whitespace())
        .peekable();
    for (start, c) in text.char_indices() {
        let cut = start + c.len_utf8();
        if cut > MAX {
            break;
        }
        if cut < first_end {
            continue;
        }
        while nonblank.peek().is_some_and(|(start, _)| *start < cut) {
            nonblank.next();
        }
        let Some(&(next_start, next)) = nonblank.peek() else {
            break;
        };
        if let Some(last_cut) = pair_cut(text, cut, next_start + next.len_utf8(), last_start) {
            return Ok(vec![&text[..cut], &text[cut..last_cut], &text[last_cut..]]);
        }
    }
    Err(invalid())
}

// A legal cut leaves nonblank text on both sides and both byte lengths <=16KiB.
// first_end and last_start are precomputed in one pass; this helper scans at
// most three continuation bytes, so the three-fact search stays linear.
fn pair_cut(text: &str, start: usize, first_end: usize, last_start: usize) -> Option<usize> {
    let lower = text.len().saturating_sub(16384).max(first_end);
    let mut upper = (start + 16384).min(last_start);
    while upper > lower && !text.is_char_boundary(upper) {
        upper -= 1;
    }
    (lower <= upper && text.is_char_boundary(upper)).then_some(upper)
}

impl MemoryWriteJobRepository for PgControlPlane {
    async fn claim_memory_write(
        &self,
        scope: &CompanyScope,
        adapter: &str,
        lease: Duration,
    ) -> Result<Option<MemoryWriteJobLease>> {
        if adapter.is_empty()
            || adapter.len() > 64
            || lease < Duration::from_secs(1)
            || lease > Duration::from_secs(300)
        {
            return Err(ControlError::InvalidData(
                "invalid memory job lease configuration".to_owned(),
            ));
        }
        let mut tx = begin(self).await?;
        sqlx::query(sql::EXHAUSTED)
            .bind(scope.company_id())
            .bind(adapter)
            .execute(&mut *tx)
            .await?;
        let row = sqlx::query(sql::CLAIM)
            .bind(scope.company_id())
            .bind(adapter)
            .bind(interval_seconds(lease))
            .fetch_optional(&mut *tx)
            .await?;
        let result = row
            .map(|row| -> Result<_> {
                Ok(MemoryWriteJobLease {
                    run_id: row.try_get("run_id")?,
                    lease_token: row.try_get("lease_token")?,
                    attempt_count: row.try_get("attempt_count")?,
                })
            })
            .transpose()?;
        tx.commit().await?;
        Ok(result)
    }

    async fn acknowledge_memory_write(
        &self,
        scope: &CompanyScope,
        lease: &MemoryWriteJobLease,
        receipt: &MemoryWriteReceipt,
    ) -> Result<bool> {
        if receipt.receipt_ref.trim().is_empty()
            || receipt.receipt_ref.len() > 1024
            || receipt.written == 0
            || receipt.written > 3
        {
            return Err(ControlError::InvalidData(
                "invalid memory write receipt".to_owned(),
            ));
        }
        let mut tx = begin(self).await?;
        let content: Option<String> = sqlx::query_scalar("SELECT content FROM runtime_memory_writes WHERE company_id=$1 AND run_id=$2 AND state='pending' AND lease_token=$3 AND lease_expires_at>clock_timestamp() AND admission_token IS NOT NULL FOR UPDATE")
            .bind(scope.company_id()).bind(lease.run_id).bind(lease.lease_token).fetch_optional(&mut *tx).await?;
        let Some(content) = content else {
            return Ok(false);
        };
        if chunks(&content)?.len() != receipt.written {
            return Err(ControlError::InvalidData(
                "memory receipt fact count does not match frozen request".to_owned(),
            ));
        }
        // Receipt records an already completed remote operation. Revocation or
        // deadline expiry after its admission must not erase that observation.
        let changed = sqlx::query("UPDATE runtime_memory_writes SET state='acknowledged',receipt=$4,acknowledged_at=clock_timestamp(),lease_token=NULL,lease_expires_at=NULL,last_error_code=NULL WHERE company_id=$1 AND run_id=$2 AND state='pending' AND lease_token=$3 AND lease_expires_at>clock_timestamp()")
            .bind(scope.company_id()).bind(lease.run_id).bind(lease.lease_token).bind(serde_json::to_value(receipt)?)
            .execute(&mut *tx).await?.rows_affected();
        tx.commit().await?;
        Ok(changed == 1)
    }

    async fn fail_memory_write(
        &self,
        scope: &CompanyScope,
        lease: &MemoryWriteJobLease,
        code: &str,
        permanent: bool,
    ) -> Result<MemoryWriteJobOutcome> {
        if code.is_empty()
            || code.len() > 64
            || !code.as_bytes()[0].is_ascii_lowercase()
            || !code
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'.')
        {
            return Err(ControlError::InvalidData(
                "invalid memory job error code".to_owned(),
            ));
        }
        let mut tx = begin(self).await?;
        let state: Option<String> = sqlx::query_scalar("UPDATE runtime_memory_writes SET state=CASE WHEN $5 OR attempt_count>=20 THEN 'failed' ELSE 'pending' END,next_attempt_at=clock_timestamp()+make_interval(secs=>least(300,power(2,least(attempt_count-1,9)))::double precision),lease_token=NULL,lease_expires_at=NULL,last_error_code=$4 WHERE company_id=$1 AND run_id=$2 AND state='pending' AND lease_token=$3 AND lease_expires_at>clock_timestamp() RETURNING state")
            .bind(scope.company_id()).bind(lease.run_id).bind(lease.lease_token).bind(code).bind(permanent)
            .fetch_optional(&mut *tx).await?;
        tx.commit().await?;
        Ok(match state.as_deref() {
            Some("failed") => MemoryWriteJobOutcome::Failed,
            Some("pending") => MemoryWriteJobOutcome::Retrying,
            _ => MemoryWriteJobOutcome::Stale,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::chunks;
    fn assert_partition(value: &str) {
        let parts = chunks(value).expect("known nonblank partition exists");
        assert_eq!(parts.concat(), value);
        assert!((1..=3).contains(&parts.len()));
        assert!(parts
            .iter()
            .all(|part| part.len() <= 16384 && !part.trim().is_empty()));
    }

    #[test]
    fn unicode_whitespace_can_require_moving_both_fact_boundaries() {
        let value = "ab".to_owned() + &" ".repeat(16380) + "\u{3000}" + &" ".repeat(16381) + "c";
        assert_eq!(value.len(), 32767);
        let parts = chunks(&value).expect("three nonblank facts fit");
        assert_eq!(
            parts.iter().map(|part| part.len()).collect::<Vec<_>>(),
            [1, 16384, 16382]
        );
        assert_partition(&value);
    }

    #[test]
    fn known_feasible_unicode_partitions_preserve_bytes_at_the_size_boundaries() {
        // Construct an independent witness: three already-valid facts. Their
        // concatenation must remain representable regardless of greedy cuts.
        fn fact(width: usize, marker: &str, space: &str, marker_first: bool) -> String {
            let padding = width - marker.len();
            let blank = space.repeat(padding / space.len()) + &" ".repeat(padding % space.len());
            if marker_first {
                marker.to_owned() + &blank
            } else {
                blank + marker
            }
        }
        for total in [16385, 32765, 32766, 32767, 32768] {
            for first in [4, 8192, 16381, 16384] {
                if total - first < 8 {
                    continue;
                }
                let second = (total - first - 4).min(16384);
                let third = total - first - second;
                for marker in ["a", "界", "🦀"] {
                    for space in [" ", "\u{a0}", "\u{3000}"] {
                        for marker_first in [false, true] {
                            let value = fact(first, marker, space, marker_first)
                                + &fact(second, marker, space, !marker_first)
                                + &fact(third, marker, space, marker_first);
                            assert_eq!(value.len(), total);
                            assert_partition(&value);
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn unrepresentable_or_invalid_output_is_refused_without_trimming() {
        for value in [
            String::new(),
            " ".repeat(16385),
            " ".repeat(16384) + "a",
            "a".to_owned() + &" ".repeat(16384),
            "a".repeat(32769),
        ] {
            assert!(chunks(&value).is_err());
        }
    }

    #[test]
    fn utf8_output_chunks_reconstruct_the_exact_published_bytes() {
        let value = "a".repeat(16383) + &"€".repeat(5461) + "aa";
        let parts = chunks(&value).expect("splittable output");
        assert_eq!(parts.concat(), value);
        assert!(parts.iter().all(|p| p.len() <= 16384));
        assert_eq!(parts.len(), 3);
    }
}
