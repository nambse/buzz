pub(super) const CLAIM: &str = r#"
WITH due AS (
    SELECT company_id,run_id FROM runtime_office_outputs
    WHERE company_id=$1 AND state='pending' AND attempt_count<20
      AND NOT (run_id=ANY($2::uuid[]))
      AND next_attempt_at<=clock_timestamp()
      AND (lease_expires_at IS NULL OR lease_expires_at<=clock_timestamp())
    ORDER BY next_attempt_at,created_at,run_id LIMIT 1 FOR UPDATE SKIP LOCKED
)
UPDATE runtime_office_outputs j SET attempt_count=j.attempt_count+1,
    lease_token=gen_random_uuid(),lease_expires_at=clock_timestamp()+interval '60 seconds'
FROM due WHERE j.company_id=due.company_id AND j.run_id=due.run_id
RETURNING j.run_id,j.lease_token
"#;

pub(super) const EXHAUSTED: &str = r#"
WITH due AS (
    SELECT company_id,run_id FROM runtime_office_outputs
    WHERE company_id=$1 AND state='pending' AND attempt_count=20
      AND (lease_expires_at IS NULL OR lease_expires_at<=clock_timestamp())
    ORDER BY next_attempt_at,run_id LIMIT $2 FOR UPDATE SKIP LOCKED
)
UPDATE runtime_office_outputs j SET state='failed',lease_token=NULL,
    lease_expires_at=NULL,last_error_code='office_output_lease_exhausted'
FROM due WHERE j.company_id=due.company_id AND j.run_id=due.run_id
"#;

pub(super) const SOURCE: &str = r#"
SELECT r.status,r.delivery_intent,r.employee_id,r.employee_revision_id,r.routing_decision_id,r.message_id,r.root_message_id,
    c.status AS company_status,i.state AS inbox_state,i.event_kind,i.channel_id,
    i.event_created_at,i.author_pubkey,d.office_input_hash,
    (r.message_id=d.message_id AND r.root_message_id=d.root_message_id
     AND rr.employee_revision_id=r.employee_revision_id AND rr.action='wake') AS pinned,
    (rev.manifest #>> '{office,public_key}' IS NOT NULL
     AND lower(rev.manifest #>> '{office,public_key}')=lower(active.manifest #>> '{office,public_key}')
     AND rev.manifest #>> '{office,signer_ref}'=active.manifest #>> '{office,signer_ref}') AS same_identity,
    (EXISTS (SELECT 1 FROM runtime_cancellations x WHERE x.company_id=r.company_id AND x.run_id=r.id)
     OR EXISTS (SELECT 1 FROM run_cancel_requests x WHERE x.company_id=r.company_id AND x.run_id=r.id)) AS cancelled
FROM runs r JOIN companies c ON c.id=r.company_id
LEFT JOIN routing_decisions d ON d.company_id=r.company_id AND d.id=r.routing_decision_id
LEFT JOIN routing_recipients rr ON rr.company_id=r.company_id
    AND rr.routing_decision_id=r.routing_decision_id AND rr.employee_id=r.employee_id
LEFT JOIN office_inbox i ON i.company_id=r.company_id AND i.event_id=d.message_id
LEFT JOIN employee_revisions rev ON rev.company_id=r.company_id
    AND rev.employee_id=r.employee_id AND rev.id=r.employee_revision_id
LEFT JOIN employees e ON e.company_id=r.company_id AND e.id=r.employee_id
LEFT JOIN employee_revisions active ON active.company_id=e.company_id
    AND active.employee_id=e.id AND active.id=e.active_revision_id
WHERE r.company_id=$1 AND r.id=$2 FOR UPDATE OF r
"#;
