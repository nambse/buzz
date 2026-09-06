"""Reviewed75/76 retained conversation evidence, without current source authority.

76 shares75 storage but explicitly admits historical exports and typed v4 uses.
Nothing calls a provider, mutates an epoch, renews a target, or acknowledges
remote cleanup. Existing75 restrictions remain a separate exact branch.
"""

import re

from backup_private_database import Refused

TABLE_KEYS = {
    'conversation_memory_authorities': ('company_id', 'project_id', 'channel_id'),
    'reviewed_memory_conversation_audiences': ('company_id', 'fact_id'),
}
ACTIVATION_GATES = ['conversation_current_source_and_epochs_revalidated']
RECOVERY_CONTRACT = {
    'storage_version': 75,
    'runtime_publication': 'not_admitted_by_schema75',
    'historical_epochs': 'preserve_older_pins_without_renewal',
    'permission_loss_cleanup': 'none; only_explicit_stop_or_expiry_uses_retained_withdrawal_keys',
}

# These checks are time-independent. They compare immutable identities/bytes or
# monotonic bounds, never current membership, source visibility or target TTL.
INVARIANTS = {
    'conversation_scope_bound_exceeded': """SELECT CASE WHEN
        (SELECT count(*) FROM conversation_memory_authorities WHERE company_id='{company}')>128
        OR EXISTS(SELECT 1 FROM conversation_memory_authorities a WHERE a.company_id='{company}'
            AND (SELECT count(*) FROM conversation_memory_authorities other
                 WHERE other.community_id=a.community_id)>256)
        THEN 1 ELSE 0 END""",
    'invalid_conversation_authority_history': """SELECT count(*) FROM conversation_memory_authorities a
        WHERE a.company_id='{company}' AND (a.epoch<0 OR a.changed_at<a.created_at
            OR ((a.epoch=0) IS DISTINCT FROM (a.last_change_reason='registered'))
            OR a.last_change_reason NOT IN ('registered','channel_changed','membership_changed',
                'project_changed','project_grant_changed','event_changed','thread_changed','identity_changed','scope_closed')
            OR NOT EXISTS(SELECT 1 FROM projects p JOIN companies co ON co.id=p.company_id
                JOIN communities cm ON cm.id=a.community_id
                WHERE p.company_id=a.company_id AND p.id=a.project_id))""",
    'invalid_conversation_fact_audience_pairs': """SELECT count(*) FROM reviewed_memory_facts f
        WHERE f.company_id='{company}' AND (f.audience_kind NOT IN ('project','conversation')
            OR ((f.audience_kind='conversation') IS DISTINCT FROM EXISTS(
                SELECT 1 FROM reviewed_memory_conversation_audiences a
                WHERE a.company_id=f.company_id AND a.fact_id=f.id)))""",
    'invalid_conversation_audience_parents': """SELECT count(*) FROM reviewed_memory_conversation_audiences a
        WHERE a.company_id='{company}' AND NOT EXISTS(
            SELECT 1 FROM reviewed_memory_facts f JOIN projects p
                ON p.company_id=f.company_id AND p.id=f.project_id
            JOIN employees e ON e.company_id=f.company_id AND e.id=f.employee_id
            JOIN conversation_memory_authorities scope ON scope.company_id=a.company_id
                AND scope.community_id=a.community_id AND scope.project_id=a.project_id AND scope.channel_id=a.channel_id
            JOIN reviewed_memory_operations receipt ON receipt.company_id=f.company_id
                AND receipt.actor_pubkey=f.approved_by AND receipt.operation_id=f.promotion_operation_id
            WHERE f.company_id=a.company_id AND f.id=a.fact_id AND f.community_id=a.community_id
                AND f.project_id=a.project_id AND f.employee_id=a.employee_id AND f.audience_kind='conversation'
                AND f.source_message_id=a.source_event_id AND f.source_artifact_id IS NULL
                AND receipt.community_id=a.community_id AND receipt.project_id=a.project_id
                AND receipt.fact_id=a.fact_id AND receipt.action='promote' AND receipt.result_version=1)""",
    'invalid_conversation_audience_bytes': """SELECT count(*) FROM reviewed_memory_conversation_audiences a
        CROSS JOIN LATERAL (SELECT jsonb_build_object(
            'channel_id',a.channel_id,'community_id',a.community_id,'company_id',a.company_id,
            'employee_id',a.employee_id,'format','ortak-reviewed-conversation-audience/1',
            'kind',a.kind,'project_id',a.project_id,
            'thread_root_event_created_at',to_char(a.thread_root_event_created_at AT TIME ZONE 'UTC','YYYY-MM-DD"T"HH24:MI:SS.US"Z"'),
            'thread_root_event_id',encode(a.thread_root_event_id,'hex')) AS audience) wire
        WHERE a.company_id='{company}' AND (a.kind NOT IN ('channel','thread')
            OR NOT ((a.kind='channel' AND a.thread_root_event_id IS NULL AND a.thread_root_event_created_at IS NULL)
                OR (a.kind='thread' AND a.thread_root_event_id IS NOT NULL AND a.thread_root_event_created_at IS NOT NULL))
            OR octet_length(a.source_event_id)<>32 OR octet_length(a.thread_root_event_id)<>32
            OR a.source_event_created_at<TIMESTAMPTZ '1970-01-01 00:00:00+00'
            OR a.source_event_created_at>=TIMESTAMPTZ '10000-01-01 00:00:00+00'
            OR a.thread_root_event_created_at<TIMESTAMPTZ '1970-01-01 00:00:00+00'
            OR a.thread_root_event_created_at>=TIMESTAMPTZ '10000-01-01 00:00:00+00'
            OR (a.source_event_id=a.thread_root_event_id AND a.source_event_created_at<>a.thread_root_event_created_at)
            OR octet_length(a.audience_bytes) NOT BETWEEN 1 AND 2048
            OR octet_length(a.provenance_bytes) NOT BETWEEN 1 AND 4096
            OR octet_length(a.audience_hash)<>32 OR octet_length(a.source_evidence_hash)<>32 OR octet_length(a.source_hash)<>32
            OR a.audience_bytes IS DISTINCT FROM convert_to(ortak_conversation_json75(wire.audience),'UTF8')
            OR a.audience_hash IS DISTINCT FROM sha256(a.audience_bytes)
            OR a.source_hash IS DISTINCT FROM sha256(convert_to(ortak_conversation_json75(jsonb_build_object(
                'audience_hash',encode(a.audience_hash,'hex'),'format','ortak-reviewed-conversation-source/1',
                'source_evidence_hash',encode(a.source_evidence_hash,'hex'))),'UTF8'))
            OR a.provenance_bytes IS DISTINCT FROM convert_to(ortak_conversation_json75(jsonb_build_object(
                'audience',wire.audience,'audience_hash',encode(a.audience_hash,'hex'),
                'format','ortak-reviewed-conversation-provenance/1',
                'source_event_created_at',to_char(a.source_event_created_at AT TIME ZONE 'UTC','YYYY-MM-DD"T"HH24:MI:SS.US"Z"'),
                'source_event_id',encode(a.source_event_id,'hex'),'source_evidence_hash',encode(a.source_evidence_hash,'hex'),
                'source_hash',encode(a.source_hash,'hex'))),'UTF8'))""",
    'invalid_conversation_target_pins': """SELECT count(*) FROM reviewed_memory_targets t
        WHERE t.company_id='{company}' AND (t.conversation_consumption_epoch<0
            OR (t.conversation_channel_id IS NULL AND (t.conversation_consumption_enabled OR t.conversation_consumption_epoch<>0))
            OR (t.conversation_channel_id IS NOT NULL AND NOT EXISTS(
                SELECT 1 FROM conversation_memory_authorities a WHERE a.company_id=t.company_id
                    AND a.community_id=t.community_id AND a.project_id=t.project_id AND a.channel_id=t.conversation_channel_id)))""",
    'invalid_conversation_use_history': """SELECT count(*) FROM run_reviewed_memory_uses u
        WHERE u.company_id='{company}' AND NOT EXISTS(
            SELECT 1 FROM reviewed_memory_facts f JOIN reviewed_memory_exports x
                ON x.company_id=f.company_id AND x.fact_id=f.id
            JOIN reviewed_memory_targets t ON t.company_id=x.company_id AND t.id=x.target_id
            JOIN runs r ON r.company_id=u.company_id AND r.id=u.run_id
            JOIN run_context_snapshots snapshot ON snapshot.company_id=u.company_id AND snapshot.run_id=u.run_id
            LEFT JOIN reviewed_memory_conversation_audiences a ON a.company_id=f.company_id AND a.fact_id=f.id
            LEFT JOIN conversation_memory_authorities scope ON scope.company_id=a.company_id
                AND scope.community_id=a.community_id AND scope.project_id=a.project_id AND scope.channel_id=a.channel_id
            WHERE f.company_id=u.company_id AND f.id=u.fact_id AND f.community_id=u.community_id
                AND r.employee_id=f.employee_id AND u.ordinal BETWEEN 0 AND 7 AND u.fact_version=1
                AND t.id=u.target_id AND t.community_id=u.community_id AND t.project_id=f.project_id AND t.employee_id=f.employee_id
                AND x.community_id=u.community_id AND x.content_hash=u.content_hash
                AND x.source_hash=u.source_hash AND t.binding_hash=u.binding_hash
                AND u.content_hash=sha256(convert_to(f.content,'UTF8'))
                AND u.approval_id=f.promotion_operation_id AND u.approved_by=f.approved_by AND u.expires_at=f.expires_at
                AND ((f.audience_kind='project' AND u.conversation_audience_hash IS NULL
                    AND u.conversation_authority_epoch IS NULL AND u.conversation_consumption_epoch IS NULL
                    AND u.consumption_epoch>=0 AND u.consumption_epoch<=t.consumption_epoch)
                  OR (f.audience_kind='conversation' AND u.consumption_epoch=0
                    AND u.conversation_audience_hash=a.audience_hash AND u.source_hash=a.source_hash
                    AND t.conversation_channel_id=a.channel_id
                    AND u.conversation_authority_epoch>=0 AND u.conversation_authority_epoch<=scope.epoch
                    AND u.conversation_consumption_epoch>=0 AND u.conversation_consumption_epoch<=t.conversation_consumption_epoch)))""",
    'unsupported_conversation_execution75': """SELECT
        (SELECT count(*) FROM reviewed_memory_exports x JOIN reviewed_memory_facts f
            ON f.company_id=x.company_id AND f.id=x.fact_id
            WHERE x.company_id='{company}' AND f.audience_kind='conversation')
        + (SELECT count(*) FROM run_reviewed_memory_uses u LEFT JOIN reviewed_memory_facts f
            ON f.company_id=u.company_id AND f.id=u.fact_id
            WHERE u.company_id='{company}' AND (f.audience_kind='conversation'
                OR u.conversation_audience_hash IS NOT NULL OR u.conversation_authority_epoch IS NOT NULL
                OR u.conversation_consumption_epoch IS NOT NULL))""",
}


def validate_evidence(evidence, counters):
    """Require complete new scoped keys and structural invariants, even offline."""
    if any(type(counters.get(name)) is not int or counters[name] != 0
           for name in invariants(evidence['schema_version'])):
        raise Refused('conversation_recovery_history_inconsistent')
    for table in TABLE_KEYS:
        rows = evidence['tables'][table]
        if table == 'conversation_memory_authorities' and len(rows) > 128:
            raise Refused('conversation_recovery_scope_bound')
        for row in rows:
            if any(not isinstance(part, str)
                    or not re.fullmatch(r'[0-9a-f]{8}(-[0-9a-f]{4}){3}-[0-9a-f]{12}', part)
                    or part == '00000000-0000-0000-0000-000000000000' for part in row['key']):
                raise Refused('conversation_recovery_key_refused')


def recovery_contract(version):
    """Keep75 exact; archive76 can never turn historical opt-in into activation."""
    if version == 75:
        return dict(RECOVERY_CONTRACT)
    if version != 76:
        raise Refused('conversation_recovery_schema_review_required')
    return RECOVERY_CONTRACT | {
        'runtime_publication': 'schema76_retained_exports_and_v4_uses_only',
        'snapshot_version': 4,
        'snapshot_history': 'exact_bytes_and_retained_pins_without_current_source',
    }


def snapshot_guards76(company):
    """Bound all selected candidate bytes before even decoding their JSON version."""
    return ("DO $$BEGIN IF (SELECT count(*) FROM run_context_snapshots WHERE company_id='" + company
        + "')>1024 OR (SELECT coalesce(sum(octet_length(spec_bytes)),0) FROM run_context_snapshots WHERE company_id='"
        + company + "')>8388608 OR EXISTS(SELECT 1 FROM run_context_snapshots WHERE company_id='" + company
        + "' AND octet_length(spec_bytes) NOT BETWEEN 1 AND 262144) THEN "
        "RAISE EXCEPTION 'conversation recovery snapshot bound'; END IF; END$$; ")


EXPORT_HISTORY76 = """SELECT count(*) FROM reviewed_memory_exports x
    LEFT JOIN reviewed_memory_facts f ON f.company_id=x.company_id AND f.id=x.fact_id
    WHERE x.company_id='{company}' AND (f.id IS NULL OR (f.audience_kind='conversation' AND NOT EXISTS(
        SELECT 1 FROM reviewed_memory_conversation_audiences a
        JOIN reviewed_memory_targets t ON t.company_id=x.company_id AND t.id=x.target_id
        JOIN employee_revisions revision ON revision.company_id=x.company_id
            AND revision.employee_id=x.employee_id AND revision.id=x.employee_revision_id
        JOIN reviewed_memory_export_commands command ON command.company_id=x.company_id
            AND command.actor_pubkey=x.requested_by AND command.operation_id=x.operation_id
        WHERE a.company_id=f.company_id AND a.fact_id=f.id AND x.community_id=f.community_id
            AND a.community_id=x.community_id AND x.project_id=f.project_id AND x.employee_id=f.employee_id
            AND t.community_id=x.community_id AND t.project_id=x.project_id AND t.employee_id=x.employee_id
            AND t.conversation_channel_id=a.channel_id AND x.content_hash=sha256(convert_to(f.content,'UTF8'))
            AND x.source_hash=a.source_hash AND command.community_id=x.community_id
            AND command.fact_id=x.fact_id AND command.action='publish' AND command.result_version=0)))"""


# Only the canonical JSON72/75 byte functions are called. Admission helpers76
# intentionally cannot be reused: they require active source/ACL/expiry and
# would reject valid stopped/retired history or renew runtime authority.
SNAPSHOT_HISTORY76 = r"""WITH parsed AS MATERIALIZED (
    SELECT s.*,ortak_snapshot_scratch_jsonb(convert_from(s.spec_bytes,'UTF8')::json) AS wire
    FROM run_context_snapshots s WHERE s.company_id='{company}'
), candidates AS MATERIALIZED (
    SELECT s.*,r.employee_id,r.employee_revision_id,r.work_item_id,r.message_id,r.root_message_id,r.routing_decision_id,
        revision.manifest,work.project_id AS work_project,work.requested_by,work.definition_bytes,work.definition_hash,
        work.execution_version,work.run_id AS work_run,work.work_item_id AS execution_work_item,
        work.employee_id AS execution_employee,work.employee_revision_id AS execution_revision,
        decision.origin_type AS decision_origin_type,decision.origin_id AS decision_origin_id,
        decision.message_id AS decision_message_id,decision.root_message_id AS decision_root_message_id,
        item.source_message_id AS work_source_message_id,
        wire->'conversation' AS context
    FROM parsed s LEFT JOIN runs r ON r.company_id=s.company_id AND r.id=s.run_id
    LEFT JOIN employee_revisions revision ON revision.company_id=r.company_id
        AND revision.employee_id=r.employee_id AND revision.id=r.employee_revision_id
    LEFT JOIN work_executions work ON work.company_id=r.company_id AND work.run_id=r.id
    LEFT JOIN routing_decisions decision ON decision.company_id=r.company_id AND decision.id=r.routing_decision_id
    LEFT JOIN work_items item ON item.company_id=work.company_id AND item.project_id=work.project_id
        AND item.id=work.work_item_id
    WHERE (wire->'version' IS DISTINCT FROM '1'::jsonb AND wire->'version' IS DISTINCT FROM '2'::jsonb
        AND wire->'version' IS DISTINCT FROM '3'::jsonb) OR wire ? 'conversation'
        OR EXISTS(SELECT 1 FROM run_reviewed_memory_uses u WHERE u.company_id=s.company_id AND u.run_id=s.run_id
            AND (u.conversation_audience_hash IS NOT NULL OR u.conversation_authority_epoch IS NOT NULL
                OR u.conversation_consumption_epoch IS NOT NULL))
), history AS MATERIALIZED (
    SELECT c.*,context->'origin' AS origin,
        (context->'origin'->>'provenance')::jsonb AS provenance,
        CASE WHEN jsonb_typeof(context->'records')='array' THEN context->'records' ELSE '[]'::jsonb END AS records,
        CASE WHEN jsonb_typeof(wire->'recall'->'records')='array' THEN wire->'recall'->'records' ELSE '[]'::jsonb END AS scratch,
        CASE WHEN jsonb_typeof(wire->'spec'->'context'->'memory_context')='array'
            THEN wire->'spec'->'context'->'memory_context' ELSE '[]'::jsonb END AS rendered
    FROM candidates c
), uses AS MATERIALIZED (
    SELECT u.*,f.project_id,f.audience_kind,f.content,a.channel_id,a.kind,a.thread_root_event_id,a.thread_root_event_created_at,
        a.provenance_bytes,t.binding,
        jsonb_build_object('fact_id',u.fact_id,'target_id',u.target_id,'fact_version',u.fact_version,
            'consumption_epoch',u.consumption_epoch,'content_hash',encode(u.content_hash,'hex'),
            'source_hash',encode(u.source_hash,'hex'),'binding_hash',encode(u.binding_hash,'hex'),
            'approval_id',u.approval_id,'approved_by',u.approved_by) AS common_pin
    FROM run_reviewed_memory_uses u
    JOIN reviewed_memory_facts f ON f.company_id=u.company_id AND f.id=u.fact_id
    JOIN reviewed_memory_targets t ON t.company_id=u.company_id AND t.id=u.target_id
    LEFT JOIN reviewed_memory_conversation_audiences a ON a.company_id=u.company_id AND a.fact_id=u.fact_id
    WHERE u.company_id='{company}'
), bad AS (
    SELECT h.run_id FROM history h WHERE NOT coalesce(
        octet_length(h.spec_hash)=32 AND h.spec_hash=sha256(h.spec_bytes)
        AND h.wire->'version'='4'::jsonb AND NOT h.wire ? 'reviewed'
        AND h.wire-'version'-'company_id'-'routing_decision_id'-'message_id'-'root_message_id'-'event_kind'
            -'input_truncated'-'memory_binding'-'recall'-'spec'-'work_origin'-'conversation'='{{}}'::jsonb
        AND h.manifest IS NOT NULL AND h.wire->>'company_id'=h.company_id::text
        AND jsonb_typeof(h.wire->'spec')='object'
        AND (h.wire->'spec')-'run_id'-'employee_id'-'revision_id'-'binding'-'permissions'-'input'-'context'-'idempotency_key'='{{}}'::jsonb
        AND (h.wire->'spec'->'context')-'conversation_ref'-'reply_to_message_id'-'work_item_id'-'memory_context'='{{}}'::jsonb
        AND jsonb_typeof(h.wire->'spec'->'input')='string' AND btrim(h.wire->'spec'->>'input')<>''
        AND octet_length(h.wire->'spec'->>'input')
            -(octet_length(h.wire->'spec'->>'input')-octet_length(regexp_replace(h.wire->'spec'->>'input',E'\x01[\x01\x02]','','g')))/2<=65536
        AND h.wire->'spec'->>'run_id'=h.run_id::text AND h.wire->'spec'->>'employee_id'=h.employee_id
        AND h.wire->'spec'->>'revision_id'=h.employee_revision_id::text
        AND h.wire->'spec'->>'idempotency_key'='ortak-run:'||h.company_id::text||':'||h.run_id::text
        AND h.wire->'spec'->'binding'=ortak_snapshot_scratch_jsonb((h.manifest->'runtime')::json)
        AND h.wire->'spec'->'permissions'=ortak_snapshot_scratch_jsonb((h.manifest->'permissions')::json)
        AND h.wire->'memory_binding'=ortak_snapshot_scratch_jsonb((h.manifest->'memory')::json)
        AND jsonb_typeof(h.context)='object' AND h.context-'origin'-'records'-'truncated'='{{}}'::jsonb
        AND jsonb_typeof(h.context->'truncated')='boolean' AND jsonb_typeof(h.context->'records')='array'
        AND jsonb_typeof(h.wire->'recall'->'records')='array' AND jsonb_typeof(h.wire->'recall'->'truncated')='boolean'
        AND jsonb_typeof(h.wire->'spec'->'context'->'memory_context')='array'
        AND jsonb_array_length(h.records) BETWEEN 1 AND 8
        AND jsonb_array_length(h.records)+jsonb_array_length(h.scratch)<=8
        AND (SELECT count(DISTINCT record->>'record_ref') FROM jsonb_array_elements(h.scratch) record)
            =jsonb_array_length(h.scratch)
        AND jsonb_array_length(h.rendered)=jsonb_array_length(h.records)+jsonb_array_length(h.scratch)
        AND jsonb_array_length(h.records)=(SELECT count(*) FROM uses u WHERE u.run_id=h.run_id)
        AND (SELECT count(DISTINCT u.fact_id) FROM uses u WHERE u.run_id=h.run_id)=jsonb_array_length(h.records)
        AND (SELECT count(DISTINCT u.project_id) FROM uses u WHERE u.run_id=h.run_id)=1
        AND EXISTS(SELECT 1 FROM uses u WHERE u.run_id=h.run_id AND u.audience_kind='conversation')
        AND coalesce((SELECT sum(octet_length(u.content)) FROM uses u WHERE u.run_id=h.run_id),0)<=8192
        AND jsonb_typeof(h.origin)='object' AND h.origin-'requester_public_key'-'provenance'='{{}}'::jsonb
        AND h.origin->>'requester_public_key' ~ '^[0-9a-f]{{64}}$'
        AND jsonb_typeof(h.origin->'provenance')='string' AND octet_length(h.origin->>'provenance') BETWEEN 1 AND 4096
        AND h.origin->>'provenance'=ortak_conversation_json75(h.provenance)
        AND h.provenance-'audience'-'audience_hash'-'format'-'source_event_created_at'-'source_event_id'
            -'source_evidence_hash'-'source_hash'='{{}}'::jsonb
        AND h.provenance->>'format'='ortak-reviewed-conversation-provenance/1'
        AND h.provenance->>'source_event_id' ~ '^[0-9a-f]{{64}}$'
        AND h.provenance->>'source_evidence_hash' ~ '^[0-9a-f]{{64}}$'
        AND h.provenance->>'audience_hash'=encode(sha256(convert_to(ortak_conversation_json75(h.provenance->'audience'),'UTF8')),'hex')
        AND h.provenance->>'source_hash'=encode(sha256(convert_to(ortak_conversation_json75(jsonb_build_object(
            'audience_hash',h.provenance->>'audience_hash','format','ortak-reviewed-conversation-source/1',
            'source_evidence_hash',h.provenance->>'source_evidence_hash')),'UTF8')),'hex')
        AND h.provenance->'audience'->>'format'='ortak-reviewed-conversation-audience/1'
        AND (h.provenance->'audience')-'channel_id'-'community_id'-'company_id'-'employee_id'-'format'
            -'kind'-'project_id'-'thread_root_event_created_at'-'thread_root_event_id'='{{}}'::jsonb
        AND h.provenance->'audience'->>'company_id'=h.company_id::text
        AND h.provenance->'audience'->>'employee_id'=h.employee_id
        AND h.provenance->'audience'->>'kind'='thread'
        AND h.provenance->'audience'->>'thread_root_event_id' ~ '^[0-9a-f]{{64}}$'
        AND h.provenance->>'source_event_created_at'=to_char((h.provenance->>'source_event_created_at')::timestamptz
            AT TIME ZONE 'UTC','YYYY-MM-DD"T"HH24:MI:SS.US"Z"')
        AND h.provenance->'audience'->>'thread_root_event_created_at'=to_char(
            (h.provenance->'audience'->>'thread_root_event_created_at')::timestamptz
            AT TIME ZONE 'UTC','YYYY-MM-DD"T"HH24:MI:SS.US"Z"')
        AND (h.provenance->>'source_event_created_at')::timestamptz>=TIMESTAMPTZ '1970-01-01 00:00:00+00'
        AND (h.provenance->>'source_event_created_at')::timestamptz<TIMESTAMPTZ '10000-01-01 00:00:00+00'
        AND (h.provenance->'audience'->>'thread_root_event_created_at')::timestamptz>=TIMESTAMPTZ '1970-01-01 00:00:00+00'
        AND (h.provenance->'audience'->>'thread_root_event_created_at')::timestamptz<TIMESTAMPTZ '10000-01-01 00:00:00+00'
        AND (h.provenance->>'source_event_id'<>h.provenance->'audience'->>'thread_root_event_id'
            OR h.provenance->>'source_event_created_at'=h.provenance->'audience'->>'thread_root_event_created_at')
        AND ((h.work_item_id IS NULL AND NOT h.wire ? 'work_origin'
            AND h.decision_origin_type='human' AND h.decision_origin_id=h.origin->>'requester_public_key'
            AND h.decision_message_id=h.message_id AND h.decision_root_message_id=h.root_message_id
            AND h.wire->>'message_id'=encode(h.message_id,'hex') AND h.wire->>'root_message_id'=encode(h.root_message_id,'hex')
            AND h.wire->>'routing_decision_id'=h.routing_decision_id::text AND h.wire->'event_kind' IN('9'::jsonb,'40002'::jsonb)
            AND h.provenance->>'source_event_id'=encode(h.message_id,'hex')
            AND h.wire->'spec'->'context'->>'conversation_ref'=h.provenance->'audience'->>'channel_id'
            AND h.wire->'spec'->'context'->>'reply_to_message_id'=encode(h.message_id,'hex')
            AND h.wire->'spec'->'context'->'work_item_id'='null'::jsonb)
          OR (h.work_item_id IS NOT NULL AND h.work_run=h.run_id
            AND h.execution_work_item=h.work_item_id AND h.execution_employee=h.employee_id
            AND h.execution_revision=h.employee_revision_id
            AND h.origin->>'requester_public_key'=h.requested_by
            AND h.provenance->>'source_event_id'=encode(h.work_source_message_id,'hex')
            AND h.provenance->'audience'->>'project_id'=h.work_project::text
            AND NOT h.wire ? 'message_id' AND NOT h.wire ? 'root_message_id' AND NOT h.wire ? 'routing_decision_id'
            AND h.wire->'event_kind'='0'::jsonb
            AND h.wire->'work_origin'=jsonb_build_object('run_id',h.run_id,'work_item_id',h.work_item_id,
                'project_id',h.work_project,'execution_version',h.execution_version,'definition_hash',encode(h.definition_hash,'hex'))
            AND h.definition_hash=sha256(h.definition_bytes)
            AND h.wire->'spec'->'input'=ortak_snapshot_scratch_jsonb(to_json(convert_from(h.definition_bytes,'UTF8')))
            AND h.wire->'spec'->'context'->>'work_item_id'=h.work_item_id::text
            AND h.wire->'spec'->'context'->'reply_to_message_id'='null'::jsonb
            AND h.wire->'spec'->'context'->'conversation_ref'='null'::jsonb))
        AND h.wire->'input_truncated'='false'::jsonb
    ,false)
    UNION ALL
    SELECT h.run_id FROM history h CROSS JOIN LATERAL jsonb_array_elements(h.records) WITH ORDINALITY selected(wrapped,n)
    LEFT JOIN uses u ON u.run_id=h.run_id AND u.ordinal=selected.n-1
    CROSS JOIN LATERAL (SELECT selected.wrapped->'record' AS record) rec
    CROSS JOIN LATERAL (SELECT u.common_pin||jsonb_build_object('expires_at',rec.record->'pin'->>'expires_at')
        || CASE WHEN u.audience_kind='conversation' THEN jsonb_build_object(
            'conversation_audience_hash',encode(u.conversation_audience_hash,'hex'),
            'conversation_authority_epoch',u.conversation_authority_epoch,
            'conversation_consumption_epoch',u.conversation_consumption_epoch) ELSE '{{}}'::jsonb END AS pin) expected
    WHERE NOT coalesce(u.run_id IS NOT NULL
        AND u.project_id::text=h.provenance->'audience'->>'project_id'
        AND selected.wrapped=jsonb_build_object('scope',u.audience_kind,'record',rec.record)
        AND rec.record=ortak_snapshot_scratch_jsonb((jsonb_build_object('pin',expected.pin,'content',u.content)
            || CASE WHEN u.audience_kind='conversation' THEN jsonb_build_object('provenance',convert_from(u.provenance_bytes,'UTF8'))
                ELSE '{{}}'::jsonb END)::json)
        AND (rec.record->'pin'->>'expires_at')::timestamptz=u.expires_at
        AND h.wire->'memory_binding'=ortak_snapshot_scratch_jsonb(u.binding::json)
        AND (u.audience_kind='conversation'
            AND u.community_id::text=h.provenance->'audience'->>'community_id'
            AND u.channel_id::text=h.provenance->'audience'->>'channel_id'
            AND (u.kind='channel' OR (encode(u.thread_root_event_id,'hex')=h.provenance->'audience'->>'thread_root_event_id'
                AND u.thread_root_event_created_at=(h.provenance->'audience'->>'thread_root_event_created_at')::timestamptz))
          OR u.audience_kind='project' AND h.work_item_id IS NOT NULL)
        AND octet_length(h.rendered->>(jsonb_array_length(h.scratch)+selected.n::integer-1))<=8192
        AND ortak_snapshot_scratch_jsonb((h.rendered->>(jsonb_array_length(h.scratch)+selected.n::integer-1))::json)
            =jsonb_build_object('type',CASE WHEN u.audience_kind='conversation' THEN 'reviewed_conversation_memory'
                ELSE 'reviewed_project_memory' END,'trust','untrusted_data','record',rec.record)
    ,false)
    UNION ALL
    SELECT h.run_id FROM history h CROSS JOIN LATERAL jsonb_array_elements(h.scratch) WITH ORDINALITY scratch(record,n)
    WHERE NOT coalesce(jsonb_typeof(scratch.record->'content')='string'
        AND scratch.record->'scope'->>'scope'='run_scratch'
        AND scratch.record->'scope'->>'run_id'=h.run_id::text
        AND scratch.record->'provenance'->>'employee_id'=h.employee_id
        AND scratch.record->'provenance'->>'run_id'=h.run_id::text
        AND jsonb_typeof(scratch.record->'record_ref')='string'
        AND octet_length(scratch.record->>'record_ref') BETWEEN 1 AND 256
        AND scratch.record->>'record_ref' !~ U&'[\0001-\001F\007F-\009F]'
        AND jsonb_typeof(scratch.record->'provenance'->'source')='string'
        AND octet_length(scratch.record->'provenance'->>'source') BETWEEN 1 AND 128
        AND scratch.record->'provenance'->>'source' !~ U&'[\0001-\001F\007F-\009F]'
        AND jsonb_typeof(scratch.record->'provenance'->'recorded_at')='string'
        AND scratch.record->'provenance'->>'recorded_at' ~ '^[+-]?[0-9]{{4,6}}-[0-9]{{2}}-[0-9]{{2}}T[0-9]{{2}}:[0-9]{{2}}:[0-9]{{2}}([.][0-9]{{1,9}})?Z$'
        AND isfinite((scratch.record->'provenance'->>'recorded_at')::timestamptz)
        AND btrim(scratch.record->>'content',U&'\0009\000A\000B\000C\000D\0020\0085\00A0\1680\2000\2001\2002\2003\2004\2005\2006\2007\2008\2009\200A\2028\2029\202F\205F\3000')<>''
        AND octet_length(scratch.record->>'content')
            -(octet_length(scratch.record->>'content')-octet_length(regexp_replace(scratch.record->>'content',E'\x01[\x01\x02]','','g')))/2<=4096
        AND octet_length(h.rendered->>(scratch.n::integer-1))<=8192
        AND ortak_snapshot_scratch_jsonb((h.rendered->>(scratch.n::integer-1))::json)
            =jsonb_build_object('type','run_scratch_memory','trust','untrusted_data','record',scratch.record),false)
    UNION ALL
    SELECT h.run_id FROM history h WHERE coalesce((SELECT sum(octet_length(u.content)) FROM uses u WHERE u.run_id=h.run_id),0)
        +coalesce((SELECT sum(octet_length(record->>'content')
            -(octet_length(record->>'content')-octet_length(regexp_replace(record->>'content',E'\x01[\x01\x02]','','g')))/2)
            FROM jsonb_array_elements(h.scratch) record),0)>16384
) SELECT count(*) FROM bad"""


def invariants(version):
    """Version76 replaces only75's execution prohibition with retained-byte proofs."""
    if version == 75:
        return dict(INVARIANTS)
    if version != 76:
        raise Refused('conversation_recovery_schema_review_required')
    return {name: sql for name, sql in INVARIANTS.items() if name != 'unsupported_conversation_execution75'} | {
        'invalid_conversation_export_history76': EXPORT_HISTORY76,
        'invalid_conversation_snapshot_history76': SNAPSHOT_HISTORY76,
    }
