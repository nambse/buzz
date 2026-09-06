WITH source AS MATERIALIZED (
 SELECT s.*,i.event_id,i.event_created_at,i.author_pubkey,i.received_at,
        e.active_revision_id,e.lifecycle_epoch,
        least(i.received_at+interval '120 seconds',b.valid_until,ch.ttl_deadline,$6::timestamptz) AS valid_before,
        ortak_encrypted_dm_outer(s.company_id,s.community_id,i.event_id,i.event_created_at,s.employee_public_key) AS outer_bytes
 FROM encrypted_dm_selections s
 JOIN employees e ON e.company_id=s.company_id AND e.id=s.employee_id
 JOIN employee_office_bindings b ON b.company_id=s.company_id AND b.id=s.office_binding_id
 JOIN channels ch ON ch.community_id=s.community_id AND ch.id=s.channel_id
 JOIN office_inbox i ON i.company_id=s.company_id AND i.event_id=$3 AND i.event_created_at=$4
 WHERE s.company_id=$1 AND s.selection_id=$2 AND s.enabled AND i.received_at>=s.enabled_at
  AND ortak_encrypted_dm_pair_current(s)
)
INSERT INTO encrypted_dm_decrypt_jobs(company_id,community_id,source_id,source_created_at,source_author,source_hash,
 source_received_at,selection_id,selection_generation,employee_id,employee_revision_id,employee_lifecycle_epoch,
 office_generation,valid_before,deadline)
SELECT company_id,community_id,event_id,event_created_at,author_pubkey,digest(outer_bytes,'sha256'),
 received_at,selection_id,generation,employee_id,active_revision_id,lifecycle_epoch,$5,valid_before,
 received_at+interval '120 seconds'
FROM source WHERE outer_bytes IS NOT NULL AND valid_before>clock_timestamp()
ON CONFLICT(company_id,source_id) DO NOTHING RETURNING source_id
