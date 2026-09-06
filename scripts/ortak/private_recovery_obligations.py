"""Reviewed recovery obligations, independent of current image/process selections.

No model, adapter or application is called here. Exact rows stay in the database
archive; the public witness contains only scoped keys and hashes of complete rows.
"""

import json
import re

from backup_private_database import Refused
import private_recovery_workspaces as workspaces
import private_recovery_conversations as conversations
import private_recovery_extensions77 as extensions77

MAX_ROWS = 1024
MAX_BYTES = 384 * 1024
PROBE = 'provisioning_runtime_probes'
DECOMPOSITION = 'work_decomposition'
REVIEWED_USES = 'run_reviewed_memory_uses'
# These names remain forbidden under every earlier ledger. Runtime semantics
# are explicitly selected by the reviewed75/76 contract, never table discovery.
UNREVIEWED_CONVERSATION_TABLES = frozenset(conversations.TABLE_KEYS)
#78 changes only current epoch invalidation; its retained storage proof is77.
REVIEWED_VERSIONS = frozenset(range(61, 79))
# These are forbidden under every earlier ledger. Database review of74 does not
# advance the selected live inventory or authorize new filesystem/process roots.
UNREVIEWED_WORKSPACE_TABLES = frozenset(workspaces.TABLE_KEYS)
EXPORT_TABLES = {
    'reviewed_memory_targets': ('company_id', 'id'),
    'reviewed_memory_exports': ('company_id', 'fact_id'),
    'reviewed_memory_export_jobs': ('company_id', 'fact_id', 'action'),
    'reviewed_memory_export_commands': ('company_id', 'actor_pubkey', 'operation_id'),
    'reviewed_memory_export_receipts': ('company_id', 'fact_id', 'action'),
}
HONCHO_BASE = ('ortak_resource_receipts', 'ortak_session_ownership', 'ortak_write_receipts')
HONCHO_REVIEWED = ('ortak_reviewed_records', 'ortak_reviewed_record_content',
                  'ortak_reviewed_tombstones', 'ortak_reviewed_operations')
ACTIVATION_GATES = ['original_writers_contained', 'same_key_remote_reconciliation',
                    'retained_withdrawal_expiry_catch_up', 'explicit_root_activation']


def require(value, code):
    """Never expose rejected rows or opaque bindings in error output."""
    if not value:
        raise Refused(code)


def table_keys(version):
    """A reviewed schema version is input authority, never inferred from a newer table."""
    require(type(version) is int and version in REVIEWED_VERSIONS, 'recovery_schema_review_required')
    return ({PROBE: ('company_id', 'operation_id', 'generation')} if version >= 68 else {}) | (
        EXPORT_TABLES if version >= 69 else {}) | (
        {DECOMPOSITION: ('company_id', 'child_id')} if version >= 70 else {}) | (
        {REVIEWED_USES: ('company_id', 'run_id', 'ordinal')} if version >= 71 else {}) | (
        workspaces.TABLE_KEYS if version >= 74 else {}) | (
        conversations.TABLE_KEYS if version >= 75 else {}) | (
        extensions77.TABLE_KEYS if version in (77, 78) else {})


def activation_gates(version):
    """Historical69–73 contracts remain exact;74 adds no automatic activation."""
    table_keys(version)
    return ACTIVATION_GATES + (workspaces.ACTIVATION_GATES if version >= 74 else []) + (
        conversations.ACTIVATION_GATES if version >= 75 else []) + (
        extensions77.ACTIVATION_GATES if version in (77, 78) else [])


def schema_version(metadata):
    """Validate the observed ledger shape; the selected preparation binds exact checksums."""
    require(isinstance(metadata, dict), 'recovery_migration_ledger_refused')
    ledger = metadata.get('migration_checksums')
    require(isinstance(ledger, list) and 0 < len(ledger) <= 78, 'recovery_migration_ledger_refused')
    prior = 0
    for row in ledger:
        require(isinstance(row, list) and len(row) == 3 and type(row[0]) is int
            and row[0] == prior + 1 and isinstance(row[1], str) and re.fullmatch(r'[0-9a-f]{96}', row[1])
            and row[2] is True, 'recovery_migration_ledger_refused')
        prior = row[0]
    table_keys(prior)
    return prior


def main_contract(metadata):
    """Require the complete new retained inventory without advancing a live pin."""
    version = schema_version(metadata)
    expected = set(table_keys(version))
    known = {PROBE, *EXPORT_TABLES, DECOMPOSITION, REVIEWED_USES, *workspaces.TABLE_KEYS,
             *UNREVIEWED_CONVERSATION_TABLES, *extensions77.TABLE_KEYS}
    tables = metadata.get('tables')
    require(isinstance(tables, dict) and all(isinstance(name, str) and name
        and type(count) is int and count >= 0 for name, count in tables.items()),
        'recovery_retained_table_inventory_refused')
    actual = {name.removeprefix('public.') for name in tables}
    require(len(actual) == len(tables) and actual & known == expected,
        'recovery_retained_table_inventory_refused')
    require({name for name in actual if name.startswith(('workspace_', 'run_workspace_'))}
        <= set(workspaces.TABLE_KEYS), 'workspace_recovery_schema_review_required')
    contract = {'schema_version': version, 'retained_tables': sorted(expected),
            'automatic_activation': False, 'activation_requires': activation_gates(version)}
    if version >= 70:
        # Preserve the exact existing69 contract. New approvals explicitly state
        # that company evidence is not community purge authority, and historical
        # use pins are never promoted back into live runtime authority.
        contract['retained_table_ownership'] = {table: 'company' if table in (PROBE, DECOMPOSITION)
            else 'company_and_community' for table in sorted(expected)}
        contract['historical_evidence'] = 'preserve_exact_rows_without_renewing_authority'
    if version >= 75:
        require({name for name in actual if name.startswith(('conversation_', 'reviewed_memory_conversation_'))}
            <= set(conversations.TABLE_KEYS), 'conversation_recovery_schema_review_required')
        contract['conversation_memory'] = conversations.recovery_contract(min(version, 76))
    require({name for name in actual if name.startswith(('employee_memory_', 'employee_reviewed_memory_',
        'run_employee_reviewed_', 'encrypted_dm_', 'confidential_'))}
        <= set(extensions77.TABLE_KEYS) | {'employee_memory_bindings'},
        'recovery_extension_schema_review_required')
    if version in (77, 78):
        contract['employee_and_protected_memory'] = extensions77.contract()
        contract['conversation_memory'] = contract['conversation_memory'] | {
            'runtime_publication': 'schema77_retained_v4_and_mixed_v5_uses_only',
            'mixed_employee_snapshot_version': 5}
    return contract


def honcho_contract(metadata):
    """Recognize all four D2a tables together, retaining native and extension metadata."""
    actual = {name.removeprefix('public.') for name in metadata['tables']}
    reviewed = actual & set(HONCHO_REVIEWED)
    employee = actual & set(extensions77.HONCHO_KEYS)
    require(set(HONCHO_BASE) <= actual and (not reviewed or reviewed == set(HONCHO_REVIEWED)),
            'honcho_reviewed_table_inventory_refused')
    require(not employee or employee == set(extensions77.HONCHO_KEYS), 'employee_honcho_inventory_refused')
    require({name for name in actual if name.startswith('ortak_')} == set(HONCHO_BASE) | reviewed | employee,
            'honcho_extension_review_required')
    return {'extension_tables': sorted(set(HONCHO_BASE) | reviewed | employee),
            'reviewed_wire_family': 'reviewed-project/1' if reviewed else None,
            'expiry_mutation': 'explicit_withdrawal_only', 'automatic_activation': False} | (
            {'employee_wire_family': 'reviewed-employee/1'} if employee else {})


def stack_contract(main_metadata, honcho_metadata):
    """This selected Honcho-backed stack cannot capture schema69 against an older extension."""
    main, honcho = main_contract(main_metadata), honcho_contract(honcho_metadata)
    require(main['schema_version'] < 69 or honcho['reviewed_wire_family'] == 'reviewed-project/1',
            'reviewed_export_honcho_generation_missing')
    require((main['schema_version'] in (77, 78)) == (honcho.get('employee_wire_family') == 'reviewed-employee/1'),
            'employee_honcho_generation_not_selected')
    return {'main': main, 'honcho': honcho,
            'future_withdrawals': 'retain_exact_rows_without_running_them',
            'activation_requires': activation_gates(main['schema_version'])}


def counters(version):
    """SQL predicates share the real job/receipt states; ACK lease fields are historical."""
    table_keys(version)
    result = {}
    if version >= 68:
        result['uncontained_runtime_probes'] = (
            "SELECT count(*) FROM provisioning_runtime_probes WHERE company_id='{company}' "
            "AND (state NOT IN ('succeeded','failed') OR contained_at IS NULL)")
    if version < 69:
        return result
    result.update({
        'uncertain_or_due_export_jobs': "SELECT count(*) FROM reviewed_memory_export_jobs j "
            "WHERE j.company_id='{company}' AND (j.state='failed' OR (j.state='pending' AND "
            "(j.lease_token IS NOT NULL OR j.total_attempts>0 OR j.last_error_code IS NOT NULL "
            "OR j.action='publish' OR j.next_attempt_at<=clock_timestamp())))",
        'invalid_export_acknowledgements': "SELECT count(*) FROM reviewed_memory_export_jobs j "
            "WHERE j.company_id='{company}' AND j.state='acknowledged' AND NOT EXISTS ("
            "SELECT 1 FROM reviewed_memory_export_receipts r JOIN reviewed_memory_exports e "
            "ON e.company_id=r.company_id AND e.fact_id=r.fact_id "
            "JOIN reviewed_memory_targets t ON t.company_id=e.company_id AND t.id=e.target_id "
            "WHERE r.company_id=j.company_id AND r.fact_id=j.fact_id AND r.action=j.action "
            "AND r.community_id=j.community_id AND e.community_id=j.community_id AND t.community_id=j.community_id "
            "AND r.request_hash=j.request_hash AND r.binding_hash=t.binding_hash "
            "AND r.lease_token=j.lease_token AND r.total_attempts=j.total_attempts "
            "AND j.idempotency_key='reviewed:'||j.action||':'||j.fact_id::text "
            "AND (j.action='publish' OR (r.erased_from_reviewed_store AND r.tombstone_at IS NOT NULL "
            "AND r.remote_status IN ('expired','withdrawn'))))",
        'incomplete_export_recovery_pairs': "SELECT count(*) FROM reviewed_memory_exports e "
            "WHERE e.company_id='{company}' AND ("
            "NOT EXISTS (SELECT 1 FROM reviewed_memory_export_jobs p WHERE p.company_id=e.company_id "
            "AND p.fact_id=e.fact_id AND p.community_id=e.community_id AND p.action='publish' AND p.state='acknowledged') "
            "OR NOT EXISTS (SELECT 1 FROM reviewed_memory_export_jobs w WHERE w.company_id=e.company_id "
            "AND w.fact_id=e.fact_id AND w.community_id=e.community_id AND w.action='withdraw' "
            "AND (w.state='acknowledged' OR (w.state='pending' AND w.lease_token IS NULL "
            "AND w.total_attempts=0 AND w.last_error_code IS NULL AND w.next_attempt_at>clock_timestamp()))))",
    })
    if version >= 71:
        # Terminal uses may outlive fact withdrawal, opt-out or expiry. Only an
        # active/missing parent run is a drain problem; current eligibility must
        # not rewrite or reject retained historical snapshot bytes.
        result['active_reviewed_memory_runs'] = (
            "SELECT count(DISTINCT u.run_id) FROM run_reviewed_memory_uses u "
            "LEFT JOIN runs r ON r.company_id=u.company_id AND r.id=u.run_id "
            "WHERE u.company_id='{company}' AND (r.id IS NULL "
            "OR r.status NOT IN ('completed','failed','cancelled'))")
    if version >= 74:
        result.update(workspaces.COUNTERS)
    if version >= 75:
        result.update(extensions77.legacy_invariants() if version in (77, 78) else conversations.invariants(version))
    if version in (77, 78):
        result.update(extensions77.HISTORY)
        result.update(extensions77.DRAIN)
    return result


def query(version, company, *, workspace_layout=False):
    """One bounded, read-only snapshot hashes full rows and retains exact public primary keys."""
    tables = table_keys(version)
    require(isinstance(company, str) and re.fullmatch(r'[0-9a-f]{8}(-[0-9a-f]{4}){3}-[0-9a-f]{12}', company),
            'recovery_company_scope_refused')
    require(type(workspace_layout) is bool and (not workspace_layout or version>=74),
            'workspace_recovery_schema_review_required')
    guards, rows = [], []
    for table, keys in tables.items():
        scope = " FROM public." + table + " t WHERE t.company_id='" + company + "'"
        guards.append('(SELECT count(*)' + scope + ')')
        key = "jsonb_build_array(" + ','.join('t.' + name for name in keys) + ')'
        rows.append("'" + table + "',coalesce((SELECT jsonb_agg(jsonb_build_object('key'," + key
            + ",'row_sha256',encode(sha256(convert_to(to_jsonb(t)::text,'UTF8')),'hex')) ORDER BY "
            + ','.join('t.' + name for name in keys) + ')' + scope + "),'[]'::jsonb)")
    count = '+'.join(guards) or '0'
    checks = ','.join("'" + name + "',(" + sql.format(company=company) + ')'
                      for name, sql in counters(version).items())
    return ("BEGIN ISOLATION LEVEL REPEATABLE READ READ ONLY; SET LOCAL TIME ZONE 'UTC'; "
        "DO $$BEGIN IF " + count + '>' + str(MAX_ROWS)
        + " THEN RAISE EXCEPTION 'recovery obligation bound'; END IF; END$$; "
        + (conversations.snapshot_guards76(company) if version >= 76 else '')
        + (extensions77.guards(company) if version in (77, 78) else '')
        + (workspaces.layout_guards(company) if workspace_layout else '')
        + "SELECT jsonb_build_object('counters',jsonb_build_object(" + checks + "),'evidence',jsonb_build_object("
        "'schema_version'," + str(version) + ",'company_id','" + company + "','tables',jsonb_build_object("
        + ','.join(rows) + '))' + (",'workspace_layout'," + workspaces.layout_sql(company)
            if workspace_layout else '') + '); ROLLBACK;')


def observe(commands, database, metadata, company, *, drained, label='recovery-obligations'):
    """Capture refuses live obligations; offline comparison never runs expired jobs or refreshes OAuth."""
    version = main_contract(metadata)['schema_version']
    require(label in ('recovery-obligations', 'restored-recovery-obligations'),
            'recovery_obligation_label_refused')
    value = json.loads(commands.run(label, commands.psql(database),
        sql=query(version, company), ceiling=MAX_BYTES))
    return validate_observation(value, version, company, drained=drained)


def validate_observation(value, version, company, *, drained):
    """Validate the exact scoped SQL witness without treating transport data as a barrier."""
    require(set(value) == {'counters', 'evidence'} and set(value['counters']) == set(counters(version)),
            'recovery_obligation_counters_refused')
    require(all(type(n) is int and n >= 0 for n in value['counters'].values()), 'recovery_obligation_counters_refused')
    evidence = value['evidence']
    require(set(evidence) == {'schema_version', 'company_id', 'tables'}
        and evidence['schema_version'] == version and evidence['company_id'] == company
        and set(evidence['tables']) == set(table_keys(version)), 'recovery_obligation_inventory_refused')
    total = 0
    for table, keys in table_keys(version).items():
        rows = evidence['tables'][table]
        require(isinstance(rows, list), 'recovery_obligation_rows_refused')
        total += len(rows)
        seen = set()
        for row in rows:
            require(set(row) == {'key', 'row_sha256'} and isinstance(row['key'], list)
                and len(row['key']) == len(keys) and row['key'][0] == company
                and isinstance(row['row_sha256'], str) and re.fullmatch(r'[0-9a-f]{64}', row['row_sha256']),
                'recovery_obligation_rows_refused')
            key = json.dumps(row['key'], separators=(',', ':'))
            require(key not in seen, 'recovery_obligation_duplicate_key')
            seen.add(key)
    require(total <= MAX_ROWS, 'recovery_obligation_bound')
    if version >= 75:
        # Reuse unchanged61–76 structural checks for the same old tables/pins.
        # The separate77 v5 counter excludes those rows from the v4 grammar.
        conversations.validate_evidence(evidence | {'schema_version': min(version, 76)}, value['counters'])
    if version in (77, 78):
        extensions77.validate(evidence, value['counters'])
    if drained:
        require(all(n == 0 for n in value['counters'].values()), 'recovery_obligations_not_drained')
    return evidence


def observe_workspace_layout(commands, database, metadata, company):
    """Drained raw layout and hashes share ONE bounded read-only transaction and SELECT."""
    version = main_contract(metadata)['schema_version']
    require(version>=74, 'workspace_recovery_schema_review_required')
    value = json.loads(commands.run('recovery-obligations', commands.psql(database),
        sql=query(version, company, workspace_layout=True), ceiling=workspaces.MAX_LAYOUT_BYTES))
    require(isinstance(value,dict) and set(value)=={'counters','evidence','workspace_layout'},
        'workspace_recovery_projection_refused')
    evidence = validate_observation({k:value[k] for k in ('counters','evidence')},version,company,drained=True)
    layout = workspaces.validate_layout(value['workspace_layout'],evidence)
    return {'database_evidence':evidence,'workspace_layout':layout}


def verify_restore(commands, database, metadata, company, expected):
    """Missing or changed evidence refuses; due work stays inert until later explicit reconciliation."""
    observed = observe(commands, database, metadata, company, drained=False,
                       label='restored-recovery-obligations')
    require(observed == expected, 'restored_recovery_obligations_changed')
    return {'evidence': observed, 'automatic_activation': False,
            'activation_requires': activation_gates(observed['schema_version'])}


HONCHO_COUNTERS = {
    'content_without_matching_header_or_after_tombstone': "SELECT count(*) FROM ortak_reviewed_record_content c "
        "LEFT JOIN ortak_reviewed_records r USING(workspace_id,project_id,record_id) "
        "LEFT JOIN ortak_reviewed_tombstones t USING(workspace_id,project_id,record_id) "
        "WHERE r.record_id IS NULL OR t.record_id IS NOT NULL "
        "OR r.content_hash<>encode(sha256(convert_to(c.content,'UTF8')),'hex')",
    'header_without_text_or_tombstone': "SELECT count(*) FROM ortak_reviewed_records r "
        "LEFT JOIN ortak_reviewed_record_content c USING(workspace_id,project_id,record_id) "
        "LEFT JOIN ortak_reviewed_tombstones t USING(workspace_id,project_id,record_id) "
        "WHERE c.record_id IS NULL AND t.record_id IS NULL",
    'header_without_publish_receipt': "SELECT count(*) FROM ortak_reviewed_records r WHERE NOT EXISTS "
        "(SELECT 1 FROM ortak_reviewed_operations o WHERE o.workspace_id=r.workspace_id "
        "AND o.project_id=r.project_id AND o.record_id=r.record_id AND o.action='publish' "
        "AND o.idempotency_key=r.publish_key AND o.request_hash=r.request_hash)",
    'tombstone_without_erasure_receipt': "SELECT count(*) FROM ortak_reviewed_tombstones t WHERE NOT EXISTS "
        "(SELECT 1 FROM ortak_reviewed_operations o WHERE o.workspace_id=t.workspace_id "
        "AND o.project_id=t.project_id AND o.record_id=t.record_id "
        "AND o.action=CASE t.reason WHEN 'expired' THEN 'expire' ELSE 'withdraw' END)",
    'tombstone_header_scope_mismatch': "SELECT count(*) FROM ortak_reviewed_tombstones t "
        "JOIN ortak_reviewed_records r USING(workspace_id,project_id,record_id) "
        "WHERE t.company_id<>r.company_id OR t.employee_id<>r.employee_id OR t.binding_hash<>r.binding_hash",
    'operation_without_lifecycle_evidence': "SELECT count(*) FROM ortak_reviewed_operations o WHERE "
        "(o.action='publish' AND NOT EXISTS (SELECT 1 FROM ortak_reviewed_records r "
        "WHERE r.workspace_id=o.workspace_id AND r.project_id=o.project_id AND r.record_id=o.record_id)) "
        "OR (o.action IN ('withdraw','expire') AND NOT EXISTS (SELECT 1 FROM ortak_reviewed_tombstones t "
        "WHERE t.workspace_id=o.workspace_id AND t.project_id=o.project_id AND t.record_id=o.record_id))",
}


def verify_honcho(commands, database, metadata, *, label='reviewed-honcho-invariants'):
    """Check retained lifecycle semantics without treating expired text as a cleanup ACK."""
    contract = honcho_contract(metadata)
    require(label in ('reviewed-honcho-invariants', 'restored-reviewed-honcho-invariants'),
            'honcho_recovery_label_refused')
    if contract['reviewed_wire_family'] is None:
        require('employee_wire_family' not in contract, 'employee_honcho_legacy_family_missing')
        return contract
    if 'employee_wire_family' in contract:
        result = json.loads(commands.run(label, commands.psql(database),
            sql=extensions77.honcho_query(HONCHO_COUNTERS), ceiling=MAX_BYTES))
        tables = extensions77.validate_honcho(result, HONCHO_COUNTERS)
        return contract | {'employee_retained_evidence': tables}
    checks = ','.join("'" + name + "',(" + sql + ')' for name, sql in HONCHO_COUNTERS.items())
    result = json.loads(commands.run(label, commands.psql(database),
        sql='BEGIN ISOLATION LEVEL REPEATABLE READ READ ONLY; SELECT jsonb_build_object('
            + checks + '); ROLLBACK;', ceiling=4096))
    require(set(result) == set(HONCHO_COUNTERS) and all(type(n) is int and n == 0 for n in result.values()),
            'honcho_reviewed_lifecycle_inconsistent')
    return contract
