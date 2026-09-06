"""Explicit77 retained employee/protected history; never current-use authority.

The enclosing observer supplies one bounded repeatable-read snapshot. SQL below
only reads retained rows and pure encoders. Public evidence contains complete-row
hashes, never fact text, protected envelopes, opaque key references or key bytes.
"""

import re

from backup_private_database import Refused
import private_recovery_conversations as conversations

TABLE_KEYS = {
    'employee_memory_channel_authorities': ('company_id', 'community_id', 'employee_id', 'channel_id'),
    'employee_reviewed_memory_facts': ('company_id', 'id'),
    'employee_reviewed_memory_operations': ('company_id', 'actor_public_key', 'operation_id'),
    'employee_reviewed_memory_targets': ('company_id', 'id'),
    'employee_reviewed_memory_exports': ('company_id', 'fact_id'),
    'employee_reviewed_memory_export_jobs': ('company_id', 'fact_id', 'action'),
    'employee_reviewed_memory_export_commands': ('company_id', 'actor_pubkey', 'operation_id'),
    'employee_reviewed_memory_export_receipts': ('company_id', 'fact_id', 'action'),
    'run_employee_reviewed_memory_uses': ('company_id', 'run_id', 'ordinal'),
    'encrypted_dm_selections': ('company_id', 'selection_id'),
    'encrypted_dm_decrypt_jobs': ('company_id', 'source_id'),
    'confidential_runs': ('company_id', 'run_id'),
    'confidential_run_payloads': ('company_id', 'run_id', 'purpose', 'ordinal'),
    'confidential_dm_receipts': ('company_id', 'source_id'),
    'confidential_run_dispatches': ('company_id', 'run_id'),
    'confidential_execution_leases': ('company_id', 'run_id'),
    'confidential_event_receipts': ('company_id', 'run_id', 'ordinal'),
    'confidential_reply_bundles': ('company_id', 'run_id'),
    'confidential_reply_outbox': ('company_id', 'run_id', 'copy'),
}
HONCHO_KEYS = {
    'ortak_employee_reviewed_records': ('workspace_id', 'employee_id', 'record_id'),
    'ortak_employee_reviewed_content': ('workspace_id', 'employee_id', 'record_id'),
    'ortak_employee_reviewed_tombstones': ('workspace_id', 'employee_id', 'record_id'),
    'ortak_employee_reviewed_operations': ('workspace_id', 'employee_id', 'idempotency_key'),
    'ortak_employee_diagnostics': ('workspace_id', 'employee_id', 'operation_id'),
    'ortak_employee_diagnostic_content': ('workspace_id', 'employee_id', 'operation_id'),
    'ortak_employee_diagnostic_tombstones': ('workspace_id', 'employee_id', 'operation_id'),
}
ACTIVATION_GATES = ['employee_namespace_and_current_use_revalidated',
                    'protected_bridge_and_reply_owners_contained',
                    'explicit_protected_store_selection_and_restore']


def contract():
    """Storage approval neither opens native stores nor activates protected runs."""
    return {'schema_version': 77, 'employee_protocol': 'reviewed-employee/1',
            'snapshot_version': 5, 'historical_epochs': 'at_most_retained_epoch',
            'protected_payload': 'ciphertext_only_no_decryption',
            'automatic_activation': False}


def _sql(value):
    # Counters are rendered by the existing observer with str.format(company=).
    # Escape literal JSON/regex braces once; only the scoped sentinel is replaced.
    return value.replace('{', '{{').replace('}', '}}').replace('@COMPANY@', '{company}')


HISTORY = {
    'invalid_employee_fact_history77': _sql("""
SELECT count(*) FROM employee_reviewed_memory_facts f
WHERE f.company_id='@COMPANY@' AND NOT coalesce(
 f.audience_hash=sha256(f.audience_bytes) AND f.sharing_hash=sha256(f.provenance_bytes)
 AND f.content_hash=sha256(convert_to(f.content,'UTF8'))
 AND f.audience_bytes=convert_to(ortak_conversation_json75(ortak_employee_memory_audience(f)),'UTF8')
 AND f.source_hash=sha256(convert_to(ortak_conversation_json75(jsonb_build_object(
  'audience_hash',encode(f.audience_hash,'hex'),'format','ortak-reviewed-employee-source/1',
  'source',ortak_employee_memory_source(f))),'UTF8'))
 AND f.provenance_bytes=convert_to(ortak_conversation_json75(jsonb_build_object(
  'format','ortak-reviewed-employee-provenance/1','audience',ortak_employee_memory_audience(f),
  'audience_hash',encode(f.audience_hash,'hex'),'source',ortak_employee_memory_source(f),
  'source_hash',encode(f.source_hash,'hex'),'approval',jsonb_build_object(
   'format','ortak-reviewed-employee-sharing/1','approval_id',f.approval_id,
   'approved_by',encode(f.approved_by,'hex'),'content_hash',encode(f.content_hash,'hex'),
   'expires_at',ortak_employee_memory_timestamp(f.expires_at)))),'UTF8')
 AND EXISTS(SELECT 1 FROM employee_reviewed_memory_operations o WHERE o.company_id=f.company_id
  AND o.community_id=f.community_id AND o.fact_id=f.id AND o.actor_public_key=f.approved_by
  AND o.operation_id=f.approval_id AND o.action='approve' AND o.result_version=1)
 AND (f.version=1 OR EXISTS(SELECT 1 FROM employee_reviewed_memory_operations o
  WHERE o.company_id=f.company_id AND o.fact_id=f.id AND o.action='stop' AND o.result_version=2
   AND o.actor_public_key=f.approved_by AND o.community_id=f.community_id))
 AND EXISTS(SELECT 1 FROM employee_memory_channel_authorities a WHERE a.company_id=f.company_id
  AND a.community_id=f.community_id AND a.employee_id=f.employee_id AND a.channel_id=f.source_channel_id)
 AND EXISTS(SELECT 1 FROM employee_memory_channel_authorities a WHERE a.company_id=f.company_id
  AND a.community_id=f.community_id AND a.employee_id=f.employee_id AND a.channel_id=f.destination_channel_id),false)"""),
    'invalid_employee_operation_history77': _sql("""
SELECT count(*) FROM employee_reviewed_memory_operations o
LEFT JOIN employee_reviewed_memory_facts f ON f.company_id=o.company_id AND f.id=o.fact_id
WHERE o.company_id='@COMPANY@' AND NOT coalesce(f.id IS NOT NULL
 AND f.community_id=o.community_id AND f.approved_by=o.actor_public_key
 AND o.submitted_hash=sha256(o.submitted_bytes)
 AND o.submitted_bytes=ortak_employee_memory_submission(f,o.operation_id,o.action)
 AND ((o.action='approve' AND o.result_version=1 AND o.operation_id=f.approval_id)
  OR (o.action='stop' AND o.result_version=2 AND f.version=2)),false)"""),
    'invalid_employee_target_history77': _sql("""
SELECT count(*) FROM employee_reviewed_memory_targets t WHERE t.company_id='@COMPANY@'
AND NOT coalesce(t.protocol='reviewed-employee/1'
 AND t.namespace_bytes=convert_to(ortak_conversation_json75(jsonb_build_object(
  'format','ortak-reviewed-employee-namespace/1','company_id',t.company_id,'employee_id',t.employee_id)),'UTF8')
 AND t.namespace_hash=sha256(t.namespace_bytes)
 AND t.binding_hash=sha256(convert_to(ortak_conversation_json75(jsonb_build_object(
  'binding',t.binding,'namespace_hash',encode(t.namespace_hash,'hex'),'protocol',t.protocol)),'UTF8'))
 AND t.creation_receipt->>'company_id'=t.company_id::text
 AND t.creation_receipt->>'employee_id'=t.employee_id
 AND t.creation_receipt->>'deployment_id'=t.deployment_id::text
 AND t.creation_receipt->'binding'=t.binding
 AND t.creation_receipt->>'protocol'=t.protocol
 AND t.creation_receipt->>'namespace_hash'=encode(t.namespace_hash,'hex')
 AND jsonb_typeof(t.registration_receipt)='object'
 AND t.registration_receipt->>'format'='ortak-employee-namespace-registration/1'
 AND t.valid_until<=(t.registration_receipt->>'validated_at')::timestamptz+interval '90 days'
 AND t.registration_receipt#>'{diagnostic,erased}'='true'::jsonb
 AND isfinite((t.registration_receipt->>'validated_at')::timestamptz)
 AND isfinite((t.registration_receipt#>>'{diagnostic,tombstone_at}')::timestamptz)
 AND t.registration_receipt#>>'{diagnostic,withdraw_request_hash}'=encode(sha256(convert_to(
  ortak_conversation_json75(jsonb_build_object('format','ortak-reviewed-employee-diagnostic-withdraw/1',
   'operation_id',(t.registration_receipt#>>'{diagnostic,operation_id}')::uuid,
   'namespace_hash',encode(t.namespace_hash,'hex'),'binding_hash',encode(t.binding_hash,'hex'),
   'employee_revision_id',(t.registration_receipt#>>'{diagnostic,employee_revision_id}')::uuid,
   'employee_lifecycle_epoch',(t.registration_receipt#>>'{diagnostic,employee_lifecycle_epoch}')::bigint,
   'challenge_hash',t.registration_receipt#>>'{diagnostic,challenge_hash}')),'UTF8')),'hex'),false)"""),
    'invalid_employee_export_history77': _sql("""
SELECT count(*) FROM employee_reviewed_memory_exports x
LEFT JOIN employee_reviewed_memory_facts f ON f.company_id=x.company_id AND f.id=x.fact_id
LEFT JOIN employee_reviewed_memory_targets t ON t.company_id=x.company_id AND t.id=x.target_id
WHERE x.company_id='@COMPANY@' AND NOT coalesce(f.id IS NOT NULL AND t.id IS NOT NULL
 AND x.community_id=f.community_id AND x.community_id=t.community_id
 AND x.employee_id=f.employee_id AND x.employee_id=t.employee_id
 AND x.destination_channel_id=f.destination_channel_id AND x.destination_channel_id=t.destination_channel_id
 AND x.content_hash=f.content_hash AND x.source_hash=f.source_hash AND x.sharing_hash=f.sharing_hash
 AND x.requested_by=encode(f.approved_by,'hex')
 AND EXISTS(SELECT 1 FROM employee_reviewed_memory_export_commands c WHERE c.company_id=x.company_id
  AND c.community_id=x.community_id AND c.fact_id=x.fact_id AND c.actor_pubkey=x.requested_by
  AND c.operation_id=x.operation_id AND c.action='publish' AND c.result_version=0),false)"""),
    'invalid_employee_export_command_history77': _sql("""
SELECT count(*) FROM employee_reviewed_memory_export_commands o
LEFT JOIN employee_reviewed_memory_facts f ON f.company_id=o.company_id AND f.id=o.fact_id
WHERE o.company_id='@COMPANY@' AND NOT coalesce(f.id IS NOT NULL
 AND o.community_id=f.community_id AND o.actor_pubkey=encode(f.approved_by,'hex')
 AND o.request_hash=sha256(convert_to(ortak_conversation_json75(jsonb_build_object(
  'format','ortak-reviewed-employee-export-command/1','operation_id',o.operation_id,
  'fact_id',o.fact_id,'action',o.action,'expected_version',CASE WHEN o.action='publish' THEN 1 ELSE o.result_version-1 END)),'UTF8'))
 AND ((o.action='publish' AND o.result_version=0) OR EXISTS(SELECT 1 FROM employee_reviewed_memory_export_jobs j
  WHERE j.company_id=o.company_id AND j.fact_id=o.fact_id AND 'retry_'||j.action=o.action
   AND o.result_version BETWEEN 1 AND j.retry_version)),false)"""),
    'invalid_employee_job_receipt_history77': _sql("""
SELECT count(*) FROM employee_reviewed_memory_export_jobs j
LEFT JOIN employee_reviewed_memory_exports x ON x.company_id=j.company_id AND x.fact_id=j.fact_id
LEFT JOIN employee_reviewed_memory_targets t ON t.company_id=x.company_id AND t.id=x.target_id
LEFT JOIN employee_reviewed_memory_export_receipts r ON r.company_id=j.company_id AND r.fact_id=j.fact_id AND r.action=j.action
WHERE j.company_id='@COMPANY@' AND NOT coalesce(x.fact_id IS NOT NULL AND t.id IS NOT NULL
 AND j.community_id=x.community_id AND j.community_id=t.community_id
 AND j.idempotency_key='employee-reviewed:'||j.action||':'||j.company_id::text||':'||j.fact_id::text
 AND j.request_hash=ortak_employee_reviewed_request_hash(j.company_id,j.fact_id,j.action)
 AND ((j.state<>'acknowledged' AND r.fact_id IS NULL) OR (j.state='acknowledged'
  AND r.community_id=j.community_id AND r.request_hash=j.request_hash AND r.binding_hash=t.binding_hash
  AND r.lease_token=j.lease_token AND r.total_attempts=j.total_attempts
  AND (r.content_hash=x.content_hash OR (r.content_hash IS NULL AND j.action='withdraw'))
  AND ((r.remote_status='withdrawn' AND r.erased_from_reviewed_store AND r.tombstone_at IS NOT NULL)
   OR (j.action='publish' AND r.remote_status IN('active','expired') AND NOT r.erased_from_reviewed_store
    AND r.tombstone_at IS NULL)))),false)"""),
    'invalid_employee_use_history77': _sql("""
SELECT count(*) FROM run_employee_reviewed_memory_uses u
LEFT JOIN runs r ON r.company_id=u.company_id AND r.id=u.run_id
LEFT JOIN employee_reviewed_memory_facts f ON f.company_id=u.company_id AND f.id=u.fact_id
LEFT JOIN employee_reviewed_memory_exports x ON x.company_id=u.company_id AND x.fact_id=u.fact_id
LEFT JOIN employee_reviewed_memory_targets t ON t.company_id=u.company_id AND t.id=u.target_id
LEFT JOIN employee_memory_channel_authorities src ON src.company_id=u.company_id
 AND src.community_id=u.community_id AND src.employee_id=f.employee_id AND src.channel_id=f.source_channel_id
LEFT JOIN employee_memory_channel_authorities dst ON dst.company_id=u.company_id
 AND dst.community_id=u.community_id AND dst.employee_id=f.employee_id AND dst.channel_id=f.destination_channel_id
WHERE u.company_id='@COMPANY@' AND NOT coalesce(r.id IS NOT NULL AND r.payload_mode='ordinary'
 AND f.id IS NOT NULL AND t.id IS NOT NULL AND x.target_id=u.target_id AND f.employee_id=r.employee_id
 AND u.community_id=f.community_id AND u.community_id=t.community_id AND u.community_id=x.community_id
 AND u.fact_version=1 AND u.content_hash=f.content_hash AND u.source_hash=f.source_hash
 AND u.sharing_hash=f.sharing_hash AND u.audience_hash=f.audience_hash AND u.binding_hash=t.binding_hash
 AND u.namespace_hash=t.namespace_hash AND u.approval_id=f.approval_id
 AND u.approved_by=encode(f.approved_by,'hex') AND u.expires_at=f.expires_at
 AND u.source_authority_epoch<=src.epoch AND u.destination_authority_epoch<=dst.epoch
 AND u.consumption_epoch<=t.consumption_epoch
 AND EXISTS(SELECT 1 FROM run_context_snapshots s WHERE s.company_id=u.company_id AND s.run_id=u.run_id
  AND ortak_snapshot_scratch_jsonb(convert_from(s.spec_bytes,'UTF8')::json)->'version'='5'::jsonb),false)"""),
}

# Pure retained encoders below inspect public envelope metadata only. The actual
# AEAD/NIP44 authentication remains the protected runtime's responsibility.
HISTORY.update({
    'invalid_protected_job_history77': _sql("""
SELECT count(*) FROM encrypted_dm_decrypt_jobs j
LEFT JOIN encrypted_dm_selections s ON s.company_id=j.company_id AND s.selection_id=j.selection_id
LEFT JOIN employee_revisions r ON r.company_id=j.company_id AND r.employee_id=j.employee_id AND r.id=j.employee_revision_id
WHERE j.company_id='@COMPANY@' AND NOT coalesce(s.selection_id IS NOT NULL AND r.id IS NOT NULL
 AND s.community_id=j.community_id AND s.employee_id=j.employee_id AND j.selection_generation<=s.generation
 AND j.attempts=j.claim_generation AND (j.state IN('claimed','verified'))=(j.claim_token IS NOT NULL)
 AND (j.state<>'verified' OR (j.verified_at IS NOT NULL AND j.rumor_id IS NOT NULL AND j.seal_id IS NOT NULL
  AND j.claim_token IS NOT NULL AND j.worker_id IS NOT NULL)),false)"""),
    'invalid_protected_admission_history77': _sql("""
SELECT count(*) FROM confidential_runs c
LEFT JOIN runs r ON r.company_id=c.company_id AND r.id=c.run_id
LEFT JOIN encrypted_dm_decrypt_jobs j ON j.company_id=c.company_id AND j.source_id=c.source_id
LEFT JOIN encrypted_dm_selections s ON s.company_id=c.company_id AND s.selection_id=c.selection_id
CROSS JOIN LATERAL (SELECT convert_from(c.wrapped_key,'UTF8')::jsonb AS value) wrapped
WHERE c.company_id='@COMPANY@' AND NOT coalesce(r.payload_mode='confidential_dm_v1'
 AND r.employee_id=c.employee_id AND c.community_id=j.community_id AND c.community_id=s.community_id
 AND c.employee_id=j.employee_id AND c.selection_id=j.selection_id AND c.human_public_key=s.human_public_key
 AND c.run_id=ortak_confidential_dm_run_id(c.company_id,c.source_id)
 AND c.claim_token=j.claim_token AND c.claim_generation=j.claim_generation AND c.claim_worker=j.worker_id
 AND j.state='verified' AND c.rumor_id=j.rumor_id
 AND c.identity_bytes=ortak_confidential_dm_identity(c.company_id,c.source_id,c.run_id,c.key_id)
 AND c.source_bytes=ortak_confidential_dm_source(c.company_id,c.source_id)
 AND c.wrapped_key=convert_to(ortak_conversation_json75(wrapped.value),'UTF8')
 AND wrapped.value-ARRAY['ciphertext','format','identity','purpose','signer_ref']='{}'::jsonb
 AND wrapped.value->>'format'='ortak-confidential-key-envelope/1' AND wrapped.value->>'purpose'='confidential_master'
 AND convert_to(wrapped.value->>'identity','UTF8')=c.identity_bytes AND wrapped.value->>'signer_ref'=s.decrypt_ref
 AND octet_length(decode(wrapped.value->>'ciphertext','base64'))>=99
 AND get_byte(decode(wrapped.value->>'ciphertext','base64'),0)=2
 AND r.employee_revision_id=j.employee_revision_id AND r.employee_lifecycle_epoch=j.employee_lifecycle_epoch
 AND r.message_id=c.source_id AND r.root_message_id=c.source_id AND r.work_item_id IS NULL
 AND j.selection_generation<=s.generation
 AND c.start_key='ortak-run:'||c.company_id::text||':'||c.run_id::text
 AND EXISTS(SELECT 1 FROM confidential_dm_receipts a WHERE a.company_id=c.company_id AND a.source_id=c.source_id
  AND a.run_id=c.run_id AND NOT a.duplicate_rumor)
 AND EXISTS(SELECT 1 FROM confidential_run_payloads p WHERE p.company_id=c.company_id AND p.run_id=c.run_id
  AND p.purpose='snapshot' AND p.ordinal=0),false)"""),
    'invalid_protected_receipt_history77': _sql("""
SELECT count(*) FROM confidential_dm_receipts a
LEFT JOIN confidential_runs c ON c.company_id=a.company_id AND c.run_id=a.run_id
LEFT JOIN encrypted_dm_decrypt_jobs j ON j.company_id=a.company_id AND j.source_id=a.source_id
LEFT JOIN encrypted_dm_selections s ON s.company_id=j.company_id AND s.selection_id=j.selection_id
WHERE a.company_id='@COMPANY@' AND NOT coalesce(c.run_id IS NOT NULL AND j.state='verified'
 AND a.community_id=c.community_id AND a.community_id=j.community_id AND s.community_id=a.community_id
 AND a.claim_generation=j.claim_generation AND a.claim_token=j.claim_token AND a.claim_worker=j.worker_id
 AND j.employee_id=c.employee_id AND s.human_public_key=c.human_public_key AND j.rumor_id=c.rumor_id
 AND a.duplicate_rumor=(a.source_id<>c.source_id),false)"""),
    'invalid_protected_payload_history77': _sql("""
SELECT count(*) FROM confidential_run_payloads p
LEFT JOIN confidential_runs c ON c.company_id=p.company_id AND c.run_id=p.run_id
WHERE p.company_id='@COMPANY@' AND NOT coalesce(c.run_id IS NOT NULL AND p.community_id=c.community_id
 AND ortak_confidential_payload_valid(p.envelope_bytes,c.identity_bytes,p.purpose,p.ordinal)
 AND p.nonce=decode(convert_from(p.envelope_bytes,'UTF8')::jsonb->>'nonce','base64')
 AND (p.purpose<>'runtime_event' OR EXISTS(SELECT 1 FROM confidential_event_receipts e
  WHERE e.company_id=p.company_id AND e.run_id=p.run_id AND e.ordinal=p.ordinal AND e.community_id=p.community_id)),false)"""),
    'invalid_protected_mode_history77': _sql("""
SELECT count(*) FROM runs r WHERE r.company_id='@COMPANY@' AND
 ((r.payload_mode='confidential_dm_v1') IS DISTINCT FROM EXISTS(SELECT 1 FROM confidential_runs c
  WHERE c.company_id=r.company_id AND c.run_id=r.id)
 OR (r.payload_mode='confidential_dm_v1' AND (
  EXISTS(SELECT 1 FROM run_context_snapshots s WHERE s.company_id=r.company_id AND s.run_id=r.id)
  OR EXISTS(SELECT 1 FROM run_events e WHERE e.company_id=r.company_id AND e.run_id=r.id)
  OR EXISTS(SELECT 1 FROM run_reviewed_memory_uses u WHERE u.company_id=r.company_id AND u.run_id=r.id)
  OR EXISTS(SELECT 1 FROM run_employee_reviewed_memory_uses u WHERE u.company_id=r.company_id AND u.run_id=r.id)
  OR EXISTS(SELECT 1 FROM run_workspace_uses u WHERE u.company_id=r.company_id AND u.run_id=r.id)
  OR EXISTS(SELECT 1 FROM runtime_office_outputs u WHERE u.company_id=r.company_id AND u.run_id=r.id)
  OR EXISTS(SELECT 1 FROM runtime_work_outputs u WHERE u.company_id=r.company_id AND u.run_id=r.id)
  OR EXISTS(SELECT 1 FROM runtime_memory_writes u WHERE u.company_id=r.company_id AND u.run_id=r.id)
  OR EXISTS(SELECT 1 FROM workspace_tool_actions u WHERE u.company_id=r.company_id AND u.run_id=r.id)
  OR EXISTS(SELECT 1 FROM workspace_tool_receipts u WHERE u.company_id=r.company_id AND u.run_id=r.id)
  OR EXISTS(SELECT 1 FROM workspace_reader_executions u WHERE u.company_id=r.company_id AND u.run_id=r.id)
  OR EXISTS(SELECT 1 FROM work_executions u WHERE u.company_id=r.company_id AND u.run_id=r.id)
  OR EXISTS(SELECT 1 FROM artifacts u WHERE u.company_id=r.company_id AND u.run_id=r.id)
  OR EXISTS(SELECT 1 FROM work_attachments u WHERE u.company_id=r.company_id AND u.run_id=r.id)
  OR EXISTS(SELECT 1 FROM outbox u WHERE u.company_id=r.company_id AND u.run_id=r.id))))"""),
    'invalid_protected_terminal_history77': _sql("""
SELECT count(*) FROM confidential_execution_leases e
LEFT JOIN confidential_runs c ON c.company_id=e.company_id AND c.run_id=e.run_id
LEFT JOIN runs r ON r.company_id=e.company_id AND r.id=e.run_id
WHERE e.company_id='@COMPANY@' AND NOT coalesce(c.run_id IS NOT NULL AND c.community_id=e.community_id
 AND (e.state<>'stopped' OR EXISTS(SELECT 1 FROM runtime_cancellations x
  WHERE x.company_id=e.company_id AND x.run_id=e.run_id AND x.state='acknowledged'))
 AND (e.state<>'complete' OR (r.status='completed' AND (
  (r.delivery_intent='silent' AND (SELECT count(*) FROM confidential_event_receipts x
   WHERE x.company_id=e.company_id AND x.run_id=e.run_id)=3)
  OR (r.delivery_intent='reply' AND (SELECT count(*) FROM confidential_event_receipts x
   WHERE x.company_id=e.company_id AND x.run_id=e.run_id)=4
   AND EXISTS(SELECT 1 FROM confidential_run_payloads p WHERE p.company_id=e.company_id
    AND p.run_id=e.run_id AND p.purpose='reply_draft' AND p.ordinal=0)
   AND EXISTS(SELECT 1 FROM confidential_reply_bundles b WHERE b.company_id=e.company_id AND b.run_id=e.run_id)))))
 AND NOT EXISTS(SELECT 1 FROM confidential_event_receipts x WHERE x.company_id=e.company_id AND x.run_id=e.run_id
  AND (x.community_id<>e.community_id OR x.ordinal<>(SELECT count(*) FROM confidential_event_receipts y
   WHERE y.company_id=x.company_id AND y.run_id=x.run_id AND y.ordinal<=x.ordinal))),false)"""),
    'invalid_protected_reply_history77': _sql("""
SELECT count(*) FROM confidential_reply_bundles b
LEFT JOIN confidential_runs c ON c.company_id=b.company_id AND c.run_id=b.run_id
WHERE b.company_id='@COMPANY@' AND NOT coalesce(c.run_id IS NOT NULL AND c.community_id=b.community_id
 AND (SELECT count(*) FROM confidential_reply_outbox o WHERE o.company_id=b.company_id AND o.run_id=b.run_id)=2
 AND NOT EXISTS(SELECT 1 FROM confidential_reply_outbox o WHERE o.company_id=b.company_id AND o.run_id=b.run_id
  AND (o.community_id<>b.community_id OR (o.state='acked') IS DISTINCT FROM (o.acknowledged_at IS NOT NULL)
   OR (o.state<>'pending' AND (o.lease_token IS NOT NULL OR o.finished_at IS NULL))))
 AND NOT EXISTS(SELECT 1 FROM (VALUES(0,b.recipient_id,b.recipient_bytes,'human_public_key'),
  (1,b.history_id,b.history_bytes,'employee_public_key')) copy(n,id,bytes,target)
  CROSS JOIN LATERAL (SELECT convert_from(copy.bytes,'UTF8')::jsonb AS wire) w
  WHERE NOT coalesce(w.wire->>'id'=encode(copy.id,'hex') AND w.wire->'kind'='1059'::jsonb
   AND w.wire->'tags'=jsonb_build_array(jsonb_build_array('p',convert_from(c.identity_bytes,'UTF8')::jsonb->>copy.target))
   AND w.wire-ARRAY['id','pubkey','created_at','kind','tags','content','sig']='{}'::jsonb
   AND jsonb_typeof(w.wire->'content')='string' AND octet_length(w.wire->>'content') BETWEEN 132 AND 60000,false)),false)"""),
})

DRAIN = {
    'unsettled_employee_export_jobs77': _sql("""
SELECT count(*) FROM employee_reviewed_memory_export_jobs j WHERE j.company_id='@COMPANY@'
 AND (j.state='failed' OR (j.state='pending' AND (j.action='publish' OR j.lease_token IS NOT NULL
  OR j.total_attempts>0 OR j.last_error_code IS NOT NULL OR j.next_attempt_at<=clock_timestamp())))"""),
    'incomplete_employee_export_pairs77': _sql("""
SELECT count(*) FROM employee_reviewed_memory_exports x WHERE x.company_id='@COMPANY@'
AND (NOT EXISTS(SELECT 1 FROM employee_reviewed_memory_export_jobs p WHERE p.company_id=x.company_id
 AND p.fact_id=x.fact_id AND p.action='publish' AND p.state='acknowledged')
 OR NOT EXISTS(SELECT 1 FROM employee_reviewed_memory_export_jobs w WHERE w.company_id=x.company_id
 AND w.fact_id=x.fact_id AND w.action='withdraw' AND (w.state='acknowledged' OR
  (w.state='pending' AND w.total_attempts=0 AND w.lease_token IS NULL AND w.last_error_code IS NULL
   AND w.next_attempt_at>clock_timestamp()))))"""),
    'active_employee_consumers77': _sql("""
SELECT count(DISTINCT u.run_id) FROM run_employee_reviewed_memory_uses u
LEFT JOIN runs r ON r.company_id=u.company_id AND r.id=u.run_id
WHERE u.company_id='@COMPANY@' AND (r.id IS NULL OR r.status NOT IN('completed','failed','cancelled'))"""),
    'unsettled_protected_decrypt77': _sql("""
SELECT count(*) FROM encrypted_dm_decrypt_jobs j WHERE j.company_id='@COMPANY@'
 AND (j.state IN('pending','claimed') OR (j.state='verified' AND NOT EXISTS(
 SELECT 1 FROM confidential_dm_receipts a WHERE a.company_id=j.company_id AND a.source_id=j.source_id)))"""),
    'unsettled_protected_runs77': _sql("""
SELECT count(*) FROM confidential_runs c
LEFT JOIN runs r ON r.company_id=c.company_id AND r.id=c.run_id
LEFT JOIN confidential_run_dispatches d ON d.company_id=c.company_id AND d.run_id=c.run_id
LEFT JOIN confidential_execution_leases e ON e.company_id=c.company_id AND e.run_id=c.run_id
WHERE c.company_id='@COMPANY@' AND NOT coalesce(r.status IN('completed','failed','cancelled')
 AND d.community_id=c.community_id AND e.community_id=c.community_id
 AND d.state IN('delivered','failed','cancelled') AND d.lease_token IS NULL AND d.lease_expires_at IS NULL
 AND e.state IN('complete','stopped') AND e.lease_token IS NULL AND e.lease_expires_at IS NULL
 AND (d.state='delivered' OR e.state='stopped'),false)"""),
    'unsettled_protected_replies77': _sql("""
SELECT count(*) FROM confidential_reply_outbox o WHERE o.company_id='@COMPANY@'
 AND (o.state='pending' OR o.lease_token IS NOT NULL OR o.lease_expires_at IS NOT NULL)"""),
}


# Build the reviewed union from retained rows, not from snapshot claims. This
# proof deliberately does not call the admission/current-use v5 SQL function.
HISTORY['invalid_employee_snapshot_history77'] = _sql(r"""
WITH parsed AS MATERIALIZED (
 SELECT s.*,ortak_snapshot_scratch_jsonb(convert_from(s.spec_bytes,'UTF8')::json) AS wire
 FROM run_context_snapshots s WHERE s.company_id='@COMPANY@'
), uses AS MATERIALIZED (
 SELECT u.company_id,u.run_id,u.ordinal,u.fact_id,'employee'::text AS scope,
  f.content,f.provenance_bytes,t.binding,u.expires_at,f.destination_channel_id,
  NULL::uuid AS project_id,CASE f.kind WHEN 'relationship' THEN 0 ELSE 1 END AS priority,
  jsonb_build_object('fact_id',u.fact_id,'target_id',u.target_id,'fact_version',u.fact_version,
   'content_hash',encode(u.content_hash,'hex'),'source_hash',encode(u.source_hash,'hex'),
   'sharing_hash',encode(u.sharing_hash,'hex'),'audience_hash',encode(u.audience_hash,'hex'),
   'binding_hash',encode(u.binding_hash,'hex'),'namespace_hash',encode(u.namespace_hash,'hex'),
   'approval_id',u.approval_id,'approved_by',u.approved_by,
   'source_authority_epoch',u.source_authority_epoch,'destination_authority_epoch',u.destination_authority_epoch,
   'consumption_epoch',u.consumption_epoch) AS pin
 FROM run_employee_reviewed_memory_uses u JOIN employee_reviewed_memory_facts f
  ON f.company_id=u.company_id AND f.id=u.fact_id
 JOIN employee_reviewed_memory_targets t ON t.company_id=u.company_id AND t.id=u.target_id
 WHERE u.company_id='@COMPANY@'
 UNION ALL
 SELECT u.company_id,u.run_id,u.ordinal,u.fact_id,f.audience_kind,f.content,a.provenance_bytes,
  t.binding,u.expires_at,NULL::uuid,f.project_id,2,
  jsonb_build_object('fact_id',u.fact_id,'target_id',u.target_id,'fact_version',u.fact_version,
   'content_hash',encode(u.content_hash,'hex'),'source_hash',encode(u.source_hash,'hex'),
   'binding_hash',encode(u.binding_hash,'hex'),'approval_id',u.approval_id,'approved_by',u.approved_by,
   'consumption_epoch',u.consumption_epoch)
   ||CASE WHEN f.audience_kind='conversation' THEN jsonb_build_object(
    'conversation_audience_hash',encode(u.conversation_audience_hash,'hex'),
    'conversation_authority_epoch',u.conversation_authority_epoch,
    'conversation_consumption_epoch',u.conversation_consumption_epoch) ELSE '{}'::jsonb END
 FROM run_reviewed_memory_uses u JOIN reviewed_memory_facts f ON f.company_id=u.company_id AND f.id=u.fact_id
 JOIN reviewed_memory_targets t ON t.company_id=u.company_id AND t.id=u.target_id
 LEFT JOIN reviewed_memory_conversation_audiences a ON a.company_id=u.company_id AND a.fact_id=u.fact_id
 WHERE u.company_id='@COMPANY@'
), history AS MATERIALIZED (
 SELECT s.*,r.employee_id,r.employee_revision_id,r.work_item_id,r.payload_mode,
  (SELECT min(u.destination_channel_id::text)::uuid FROM uses u WHERE u.run_id=s.run_id AND u.scope='employee') AS destination_channel_id,
  r.routing_decision_id,r.message_id,r.root_message_id,revision.manifest,
  d.origin_type,d.origin_id,d.message_id AS decision_message,d.root_message_id AS decision_root,
  w.run_id AS work_run,w.work_item_id AS execution_work,w.employee_id AS execution_employee,
  w.employee_revision_id AS execution_revision,w.project_id,w.requested_by,w.definition_bytes,w.definition_hash,
  w.execution_version,item.source_message_id,
  CASE WHEN jsonb_typeof(s.wire#>'{employee,records}')='array' THEN s.wire#>'{employee,records}' ELSE '[]'::jsonb END AS records,
  CASE WHEN jsonb_typeof(s.wire#>'{recall,records}')='array' THEN s.wire#>'{recall,records}' ELSE '[]'::jsonb END AS scratch,
  CASE WHEN jsonb_typeof(s.wire#>'{spec,context,memory_context}')='array' THEN s.wire#>'{spec,context,memory_context}' ELSE '[]'::jsonb END AS rendered,
  (s.wire#>>'{employee,origin}')::jsonb AS origin
 FROM parsed s LEFT JOIN runs r ON r.company_id=s.company_id AND r.id=s.run_id
 LEFT JOIN employee_revisions revision ON revision.company_id=r.company_id
  AND revision.employee_id=r.employee_id AND revision.id=r.employee_revision_id
 LEFT JOIN routing_decisions d ON d.company_id=r.company_id AND d.id=r.routing_decision_id
 LEFT JOIN work_executions w ON w.company_id=r.company_id AND w.run_id=r.id
 LEFT JOIN work_items item ON item.company_id=w.company_id AND item.id=w.work_item_id
 WHERE s.wire->'version'='5'::jsonb OR s.wire ? 'employee'
  OR EXISTS(SELECT 1 FROM run_employee_reviewed_memory_uses u WHERE u.company_id=s.company_id AND u.run_id=s.run_id)
), bad AS (
 SELECT h.run_id FROM history h WHERE NOT coalesce(h.wire->'version'='5'::jsonb
  AND h.spec_hash=sha256(h.spec_bytes) AND h.payload_mode='ordinary' AND h.manifest IS NOT NULL
  AND h.wire->>'company_id'=h.company_id::text AND h.wire#>>'{spec,run_id}'=h.run_id::text
  AND h.wire#>>'{spec,employee_id}'=h.employee_id AND h.wire#>>'{spec,revision_id}'=h.employee_revision_id::text
  AND h.wire#>>'{spec,idempotency_key}'='ortak-run:'||h.company_id::text||':'||h.run_id::text
  AND h.wire#>'{spec,binding}'=ortak_snapshot_scratch_jsonb((h.manifest->'runtime')::json)
  AND h.wire#>'{spec,permissions}'=ortak_snapshot_scratch_jsonb((h.manifest->'permissions')::json)
  AND h.wire->'memory_binding'=ortak_snapshot_scratch_jsonb((h.manifest->'memory')::json)
  AND h.wire-ARRAY['version','company_id','routing_decision_id','message_id','root_message_id','event_kind',
   'input_truncated','memory_binding','recall','spec','work_origin','employee']='{}'::jsonb
  AND (h.wire->'spec')-ARRAY['run_id','employee_id','revision_id','binding','permissions','input','context','idempotency_key']='{}'::jsonb
  AND (h.wire#>'{spec,context}')-ARRAY['conversation_ref','reply_to_message_id','work_item_id','memory_context']='{}'::jsonb
  AND jsonb_typeof(h.wire#>'{spec,input}')='string' AND btrim(h.wire#>>'{spec,input}')<>''
  AND octet_length(h.wire#>>'{spec,input}')-(octet_length(h.wire#>>'{spec,input}')
   -octet_length(regexp_replace(h.wire#>>'{spec,input}',E'\x01[\x01\x02]','','g')))/2<=65536
  AND NOT h.wire ? 'reviewed' AND NOT h.wire ? 'conversation'
  AND (h.wire->'employee')-ARRAY['origin','conversation_origin','records','truncated']='{}'::jsonb
  AND jsonb_typeof(h.wire#>'{employee,truncated}')='boolean'
  AND jsonb_typeof(h.wire#>'{employee,records}')='array' AND jsonb_typeof(h.wire#>'{recall,records}')='array'
  AND jsonb_typeof(h.wire#>'{spec,context,memory_context}')='array'
  AND jsonb_array_length(h.records) BETWEEN 1 AND 8
  AND jsonb_array_length(h.records)=(SELECT count(*) FROM uses u WHERE u.run_id=h.run_id)
  AND jsonb_array_length(h.records)=(SELECT count(DISTINCT u.fact_id) FROM uses u WHERE u.run_id=h.run_id)
  AND jsonb_array_length(h.records)=(SELECT count(DISTINCT u.ordinal) FROM uses u WHERE u.run_id=h.run_id)
  AND jsonb_array_length(h.records)+jsonb_array_length(h.scratch)<=8
  AND jsonb_array_length(h.rendered)=jsonb_array_length(h.records)+jsonb_array_length(h.scratch)
  AND EXISTS(SELECT 1 FROM uses u WHERE u.run_id=h.run_id AND u.scope='employee')
  AND (SELECT count(DISTINCT u.destination_channel_id) FROM uses u WHERE u.run_id=h.run_id AND u.scope='employee')=1
  AND coalesce((SELECT sum(octet_length(u.content)) FROM uses u WHERE u.run_id=h.run_id),0)<=8192
  AND h.origin->>'format'='ortak-reviewed-employee-run-origin/1'
  AND h.origin->>'company_id'=h.company_id::text AND h.origin->>'employee_id'=h.employee_id
  AND h.origin->>'destination_channel_id'=h.destination_channel_id::text
  AND h.wire#>>'{employee,origin}'=ortak_conversation_json75(h.origin)
  AND h.origin-ARRAY['format','company_id','employee_id','destination_channel_id','requester_public_key',
   'source_authority_epoch','destination_authority_epoch','source']='{}'::jsonb
  AND h.origin->>'requester_public_key' ~ '^[0-9a-f]{64}$'
  AND h.origin#>>'{source,author_public_key}'=h.origin->>'requester_public_key'
  AND h.origin#>>'{source,evidence_hash}' ~ '^[0-9a-f]{64}$'
  AND (h.origin->'source')-ARRAY['community_id','channel_id','event_id','event_created_at','author_public_key','evidence_hash']='{}'::jsonb
  AND h.origin#>>'{source,event_created_at}'=ortak_employee_memory_timestamp((h.origin#>>'{source,event_created_at}')::timestamptz)
  AND isfinite((h.origin#>>'{source,event_created_at}')::timestamptz)
  AND EXISTS(SELECT 1 FROM employee_memory_channel_authorities a WHERE a.company_id=h.company_id
   AND a.community_id::text=h.origin#>>'{source,community_id}' AND a.employee_id=h.employee_id
   AND a.channel_id::text=h.origin#>>'{source,channel_id}'
   AND (h.origin->>'source_authority_epoch')::bigint BETWEEN 0 AND a.epoch)
  AND EXISTS(SELECT 1 FROM employee_memory_channel_authorities a WHERE a.company_id=h.company_id
   AND a.community_id::text=h.origin#>>'{source,community_id}' AND a.employee_id=h.employee_id
   AND a.channel_id=h.destination_channel_id AND (h.origin->>'destination_authority_epoch')::bigint BETWEEN 0 AND a.epoch)
  AND h.wire->'input_truncated'='false'::jsonb
  AND ((h.work_item_id IS NULL AND h.origin_type='human' AND h.origin_id=h.origin->>'requester_public_key'
   AND h.decision_message=h.message_id AND h.decision_root=h.root_message_id
   AND h.origin#>>'{source,event_id}'=encode(h.message_id,'hex')
   AND h.wire->>'message_id'=encode(h.message_id,'hex') AND h.wire->>'root_message_id'=encode(h.root_message_id,'hex')
   AND h.wire->>'routing_decision_id'=h.routing_decision_id::text AND NOT h.wire ? 'work_origin'
   AND h.wire#>>'{spec,context,conversation_ref}'=h.destination_channel_id::text
   AND h.wire#>>'{spec,context,reply_to_message_id}'=encode(h.message_id,'hex')
   AND h.wire#>'{spec,context,work_item_id}'='null'::jsonb AND h.wire->'event_kind' IN('9'::jsonb,'40002'::jsonb))
  OR (h.work_item_id IS NOT NULL AND h.work_run=h.run_id AND h.execution_work=h.work_item_id
   AND h.execution_employee=h.employee_id AND h.execution_revision=h.employee_revision_id
   AND h.requested_by=h.origin->>'requester_public_key' AND h.origin#>>'{source,event_id}'=encode(h.source_message_id,'hex')
   AND NOT h.wire ? 'message_id' AND NOT h.wire ? 'root_message_id' AND NOT h.wire ? 'routing_decision_id'
   AND h.wire->'event_kind'='0'::jsonb AND h.definition_hash=sha256(h.definition_bytes)
   AND h.wire->'work_origin'=jsonb_build_object('run_id',h.run_id,'work_item_id',h.work_item_id,
    'project_id',h.project_id,'execution_version',h.execution_version,'definition_hash',encode(h.definition_hash,'hex'))
   AND h.wire#>'{spec,input}'=ortak_snapshot_scratch_jsonb(to_json(convert_from(h.definition_bytes,'UTF8')))
   AND h.wire#>>'{spec,context,work_item_id}'=h.work_item_id::text
   AND h.wire#>'{spec,context,conversation_ref}'='null'::jsonb
   AND h.wire#>'{spec,context,reply_to_message_id}'='null'::jsonb)),false)
 UNION ALL
 SELECT h.run_id FROM history h CROSS JOIN LATERAL jsonb_array_elements(h.records) WITH ORDINALITY selected(wrapped,n)
 LEFT JOIN uses u ON u.run_id=h.run_id AND u.ordinal=selected.n-1
 CROSS JOIN LATERAL (SELECT selected.wrapped->'record' AS value) rec
 WHERE NOT coalesce(u.fact_id IS NOT NULL
  AND selected.wrapped=jsonb_build_object('scope',u.scope,'record',rec.value)
  AND rec.value=ortak_snapshot_scratch_jsonb((jsonb_build_object('pin',u.pin||jsonb_build_object(
   'expires_at',rec.value#>>'{pin,expires_at}'),'content',u.content)
   ||CASE WHEN u.scope IN('employee','conversation') THEN jsonb_build_object('provenance',convert_from(u.provenance_bytes,'UTF8'))
    ELSE '{}'::jsonb END)::json)
  AND (rec.value#>>'{pin,expires_at}')::timestamptz=u.expires_at
  AND h.wire->'memory_binding'=ortak_snapshot_scratch_jsonb(u.binding::json)
  AND (u.scope<>'employee' OR (u.destination_channel_id=h.destination_channel_id
   AND NOT EXISTS(SELECT 1 FROM employee_reviewed_memory_facts f WHERE f.company_id=u.company_id AND f.id=u.fact_id
    AND f.kind='relationship' AND encode(f.human_public_key,'hex')<>h.origin->>'requester_public_key')
   AND NOT EXISTS(SELECT 1 FROM uses prior WHERE prior.run_id=u.run_id AND prior.ordinal<u.ordinal
    AND (prior.scope<>'employee' OR prior.priority>u.priority OR prior.priority=u.priority AND prior.fact_id>=u.fact_id))))
  AND (u.scope<>'project' OR h.work_item_id IS NOT NULL AND u.project_id=h.project_id)
  AND octet_length(h.rendered->>(selected.n::integer-1))<=8192
  AND ortak_snapshot_scratch_jsonb((h.rendered->>(selected.n::integer-1))::json)=jsonb_build_object(
   'type','reviewed_'||CASE u.scope WHEN 'employee' THEN 'employee' WHEN 'conversation' THEN 'conversation' ELSE 'project' END||'_memory',
   'trust','untrusted_data','record',rec.value),false)
 UNION ALL
 SELECT h.run_id FROM history h CROSS JOIN LATERAL jsonb_array_elements(h.scratch) WITH ORDINALITY selected(record,n)
 WHERE NOT coalesce(jsonb_typeof(selected.record->'content')='string'
  AND selected.record#>>'{scope,scope}'='run_scratch' AND selected.record#>>'{scope,run_id}'=h.run_id::text
  AND selected.record#>>'{provenance,employee_id}'=h.employee_id AND selected.record#>>'{provenance,run_id}'=h.run_id::text
  AND octet_length(selected.record->>'record_ref') BETWEEN 1 AND 256
  AND jsonb_typeof(selected.record->'record_ref')='string'
  AND selected.record->>'record_ref' !~ U&'[\0001-\001F\007F-\009F]'
  AND octet_length(selected.record#>>'{provenance,source}') BETWEEN 1 AND 128
  AND jsonb_typeof(selected.record#>'{provenance,source}')='string'
  AND selected.record#>>'{provenance,source}' !~ U&'[\0001-\001F\007F-\009F]'
  AND isfinite((selected.record#>>'{provenance,recorded_at}')::timestamptz)
  AND selected.record#>>'{provenance,recorded_at}' ~ '^[+-]?[0-9]{4,6}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}([.][0-9]{1,9})?Z$'
  AND btrim(selected.record->>'content',U&'\0009\000A\000B\000C\000D\0020\0085\00A0\1680\2000\2001\2002\2003\2004\2005\2006\2007\2008\2009\200A\2028\2029\202F\205F\3000')<>''
  AND octet_length(selected.record->>'content')-(octet_length(selected.record->>'content')
   -octet_length(regexp_replace(selected.record->>'content',E'\x01[\x01\x02]','','g')))/2<=4096
  AND (SELECT count(*) FROM jsonb_array_elements(h.scratch) x WHERE x->>'record_ref'=selected.record->>'record_ref')=1
  AND octet_length(h.rendered->>(jsonb_array_length(h.records)+selected.n::integer-1))<=8192
  AND ortak_snapshot_scratch_jsonb((h.rendered->>(jsonb_array_length(h.records)+selected.n::integer-1))::json)
   =jsonb_build_object('type','run_scratch_memory','trust','untrusted_data','record',selected.record),false)
 UNION ALL
 SELECT h.run_id FROM history h WHERE coalesce((SELECT sum(octet_length(u.content)) FROM uses u WHERE u.run_id=h.run_id),0)
  +coalesce((SELECT sum(octet_length(x->>'content')-(octet_length(x->>'content')
   -octet_length(regexp_replace(x->>'content',E'\x01[\x01\x02]','','g')))/2) FROM jsonb_array_elements(h.scratch) x),0)>16384
) SELECT count(*) FROM bad""")

HISTORY['invalid_employee_mixed_origin77'] = _sql("""
WITH snapshots AS MATERIALIZED (
 SELECT s.*,ortak_snapshot_scratch_jsonb(convert_from(s.spec_bytes,'UTF8')::json) AS wire
 FROM run_context_snapshots s WHERE s.company_id='@COMPANY@'
), selected AS (
 SELECT s.*,s.wire#>'{employee,conversation_origin}' AS origin,
  (s.wire#>>'{employee,conversation_origin,provenance}')::jsonb AS provenance,
  (s.wire#>>'{employee,origin}')::jsonb AS employee_origin
 FROM snapshots s WHERE s.wire->'version'='5'::jsonb
)
SELECT count(*) FROM selected s WHERE
 (NOT EXISTS(SELECT 1 FROM run_reviewed_memory_uses u JOIN reviewed_memory_facts f
  ON f.company_id=u.company_id AND f.id=u.fact_id WHERE u.company_id=s.company_id AND u.run_id=s.run_id
  AND f.audience_kind='conversation') AND (s.wire->'employee') ? 'conversation_origin')
 OR EXISTS(SELECT 1 FROM run_reviewed_memory_uses u JOIN reviewed_memory_facts f
  ON f.company_id=u.company_id AND f.id=u.fact_id
  LEFT JOIN reviewed_memory_conversation_audiences a ON a.company_id=f.company_id AND a.fact_id=f.id
  WHERE u.company_id=s.company_id AND u.run_id=s.run_id AND f.audience_kind='conversation'
  AND NOT coalesce(s.origin-'requester_public_key'-'provenance'='{}'::jsonb
   AND s.origin->>'requester_public_key'=s.employee_origin->>'requester_public_key'
   AND s.origin->>'provenance'=ortak_conversation_json75(s.provenance)
   AND s.provenance-ARRAY['audience','audience_hash','format','source_event_created_at','source_event_id',
    'source_evidence_hash','source_hash']='{}'::jsonb
   AND s.provenance->>'format'='ortak-reviewed-conversation-provenance/1'
   AND s.provenance->>'source_event_id'=s.employee_origin#>>'{source,event_id}'
   AND s.provenance->>'source_event_created_at'=s.employee_origin#>>'{source,event_created_at}'
   AND s.provenance->>'source_evidence_hash' ~ '^[0-9a-f]{64}$'
   AND s.provenance->>'audience_hash'=encode(sha256(convert_to(ortak_conversation_json75(s.provenance->'audience'),'UTF8')),'hex')
   AND s.provenance->>'source_hash'=encode(sha256(convert_to(ortak_conversation_json75(jsonb_build_object(
    'audience_hash',s.provenance->>'audience_hash','format','ortak-reviewed-conversation-source/1',
    'source_evidence_hash',s.provenance->>'source_evidence_hash')),'UTF8')),'hex')
   AND s.provenance#>>'{audience,format}'='ortak-reviewed-conversation-audience/1'
   AND (s.provenance->'audience')-ARRAY['channel_id','community_id','company_id','employee_id','format',
    'kind','project_id','thread_root_event_created_at','thread_root_event_id']='{}'::jsonb
   AND s.provenance#>>'{audience,company_id}'=s.company_id::text
   AND s.provenance#>>'{audience,employee_id}'=s.employee_origin->>'employee_id'
   AND s.provenance#>>'{audience,kind}'='thread'
   AND s.provenance#>>'{audience,project_id}'=f.project_id::text
   AND s.provenance#>>'{audience,community_id}'=a.community_id::text
   AND s.provenance#>>'{audience,channel_id}'=a.channel_id::text
   AND s.provenance#>>'{audience,thread_root_event_id}' ~ '^[0-9a-f]{64}$'
   AND isfinite((s.provenance#>>'{audience,thread_root_event_created_at}')::timestamptz)
   AND (a.kind='channel' OR (s.provenance#>>'{audience,thread_root_event_id}'=encode(a.thread_root_event_id,'hex')
    AND (s.provenance#>>'{audience,thread_root_event_created_at}')::timestamptz=a.thread_root_event_created_at)),false))""")


def guards(company):
    """Limit all protected byte aggregates before any decoding in the snapshot."""
    return ("DO $$BEGIN IF (SELECT coalesce(sum(octet_length(identity_bytes)+octet_length(source_bytes)"
            "+octet_length(wrapped_key)),0) FROM confidential_runs WHERE company_id='" + company + "')"
            "+(SELECT coalesce(sum(octet_length(envelope_bytes)),0) FROM confidential_run_payloads WHERE company_id='"
            + company + "')+(SELECT coalesce(sum(octet_length(recipient_bytes)+octet_length(history_bytes)),0)"
            " FROM confidential_reply_bundles WHERE company_id='" + company + "')>8388608"
            " THEN RAISE EXCEPTION 'protected recovery byte bound'; END IF; END$$; ")


HONCHO_HISTORY = {
    'employee_content_lifecycle': """SELECT count(*) FROM ortak_employee_reviewed_content c
LEFT JOIN ortak_employee_reviewed_records r USING(workspace_id,employee_id,record_id)
LEFT JOIN ortak_employee_reviewed_tombstones t USING(workspace_id,employee_id,record_id)
WHERE r.record_id IS NULL OR t.record_id IS NOT NULL
 OR r.content_hash<>encode(sha256(convert_to(c.content,'UTF8')),'hex')""",
    'employee_header_lifecycle': """SELECT count(*) FROM ortak_employee_reviewed_records r
LEFT JOIN ortak_employee_reviewed_content c USING(workspace_id,employee_id,record_id)
LEFT JOIN ortak_employee_reviewed_tombstones t USING(workspace_id,employee_id,record_id)
WHERE (c.record_id IS NULL AND t.record_id IS NULL)
 OR r.publish_key<>'employee-reviewed:publish:'||r.company_id||':'||r.record_id
 OR NOT EXISTS(SELECT 1 FROM ortak_employee_reviewed_operations o WHERE o.workspace_id=r.workspace_id
  AND o.employee_id=r.employee_id AND o.record_id=r.record_id AND o.action='publish'
  AND o.idempotency_key=r.publish_key AND o.request_hash=r.request_hash AND o.body_hash=r.body_hash)""",
    'employee_tombstone_lifecycle': """SELECT count(*) FROM ortak_employee_reviewed_tombstones t
LEFT JOIN ortak_employee_reviewed_records r USING(workspace_id,employee_id,record_id)
WHERE t.withdraw_key<>'employee-reviewed:withdraw:'||t.company_id||':'||t.record_id
 OR NOT EXISTS(SELECT 1 FROM ortak_employee_reviewed_operations o WHERE o.workspace_id=t.workspace_id
  AND o.employee_id=t.employee_id AND o.record_id=t.record_id AND o.action='withdraw'
  AND o.idempotency_key=t.withdraw_key AND o.request_hash=t.request_hash AND o.body_hash=t.body_hash)
 OR (r.record_id IS NOT NULL AND (t.company_id,t.deployment_id,t.namespace_hash,t.binding_hash,t.ownership,
  t.target_id,t.destination_channel_id,t.content_hash,t.source_hash,t.sharing_hash) IS DISTINCT FROM
 (r.company_id,r.deployment_id,r.namespace_hash,r.binding_hash,r.ownership,
  r.target_id,r.destination_channel_id,r.content_hash,r.source_hash,r.sharing_hash))""",
    'employee_operation_lifecycle': """SELECT count(*) FROM ortak_employee_reviewed_operations o WHERE
 (o.action='publish' AND NOT EXISTS(SELECT 1 FROM ortak_employee_reviewed_records r
  WHERE r.workspace_id=o.workspace_id AND r.employee_id=o.employee_id AND r.record_id=o.record_id))
 OR (o.action='withdraw' AND NOT EXISTS(SELECT 1 FROM ortak_employee_reviewed_tombstones t
  WHERE t.workspace_id=o.workspace_id AND t.employee_id=o.employee_id AND t.record_id=o.record_id))""",
    'employee_diagnostic_cleanup': """SELECT
 (SELECT count(*) FROM ortak_employee_diagnostic_content)
 +(SELECT count(*) FROM ortak_employee_diagnostics d
 LEFT JOIN ortak_employee_diagnostic_tombstones t USING(workspace_id,employee_id,operation_id)
 WHERE t.operation_id IS NULL OR (d.company_id,d.deployment_id,d.namespace_hash,d.binding_hash,d.ownership,
  d.employee_revision_id,d.employee_lifecycle_epoch,d.challenge_hash) IS DISTINCT FROM
 (t.company_id,t.deployment_id,t.namespace_hash,t.binding_hash,t.ownership,
  t.employee_revision_id,t.employee_lifecycle_epoch,t.challenge_hash))""",
}


def honcho_query(legacy):
    """Exact family, complete row hashes and finite diagnostic cleanup in one snapshot."""
    counts = '+'.join('(SELECT count(*) FROM ' + table + ')' for table in HONCHO_KEYS)
    rows = []
    for table, keys in HONCHO_KEYS.items():
        rows.append("'" + table + "',coalesce((SELECT jsonb_agg(jsonb_build_object('key',jsonb_build_array("
            + ','.join('t.' + key for key in keys)
            + "),'row_sha256',encode(sha256(convert_to(to_jsonb(t)::text,'UTF8')),'hex')) ORDER BY "
            + ','.join('t.' + key for key in keys) + ') FROM ' + table + " t),'[]'::jsonb)")
    checks = ','.join("'" + key + "',(" + sql + ')' for key, sql in (legacy | HONCHO_HISTORY).items())
    return ('BEGIN ISOLATION LEVEL REPEATABLE READ READ ONLY; DO $$BEGIN IF ' + counts
        + ">1024 THEN RAISE EXCEPTION 'employee Honcho recovery bound'; END IF; END$$; "
        + "SELECT jsonb_build_object('counters',jsonb_build_object(" + checks
        + "),'tables',jsonb_build_object(" + ','.join(rows) + ')); ROLLBACK;')


def validate_honcho(value, legacy):
    """No old/partial/extra group or uncontained diagnostic can become a receipt."""
    if not isinstance(value, dict) or set(value) != {'counters', 'tables'}:
        raise Refused('employee_honcho_recovery_proof_refused')
    if (set(value['counters']) != set(legacy) | set(HONCHO_HISTORY)
            or any(type(v) is not int or v != 0 for v in value['counters'].values())
            or set(value['tables']) != set(HONCHO_KEYS)):
        raise Refused('employee_honcho_recovery_history_inconsistent')
    total = 0
    for name, keys in HONCHO_KEYS.items():
        rows = value['tables'][name]
        if not isinstance(rows, list):
            raise Refused('employee_honcho_recovery_rows_refused')
        total += len(rows)
        seen = set()
        for row in rows:
            if (not isinstance(row, dict) or set(row) != {'key', 'row_sha256'}
                    or not isinstance(row['key'], list) or len(row['key']) != len(keys)
                    or any(not isinstance(x, str) or not 1 <= len(x.encode()) <= 256 for x in row['key'])
                    or not isinstance(row['row_sha256'], str) or not re.fullmatch('[0-9a-f]{64}', row['row_sha256'])):
                raise Refused('employee_honcho_recovery_rows_refused')
            key = tuple(row['key'])
            if key in seen:
                raise Refused('employee_honcho_recovery_duplicate_key')
            seen.add(key)
    if total > 1024:
        raise Refused('employee_honcho_recovery_bound')
    return value['tables']


def legacy_invariants():
    """Keep the reviewed76 proof exact for versions1..4; v5 has its own proof."""
    result = conversations.invariants(76)
    marker = 'FROM parsed s LEFT JOIN runs'
    key = 'invalid_conversation_snapshot_history76'
    if key not in result or result[key].count(marker) != 1:
        raise Refused('recovery77_legacy_query_seam_changed')
    result[key] = result[key].replace(marker,
        "FROM (SELECT * FROM parsed WHERE wire->'version' IS DISTINCT FROM '5'::jsonb) s LEFT JOIN runs")
    return result


def validate(evidence, counters):
    """Structural history is mandatory even when an offline withdrawal became due."""
    if any(counters[name] != 0 for name in HISTORY):
        raise Refused('recovery77_history_inconsistent')
    scopes = evidence['tables']['employee_memory_channel_authorities']
    if len(scopes) > 128 or len(evidence['tables']['encrypted_dm_selections']) > 128:
        raise Refused('employee_recovery_scope_bound')
    for name, fields in TABLE_KEYS.items():
        for row in evidence['tables'][name]:
            for field, value in zip(fields, row['key']):
                if field in ('ordinal', 'copy'):
                    ceiling = 1 if field == 'copy' else (7 if name.startswith('run_') else 512)
                    if type(value) is not int or not 0 <= value <= ceiling:
                        raise Refused('recovery77_key_refused')
                elif field in ('source_id', 'actor_public_key'):
                    if not isinstance(value, str) or not re.fullmatch(r'\\x[0-9a-f]{64}', value):
                        raise Refused('recovery77_key_refused')
                elif field in ('employee_id', 'actor_pubkey', 'action', 'purpose'):
                    if not isinstance(value, str) or not 1 <= len(value.encode()) <= 256:
                        raise Refused('recovery77_key_refused')
                elif not isinstance(value, str) or not re.fullmatch(r'[0-9a-f]{8}(-[0-9a-f]{4}){3}-[0-9a-f]{12}', value):
                    raise Refused('recovery77_key_refused')
