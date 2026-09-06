pub(super) const IMPORT_HUMAN: &str = r#"
INSERT INTO runtime_cancellations (company_id, run_id, reason)
SELECT h.company_id, h.run_id, 'human_requested'
FROM run_cancel_requests h
JOIN runs r ON r.company_id=h.company_id AND r.id=h.run_id
WHERE h.company_id=$1 AND h.status='pending' AND r.payload_mode='ordinary'
  AND NOT EXISTS (SELECT 1 FROM runtime_cancellations c
                  WHERE c.company_id=h.company_id AND c.run_id=h.run_id)
ORDER BY h.requested_at, h.run_id LIMIT $2
ON CONFLICT (company_id,run_id) DO NOTHING
"#;

// A human may ask after an Office stop exhausted its budget. Terminal queue
// rows are immutable, so read them without a lock while locking only the human
// row; there is no human→queue/run lock inversion or implicit retry reset.
pub(super) const MIRROR_LATE_HUMAN: &str = r#"
WITH late AS (
    SELECT h.company_id,h.run_id FROM run_cancel_requests h
    JOIN runtime_cancellations c USING (company_id,run_id)
    WHERE h.company_id=$1 AND h.status='pending' AND c.state IN ('acknowledged','failed')
    ORDER BY h.requested_at,h.run_id LIMIT $2 FOR UPDATE OF h SKIP LOCKED
)
UPDATE run_cancel_requests h SET status=c.state,
    attempts=GREATEST(h.attempts,c.attempt_count),next_attempt_at=c.next_attempt_at,
    lease_token=NULL,lease_expires_at=NULL,last_error_code=c.last_error_code,
    acknowledged_at=c.acknowledged_at
FROM runtime_cancellations c,late
WHERE h.company_id=late.company_id AND h.run_id=late.run_id
  AND c.company_id=h.company_id AND c.run_id=h.run_id
"#;

pub(super) const EXHAUSTED: &str = r#"
WITH due AS (
    SELECT c.company_id,c.run_id FROM runtime_cancellations c
    JOIN runs r ON r.company_id=c.company_id AND r.id=c.run_id
    WHERE c.company_id=$1 AND r.runtime_adapter=$2 AND r.payload_mode='ordinary' AND c.state='pending'
      AND c.attempt_count=c.max_attempts
      AND (c.lease_expires_at IS NULL OR c.lease_expires_at<=clock_timestamp())
    ORDER BY c.next_attempt_at,c.run_id LIMIT $3 FOR UPDATE OF c SKIP LOCKED
)
UPDATE runtime_cancellations c SET state='failed',lease_token=NULL,
    lease_expires_at=NULL,last_error_code='cancellation_lease_exhausted'
FROM due WHERE c.company_id=due.company_id AND c.run_id=due.run_id
RETURNING c.run_id
"#;

pub(super) const CLAIM: &str = r#"
WITH due AS (
    SELECT c.company_id,c.run_id,r.runtime_adapter FROM runtime_cancellations c
    JOIN runs r ON r.company_id=c.company_id AND r.id=c.run_id
    WHERE c.company_id=$1 AND r.runtime_adapter=$2 AND r.payload_mode='ordinary' AND c.state='pending'
      AND c.attempt_count<c.max_attempts AND c.next_attempt_at<=clock_timestamp()
      AND (c.lease_expires_at IS NULL OR c.lease_expires_at<=clock_timestamp())
    ORDER BY c.next_attempt_at,c.requested_at,c.run_id
    LIMIT $3 FOR UPDATE OF c SKIP LOCKED
)
UPDATE runtime_cancellations c SET attempt_count=c.attempt_count+1,
    lease_token=gen_random_uuid(),lease_expires_at=clock_timestamp()+$4::bigint*interval '1 millisecond'
FROM due WHERE c.company_id=due.company_id AND c.run_id=due.run_id
RETURNING c.run_id,c.reason,c.attempt_count,c.max_attempts,c.lease_token,
          c.lease_expires_at,due.runtime_adapter
"#;

// The human request is attribution; the adapter queue owns retry authority.
// Call only after run→queue locks for acknowledgement, or queue-only claiming.
pub(super) const MIRROR_HUMAN: &str = r#"
UPDATE run_cancel_requests h SET status=c.state,
    attempts=GREATEST(h.attempts,c.attempt_count),next_attempt_at=c.next_attempt_at,
    lease_token=c.lease_token,lease_expires_at=c.lease_expires_at,
    last_error_code=c.last_error_code,acknowledged_at=c.acknowledged_at
FROM runtime_cancellations c
WHERE h.company_id=$1 AND h.run_id=ANY($2) AND h.status='pending'
  AND c.company_id=h.company_id AND c.run_id=h.run_id
"#;
