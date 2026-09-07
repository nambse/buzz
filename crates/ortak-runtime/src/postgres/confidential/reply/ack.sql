-- Receipt-only settlement after a known exact relay acknowledgement. No current
-- authority or expiry requirement; the unchanged locked claim is mandatory.
UPDATE confidential_reply_outbox
SET state='acked',acknowledged_at=clock_timestamp(),finished_at=clock_timestamp(),
    lease_token=NULL,lease_expires_at=NULL,error_code=NULL
WHERE company_id=$1 AND community_id=$2 AND run_id=$3 AND copy=$4
    AND generation=$5 AND lease_token=$6 AND state='pending'
