"""Schema74 retained workspace evidence; never opens files or renews authority.

The database witness is necessary, not sufficient, for a populated workspace
capture. A later selected operator must also inventory immutable input/run roots,
the exact reader binary and controller journal, and prove child containment.
"""

import json
import re

from backup_private_database import Refused

MAX_LAYOUT_BYTES = 1024 * 1024
MAX_BINDINGS, MAX_RUNS, MAX_READERS = 32, 64, 128

TABLE_KEYS = {
    'workspace_bindings': ('company_id', 'id'),
    'workspace_files': ('company_id', 'workspace_id', 'id'),
    'run_workspace_uses': ('company_id', 'run_id'),
    'workspace_tool_actions': ('company_id', 'run_id', 'call_id'),
    'workspace_tool_receipts': ('company_id', 'run_id', 'call_id'),
    'workspace_reader_executions': ('company_id', 'id'),
}
ACTIVATION_GATES = ['workspace_roots_and_reader_identity_selected',
    'workspace_reader_containment_confirmed', 'workspace_journal_same_key_reconciliation']

RUN_PAIRS = """SELECT run_id,workspace_id FROM run_workspace_uses WHERE company_id='{company}'
    UNION SELECT run_id,workspace_id FROM workspace_reader_executions WHERE company_id='{company}'"""


def layout_guards(company):
    """Bounds execute before the shared witness SELECT, in its same transaction."""
    return ("DO $$BEGIN IF (SELECT count(*) FROM workspace_bindings WHERE company_id='" + company
        + "')>32 OR (SELECT count(*) FROM (" + RUN_PAIRS.format(company=company)
        + ") pairs)>64 OR (SELECT count(*) FROM workspace_reader_executions WHERE company_id='" + company
        + "')>128 THEN RAISE EXCEPTION 'workspace recovery projection bound'; END IF; END$$; ")


def layout_sql(company):
    """Only canonical grants, retained run mapping and reader metadata leave this private query."""
    return """jsonb_build_object('company_id','{company}',
        'bindings',coalesce((SELECT jsonb_agg(jsonb_build_object('revision',b.id,
            'grant_bytes',convert_from(b.grant_bytes,'UTF8')) ORDER BY b.id)
            FROM workspace_bindings b WHERE b.company_id='{company}'),'[]'::jsonb),
        'runs',coalesce((SELECT jsonb_agg(jsonb_build_object('run_id',p.run_id,'revision',p.workspace_id,
            'manifest_hash',encode(b.manifest_hash,'hex'),'store_ref',u.store_ref,'status',r.status)
            ORDER BY p.run_id,p.workspace_id)
            FROM ({pairs}) p JOIN runs r ON r.company_id='{company}' AND r.id=p.run_id
            JOIN workspace_bindings b ON b.company_id=r.company_id AND b.id=p.workspace_id
            LEFT JOIN run_workspace_uses u ON u.company_id=r.company_id AND u.run_id=r.id AND u.workspace_id=p.workspace_id),'[]'::jsonb),
        'readers',coalesce((SELECT jsonb_agg(jsonb_build_object('id',e.id,'run_id',e.run_id,'revision',e.workspace_id,
            'executable',e.executable,'executable_hash',encode(e.executable_hash,'hex'),'operating_uid',e.operating_uid,
            'state',e.state,'stop_proof',e.stop_proof,'created_at',e.created_at,'owner_deadline',e.owner_deadline,
            'stopped_at',e.stopped_at) ORDER BY e.id)
            FROM workspace_reader_executions e WHERE e.company_id='{company}'),'[]'::jsonb))""".format(
                company=company, pairs=RUN_PAIRS.format(company=company))


def validate_layout(layout, evidence):
    """Refuse malformed private transport with a fixed code, without raw grant diagnostics."""
    try:
        return _validate_layout(layout, evidence)
    except Refused:
        raise
    except (ValueError, TypeError, KeyError, UnicodeError):
        raise Refused('workspace_recovery_projection_refused') from None


def _validate_layout(layout, evidence):
    """Check private projection completeness against hashes emitted by the same SQL statement.

    This validates transport and scope, not process closure or filesystem bytes.
    The file helper performs canonical grant/layout and live-barrier checks too.
    """
    def require(value):
        if not value: raise Refused('workspace_recovery_projection_refused')
    def identifier(value):
        return isinstance(value, str) and re.fullmatch(r'[0-9a-f]{8}(-[0-9a-f]{4}){3}-[0-9a-f]{12}', value)
    require(isinstance(layout, dict) and set(layout) == {'company_id','bindings','runs','readers'}
        and layout['company_id'] == evidence['company_id'])
    require(len(json.dumps(layout, ensure_ascii=False, allow_nan=False).encode()) <= MAX_LAYOUT_BYTES)
    bindings, runs, readers = (layout[name] for name in ('bindings','runs','readers'))
    require(all(isinstance(rows,list) and len(rows)<=limit for rows,limit in
        ((bindings,MAX_BINDINGS),(runs,MAX_RUNS),(readers,MAX_READERS))))
    company, revisions, run_pairs, reader_ids, file_keys, uses = layout['company_id'], set(), set(), set(), set(), set()
    for row in bindings:
        require(isinstance(row,dict) and set(row)=={'revision','grant_bytes'} and identifier(row['revision'])
            and row['revision'] not in revisions and isinstance(row['grant_bytes'],str)
            and 0<len(row['grant_bytes'].encode())<=16384)
        revisions.add(row['revision'])
        grant = json.loads(row['grant_bytes'])
        require(isinstance(grant,dict) and grant.get('company_id')==company and grant.get('revision')==row['revision']
            and isinstance(grant.get('files'),list) and 1<=len(grant['files'])<=8)
        for file in grant['files']:
            require(isinstance(file,dict) and identifier(file.get('file_id')))
            key = (company,row['revision'],file['file_id'])
            require(key not in file_keys); file_keys.add(key)
    for row in runs:
        require(isinstance(row,dict) and set(row)=={'run_id','revision','manifest_hash','store_ref','status'}
            and identifier(row['run_id']) and row['revision'] in revisions
            and isinstance(row['manifest_hash'],str) and re.fullmatch('[0-9a-f]{64}',row['manifest_hash'])
            and row['status'] in ('completed','failed','cancelled')
            and row['store_ref'] in (None,'workspace-run:'+company+':'+row['run_id']))
        pair=(row['run_id'],row['revision']); require(pair not in run_pairs); run_pairs.add(pair)
        if row['store_ref'] is not None: uses.add((company,row['run_id']))
    for row in readers:
        require(isinstance(row,dict) and set(row)=={'id','run_id','revision','executable','executable_hash',
            'operating_uid','state','stop_proof','created_at','owner_deadline','stopped_at'}
            and identifier(row['id']) and row['id'] not in reader_ids
            and (row['run_id'],row['revision']) in run_pairs and row['state']=='stopped')
        reader_ids.add(row['id'])
    expected = {'workspace_bindings': {(company,r) for r in revisions}, 'workspace_files': file_keys,
        'run_workspace_uses': uses, 'workspace_reader_executions': {(company,r) for r in reader_ids}}
    require(run_pairs == {(r['run_id'],r['revision']) for r in readers})
    for table, keys in expected.items():
        require(keys == {tuple(row['key']) for row in evidence['tables'][table]})
    return layout

# All joins carry company identity. Current employee revisions, revoked project
# grants and expired workspace bindings do not invalidate terminal history.
COUNTERS = {
    'active_workspace_runs': """SELECT count(*) FROM (
        SELECT company_id,run_id FROM run_workspace_uses WHERE company_id='{company}'
        UNION SELECT company_id,run_id FROM workspace_reader_executions WHERE company_id='{company}'
    ) u LEFT JOIN runs r ON r.company_id=u.company_id AND r.id=u.run_id
      WHERE r.id IS NULL OR r.status NOT IN ('completed','failed','cancelled')""",
    'unsettled_workspace_actions': """SELECT count(*) FROM workspace_tool_actions a
        WHERE a.company_id='{company}' AND (a.state NOT IN ('delivered','interrupted')
            OR a.lease_token IS NOT NULL OR a.lease_expires_at IS NOT NULL)""",
    'uncontained_workspace_readers': """SELECT count(*) FROM workspace_reader_executions e
        WHERE e.company_id='{company}' AND (e.state<>'stopped' OR e.stopped_at IS NULL
            OR e.stop_proof IS NULL OR e.stop_proof NOT IN ('reaped','in_process_returned','confirmed_absence')
            OR e.stopped_at<e.created_at
            OR (e.stop_proof='confirmed_absence' AND e.stopped_at<e.owner_deadline)
            OR ((e.stop_proof='in_process_returned') IS DISTINCT FROM (e.executable IS NULL)))""",
    'invalid_workspace_manifests': """SELECT count(*) FROM workspace_bindings b
        LEFT JOIN LATERAL (SELECT count(*) AS n,coalesce(sum(f.byte_count),0) AS bytes,
            jsonb_agg(jsonb_build_object('file_id',f.id,'name',f.logical_name,
                'media_type',f.media_type,'bytes',f.byte_count,'sha256',encode(f.content_hash,'hex'))
                ORDER BY f.id) AS files
            FROM workspace_files f WHERE f.company_id=b.company_id AND f.workspace_id=b.id) fs ON true
        CROSS JOIN LATERAL (SELECT jsonb_build_object('format','ortak-workspace-read/v1',
            'company_id',b.company_id,'project_id',b.project_id,'employee_id',b.employee_id,
            'workspace_ref',b.workspace_ref,'revision',b.id,'manifest_hash',encode(b.manifest_hash,'hex'),
            'files',fs.files) AS value) wire
        WHERE b.company_id='{company}' AND (fs.n NOT BETWEEN 1 AND 8 OR fs.bytes>65536
            OR b.grant_bytes<>convert_to(ortak_workspace_canonical(wire.value),'UTF8')
            OR b.manifest_hash<>sha256(convert_to(ortak_workspace_canonical(wire.value-'manifest_hash'),'UTF8'))
            OR EXISTS(SELECT 1 FROM workspace_files f WHERE f.company_id=b.company_id AND f.workspace_id=b.id
                AND (f.community_id<>b.community_id OR f.ordinal<>(SELECT count(*) FROM workspace_files p
                    WHERE p.company_id=f.company_id AND p.workspace_id=f.workspace_id AND p.id<f.id))))""",
    'invalid_workspace_parents': """SELECT count(*) FROM (
        SELECT b.id FROM workspace_bindings b WHERE b.company_id='{company}' AND (
            NOT EXISTS(SELECT 1 FROM projects p WHERE p.company_id=b.company_id AND p.id=b.project_id)
            OR NOT EXISTS(SELECT 1 FROM employees e WHERE e.company_id=b.company_id AND e.id=b.employee_id))
        UNION ALL SELECT f.id FROM workspace_files f WHERE f.company_id='{company}' AND NOT EXISTS(
            SELECT 1 FROM workspace_bindings b WHERE b.company_id=f.company_id AND b.id=f.workspace_id
                AND b.community_id=f.community_id)
        UNION ALL SELECT u.run_id FROM run_workspace_uses u WHERE u.company_id='{company}' AND NOT EXISTS(
            SELECT 1 FROM runs r JOIN workspace_bindings b ON b.company_id=r.company_id AND b.id=u.workspace_id
            JOIN outbox o ON o.company_id=u.company_id AND o.id=u.outbox_id
            JOIN work_executions w ON w.company_id=r.company_id AND w.run_id=r.id
            WHERE r.company_id=u.company_id AND r.id=u.run_id AND r.employee_id=b.employee_id
                AND r.employee_revision_id=u.employee_revision_id AND r.employee_lifecycle_epoch=u.employee_lifecycle_epoch
                AND b.community_id=u.community_id AND b.manifest_hash=u.manifest_hash
                AND o.kind='work_run_dispatch' AND o.run_id=u.run_id
                AND w.project_id=b.project_id AND w.work_item_id=r.work_item_id
                AND u.store_ref='workspace-run:'||u.company_id::text||':'||u.run_id::text
                AND EXISTS(SELECT 1 FROM workspace_reader_executions e WHERE e.company_id=u.company_id
                    AND e.run_id=u.run_id AND e.workspace_id=u.workspace_id AND e.community_id=u.community_id
                    AND e.request_key='prepare' AND e.owner_lease=u.admission_lease AND e.state='stopped'
                    AND e.stop_proof IN ('reaped','in_process_returned')))
        UNION ALL SELECT a.run_id FROM workspace_tool_actions a WHERE a.company_id='{company}' AND NOT EXISTS(
            SELECT 1 FROM run_workspace_uses u JOIN workspace_files f ON f.company_id=u.company_id AND f.workspace_id=u.workspace_id
            WHERE u.company_id=a.company_id AND u.run_id=a.run_id AND u.community_id=a.community_id
                AND f.id=a.file_id AND f.community_id=a.community_id)
        UNION ALL SELECT e.id FROM workspace_reader_executions e WHERE e.company_id='{company}' AND NOT EXISTS(
            SELECT 1 FROM runs r JOIN workspace_bindings b ON b.company_id=r.company_id AND b.id=e.workspace_id
            WHERE r.company_id=e.company_id AND r.id=e.run_id AND b.community_id=e.community_id
                AND r.employee_id=b.employee_id AND (
                    (e.request_key='prepare' AND EXISTS(SELECT 1 FROM outbox o WHERE o.company_id=e.company_id
                        AND o.run_id=e.run_id AND o.kind='work_run_dispatch'))
                    OR EXISTS(SELECT 1 FROM workspace_tool_actions a JOIN run_workspace_uses u
                        ON u.company_id=a.company_id AND u.run_id=a.run_id
                        WHERE a.company_id=e.company_id AND a.run_id=e.run_id AND e.request_key='read:'||a.call_id
                            AND u.workspace_id=e.workspace_id AND u.community_id=e.community_id AND a.community_id=e.community_id)))
    ) invalid""",
    'invalid_workspace_receipt_history': """SELECT count(*) FROM workspace_tool_receipts x
        WHERE x.company_id='{company}' AND NOT EXISTS(
            SELECT 1 FROM workspace_tool_actions a JOIN run_workspace_uses u
                ON u.company_id=a.company_id AND u.run_id=a.run_id
            WHERE a.company_id=x.company_id AND a.run_id=x.run_id AND a.call_id=x.call_id
                AND a.community_id=x.community_id AND u.community_id=x.community_id
                AND a.arguments_hash=x.arguments_hash AND x.attempt_count<=a.attempt_count
                AND a.state IN ('result_ready','delivered','interrupted')
                AND EXISTS(SELECT 1 FROM workspace_reader_executions e WHERE e.company_id=x.company_id
                    AND e.run_id=x.run_id AND e.workspace_id=u.workspace_id AND e.community_id=x.community_id
                    AND e.request_key='read:'||x.call_id AND e.owner_lease=x.lease_token
                    AND e.state='stopped' AND e.stop_proof IN ('reaped','in_process_returned')))""",
    'workspace_actions_missing_receipts': """SELECT count(*) FROM workspace_tool_actions a
        WHERE a.company_id='{company}' AND a.state IN ('result_ready','delivered') AND NOT EXISTS(
            SELECT 1 FROM workspace_tool_receipts r WHERE r.company_id=a.company_id
                AND r.run_id=a.run_id AND r.call_id=a.call_id)""",
    'invalid_workspace_result_bytes': """SELECT count(*) FROM workspace_tool_receipts x
        JOIN workspace_tool_actions a ON a.company_id=x.company_id AND a.run_id=x.run_id AND a.call_id=x.call_id
        JOIN run_workspace_uses u ON u.company_id=a.company_id AND u.run_id=a.run_id
        JOIN workspace_files f ON f.company_id=u.company_id AND f.workspace_id=u.workspace_id AND f.id=a.file_id
        CROSS JOIN LATERAL (SELECT convert_from(x.result_bytes,'UTF8')::jsonb AS value) wire
        WHERE x.company_id='{company}' AND (x.result_hash<>sha256(x.result_bytes)
            OR x.result_bytes<>convert_to(ortak_workspace_canonical(wire.value),'UTF8')
            OR NOT coalesce((wire.value->>'status'='failed'
                AND wire.value->>'code' IN ('authority_changed','workspace_unavailable','file_unavailable',
                    'input_changed','deadline_exceeded','cancelled')
                AND wire.value=jsonb_build_object('status','failed','code',wire.value->>'code'))
                OR (wire.value->>'status'='completed'
                    AND wire.value=jsonb_build_object('status','completed','content',wire.value->>'content',
                        'sha256',encode(f.content_hash,'hex'),'bytes',f.byte_count,'name',f.logical_name)
                    AND octet_length(convert_to(wire.value->>'content','UTF8'))=f.byte_count
                    AND sha256(convert_to(wire.value->>'content','UTF8'))=f.content_hash),false))""",
}


def require_capture_selection(metadata, selected, company):
    """Populated workspace state cannot enter a full bundle without an explicit file selection."""
    # The caller has already checked the full reviewed migration ledger.
    version=metadata['migration_checksums'][-1][0]
    if version<74:
        if selected is not None: raise Refused('workspace_capture_schema_refused')
        return
    if selected is None:
        if any(metadata['tables'].get('public.'+name,metadata['tables'].get(name,0)) != 0 for name in TABLE_KEYS):
            raise Refused('workspace_capture_selection_required')
    else:
        from recovery_workspace_layout import selection
        if selection(selected)['company_id'] != company: raise Refused('workspace_capture_company_refused')


def require_capture_scope(metadata, evidence):
    """A one-company file selection cannot omit another company's files from a full database archive."""
    if evidence['schema_version']<74: return
    if any(metadata['tables'].get('public.'+name,metadata['tables'].get(name,0)) != len(evidence['tables'][name])
            for name in TABLE_KEYS):
        raise Refused('workspace_capture_foreign_scope')
