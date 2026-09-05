pub(super) const CLAIM: &str = "WITH due AS (
 SELECT company_id,run_id FROM runtime_memory_writes WHERE company_id=$1
 AND binding->>'adapter'=$2 AND state='pending' AND attempt_count<20
 AND next_attempt_at<=clock_timestamp() AND (lease_expires_at IS NULL OR lease_expires_at<=clock_timestamp())
 ORDER BY next_attempt_at,created_at,run_id LIMIT 1 FOR UPDATE SKIP LOCKED)
 UPDATE runtime_memory_writes j SET attempt_count=j.attempt_count+1,lease_token=gen_random_uuid(),
 lease_expires_at=clock_timestamp()+make_interval(secs=>$3),
 admission_generation=NULL,admission_valid_before=NULL,admission_token=NULL
 FROM due WHERE j.company_id=due.company_id AND j.run_id=due.run_id
 RETURNING j.run_id,j.lease_token,j.attempt_count";

pub(super) const EXHAUSTED: &str = "WITH due AS (
 SELECT company_id,run_id FROM runtime_memory_writes WHERE company_id=$1 AND binding->>'adapter'=$2
 AND state='pending' AND attempt_count=20 AND (lease_expires_at IS NULL OR lease_expires_at<=clock_timestamp())
 ORDER BY next_attempt_at,run_id LIMIT 64 FOR UPDATE SKIP LOCKED)
 UPDATE runtime_memory_writes j SET state='failed',lease_token=NULL,lease_expires_at=NULL,last_error_code='memory_lease_exhausted'
 FROM due WHERE j.company_id=due.company_id AND j.run_id=due.run_id";

pub(super) const PREPARE: &str = r#"
SELECT j.*, (
 c.status='active' AND e.status='active' AND r.status='completed' AND r.delivery_intent IN ('reply','channel')
 AND j.employee_id=r.employee_id AND j.employee_revision_id=r.employee_revision_id
 AND o.state='delivered' AND o.kind='office_publish' AND o.run_id=r.id AND o.signed_event_id=j.signed_event_id
 AND o.signed_event_bytes IS NOT NULL AND output.state='enqueued' AND output.outbox_id=j.outbox_id
 AND output.source_facts=j.source_facts AND output.draft_content=j.content
 AND i.state='decided' AND i.channel_id=j.channel_id
 AND rev.manifest->'memory'=j.binding AND active.manifest->'memory'=j.binding
 AND lower(rev.manifest#>>'{office,public_key}')=lower(active.manifest#>>'{office,public_key}')
 AND rev.manifest#>>'{office,signer_ref}'=active.manifest#>>'{office,signer_ref}'
 AND mb.validated_at IS NOT NULL AND amb.validated_at IS NOT NULL
 AND jsonb_build_object('adapter',mb.adapter,'endpoint_ref',mb.endpoint_ref,'workspace',mb.workspace,
     'user_peer',mb.user_peer,'employee_peer',mb.employee_peer,'options',mb.options)=j.binding
 AND jsonb_build_object('adapter',amb.adapter,'endpoint_ref',amb.endpoint_ref,'workspace',amb.workspace,
     'user_peer',amb.user_peer,'employee_peer',amb.employee_peer,'options',amb.options)=j.binding
 AND EXISTS (SELECT 1 FROM employee_office_bindings b WHERE b.company_id=r.company_id AND b.employee_id=r.employee_id
     AND encode(b.public_key,'hex')=lower(rev.manifest#>>'{office,public_key}')
     AND b.signer_ref=rev.manifest#>>'{office,signer_ref}' AND b.verified_at IS NOT NULL
     AND b.valid_from<=clock_timestamp() AND (b.valid_until IS NULL OR b.valid_until>clock_timestamp()))
 AND NOT EXISTS (SELECT 1 FROM runtime_cancellations x WHERE x.company_id=r.company_id AND x.run_id=r.id)
 AND NOT EXISTS (SELECT 1 FROM run_cancel_requests x WHERE x.company_id=r.company_id AND x.run_id=r.id)
) AS authorized
FROM runtime_memory_writes j
JOIN companies c ON c.id=j.company_id
JOIN runs r ON r.company_id=j.company_id AND r.id=j.run_id
JOIN employees e ON e.company_id=r.company_id AND e.id=r.employee_id
JOIN employee_revisions rev ON rev.company_id=r.company_id AND rev.employee_id=r.employee_id AND rev.id=r.employee_revision_id
LEFT JOIN employee_revisions active ON active.company_id=e.company_id AND active.employee_id=e.id AND active.id=e.active_revision_id
LEFT JOIN employee_memory_bindings mb ON mb.company_id=r.company_id AND mb.employee_id=r.employee_id AND mb.revision_id=r.employee_revision_id
LEFT JOIN employee_memory_bindings amb ON amb.company_id=e.company_id AND amb.employee_id=e.id AND amb.revision_id=e.active_revision_id
LEFT JOIN outbox o ON o.company_id=j.company_id AND o.id=j.outbox_id
LEFT JOIN runtime_office_outputs output ON output.company_id=j.company_id AND output.run_id=j.run_id
LEFT JOIN office_inbox i ON i.company_id=r.company_id AND i.event_id=r.message_id
WHERE j.company_id=$1 AND j.run_id=$2 AND j.state='pending' AND j.lease_token=$3
 AND j.lease_expires_at>clock_timestamp() FOR UPDATE OF j
"#;
