"""Reviewed76 catalog invariants; matching weakened catalogs are not parity."""

TABLES = ['conversation_memory_authorities', 'reviewed_memory_conversation_audiences']
FUNCTIONS = {
    'ortak_conversation_json75': ['value jsonb, nesting integer', 'plpgsql', 'i', True,
        False, False, 's', ['search_path=pg_catalog, public, pg_temp'], 'text'],
    'ortak_conversation_source_observation': [
        'company uuid, project uuid, employee text, human bytea, source_id bytea, audience_kind text',
        'plpgsql', 's', False, False, False, 'r', ['search_path=pg_catalog, public, pg_temp'],
        'TABLE(community_id uuid, channel_id uuid, source_event_created_at timestamp with time zone, '
        'thread_root_event_id bytea, thread_root_event_created_at timestamp with time zone, '
        'audience_bytes bytea, audience_hash bytea, source_evidence_hash bytea, source_hash bytea, '
        'provenance_bytes bytea, observed_at timestamp with time zone, valid_before timestamp with time zone)'],
    'ortak_conversation_scope_current': [
        'company uuid, community uuid, project uuid, channel uuid', 'sql', 's',
        False, False, False, 'u', None, 'boolean'],
    'ortak_register_conversation_authority': [
        'company uuid, community uuid, project uuid, channel uuid', 'plpgsql', 'v',
        False, False, False, 'u', None, 'bigint'],
    'ortak_conversation_authority_guard': ['', 'plpgsql', 'v', False, False, False, 'u', None, 'trigger'],
    'ortak_conversation_fact_storage_at_commit': ['', 'plpgsql', 'v', False, False, False, 'u', None, 'trigger'],
    'ortak_conversation_use_storage_at_commit': ['', 'plpgsql', 'v', False, False, False, 'u', None, 'trigger'],
    'ortak_conversation_thread_insert_neutral75': [
        'proposed jsonb', 'sql', 's', True, False, False, 'u', None, 'boolean'],
    'ortak_advance_conversation_scopes75': [
        'companies uuid[], communities uuid[], channels uuid[], projects uuid[], employees text[], '
        'public_keys bytea[], selection text, reason text, office_fence boolean',
        'plpgsql', 'v', False, False, False, 'u', None, 'void'],
    'ortak_conversation_epoch_mutation75': ['', 'plpgsql', 'v', False, False, False, 'u', None, 'trigger'],
    'ortak_conversation_run_origin': ['company uuid, run uuid, project uuid', 'sql', 's', False, False, False, 'u', None,
        'TABLE(requester_public_key bytea, provenance_bytes bytea, observed_at timestamp with time zone, valid_before timestamp with time zone)'],
    'ortak_conversation_target_eligible76': ['company uuid, fact uuid, target uuid, publication boolean', 'sql', 's', False, False, False, 'u', None, 'boolean'],
    'ortak_conversation_export_eligible': ['company uuid, fact uuid, target uuid', 'sql', 's', False, False, False, 'u', None, 'boolean'],
    'ortak_conversation_runtime_eligible': ['company uuid, run uuid, fact uuid, target uuid, authority_epoch bigint, consumption_epoch bigint', 'sql', 's', False, False, False, 'u', None, 'boolean'],
    'ortak_conversation_effect_admission76': ['', 'plpgsql', 'v', False, False, False, 'u', None, 'trigger'],
    'ortak_conversation_snapshot76': ['company uuid, run uuid, wire jsonb', 'plpgsql', 'v', False, False, False, 'u', None, 'void'],
}

FUNCTION_DEFAULTS = sorted([
    [name, 1, '0'] if name == 'ortak_conversation_json75' else [name, 0, None]
    for name in (*FUNCTIONS, 'ortak_fence_office_mutation', 'ortak_conversation_plaintext79',
                 'ortak_run_conversation_context_current', 'ortak_conversation_snapshot_admission79')
])

# table, trigger, event/type mask, deferred, function. These are independent of
# the two compared databases and distinguish immediate guards from commit checks.
TRIGGERS = [
    ('conversation_memory_authorities', 'conversation_authority_guard', 31, False, 'ortak_conversation_authority_guard'),
    ('conversation_memory_authorities', 'conversation_authority_no_truncate', 34, False, 'ortak_reject_office_truncate'),
    ('reviewed_memory_conversation_audiences', 'conversation_audience_immutable', 27, False, 'ortak_reject_row_mutation'),
    ('reviewed_memory_conversation_audiences', 'conversation_audience_no_truncate', 34, False, 'ortak_reject_office_truncate'),
    ('reviewed_memory_facts', 'conversation_fact_storage_at_commit', 5, True, 'ortak_conversation_fact_storage_at_commit'),
    ('reviewed_memory_conversation_audiences', 'conversation_audience_storage_at_commit', 5, True, 'ortak_conversation_fact_storage_at_commit'),
    ('run_reviewed_memory_uses', 'conversation_use_storage_at_commit', 5, True, 'ortak_conversation_use_storage_at_commit'),
    ('runtime_work_outputs', 'conversation_work_output_at_commit', 21, True, 'ortak_conversation_effect_admission76'),
    ('runtime_office_outputs', 'conversation_office_output_at_commit', 21, True, 'ortak_conversation_effect_admission76'),
    ('runtime_memory_writes', 'conversation_memory_write_at_commit', 21, True, 'ortak_conversation_effect_admission76'),
    ('outbox', 'conversation_delivery_at_commit', 21, True, 'ortak_conversation_effect_admission76'),
]

# Exact root trigger locations; ordinary INSERT events do not retire epochs.
EPOCH_TRIGGERS = [
    ('channels', 'conversation_epoch_channels', 29, 'channel'),
    ('channel_members', 'conversation_epoch_members', 29, 'membership'),
    ('events', 'conversation_epoch_events', 25, 'event'),
    ('thread_metadata', 'conversation_epoch_threads', 29, 'thread'),
    ('office_inbox', 'conversation_epoch_inbox', 25, 'inbox'),
    ('projects', 'conversation_epoch_projects', 25, 'project'),
    ('project_api_bindings', 'conversation_epoch_project_bindings', 29, 'project_binding'),
    ('project_access_grants', 'conversation_epoch_grants', 29, 'grant'),
    ('users', 'conversation_epoch_users', 29, 'user'),
    ('employees', 'conversation_epoch_employees', 29, 'employee'),
    ('employee_office_bindings', 'conversation_epoch_office_identities', 29, 'office_identity'),
    ('employee_memory_bindings', 'conversation_epoch_memory_identities', 29, 'memory_identity'),
    ('companies', 'conversation_epoch_companies', 25, 'company'),
    ('communities', 'ortak_z_conversation_epoch_communities', 27, 'community'),
    ('office_company_bindings', 'conversation_epoch_company_bindings', 29, 'company_binding'),
]
THREAD_ARGUMENTS = ('community', 'community_id', 'event_id', 'event_created_at', 'channel_id',
                    'parent_event_id', 'parent_event_created_at', 'root_event_id',
                    'root_event_created_at', 'depth')

INDEXES = sorted([
    ['thread_metadata', 'idx_conversation_thread_parent_exact',
     'CREATE INDEX idx_conversation_thread_parent_exact ON public.thread_metadata USING btree '
     '(community_id, parent_event_id, parent_event_created_at) WHERE (parent_event_id IS NOT NULL)',
     '[0:2]={0,0,0}', True, True, False, False, False, True, 3, 3, 'btree', None],
    ['thread_metadata', 'idx_conversation_thread_root_exact',
     'CREATE INDEX idx_conversation_thread_root_exact ON public.thread_metadata USING btree '
     '(community_id, root_event_id, root_event_created_at) WHERE (root_event_id IS NOT NULL)',
     '[0:2]={0,0,0}', True, True, False, False, False, True, 3, 3, 'btree', None],
    ['employee_office_bindings', 'idx_conversation_office_employee_keys',
     'CREATE INDEX idx_conversation_office_employee_keys ON public.employee_office_bindings '
     'USING btree (company_id, employee_id, public_key)',
     '[0:2]={0,0,0}', True, True, False, False, False, True, 3, 3, 'btree', None],
])


def trigger_row(table, name, kind, deferred, function, arguments=(), parent=None):
    """Expected portable catalog row; parent links use names, never local OIDs."""
    encoded = ''.join(argument + '\0' for argument in arguments).encode().hex()
    return [table, name, 'O', kind, deferred, deferred, 'public', function,
            encoded, len(arguments), [], True, False, parent]


def check_epoch_catalog(value, refused):
    """Check real parent/clone attachment and the full scoped trigger metadata."""
    relations = value.get('conversation_event_relations')
    if not isinstance(relations, list) or not 2 <= len(relations) <= 1024:
        raise refused('conversation_event_partition_inventory_invalid')
    relation_map = {}
    for row in relations:
        if (not isinstance(row, list) or len(row) != 7 or row[0] != 'public'
                or not isinstance(row[1], str) or not row[1] or row[1] in relation_map):
            raise refused('conversation_event_partition_inventory_invalid')
        relation_map[row[1]] = row
    if relation_map.get('events') != ['public', 'events', None, None, 'p', False, None]:
        raise refused('conversation_event_partition_inventory_invalid')
    for name, row in relation_map.items():
        if name == 'events':
            continue
        if (row[2] != 'public' or not isinstance(row[3], str) or row[3] not in relation_map or row[4] not in ('p', 'r')
                or row[5] is not True or not isinstance(row[6], str) or not row[6]):
            raise refused('conversation_event_partition_inventory_invalid')
        seen = {name}
        parent = row[3]
        while parent != 'events':
            if (len(seen) > 32 or not isinstance(parent, str) or parent in seen
                    or parent not in relation_map or relation_map[parent][4] != 'p'):
                raise refused('conversation_event_partition_inventory_invalid')
            seen.add(parent)
            parent = relation_map[parent][3]
    expected = [trigger_row(*row) for row in TRIGGERS]
    expected += [trigger_row(table, 'community_write_fence_' + table, 31, False,
                             'enforce_community_write_fence') for table in TABLES]
    expected += [trigger_row(table, name, kind, False, 'ortak_conversation_epoch_mutation75', (argument,))
                 for table, name, kind, argument in EPOCH_TRIGGERS]
    expected.append(trigger_row('thread_metadata', 'ortak_office_authority_thread_metadata', 31,
                                False, 'ortak_fence_office_mutation', THREAD_ARGUMENTS))
    for name, row in relation_map.items():
        if name != 'events':
            expected.append(trigger_row(name, 'conversation_epoch_events', 25, False,
                'ortak_conversation_epoch_mutation75', ('event',),
                ['public', row[3], 'conversation_epoch_events']))
    observed = value.get('conversation_triggers')
    if (not isinstance(observed, list) or any(not isinstance(row, list) or len(row) != 14
            or not all(isinstance(key, str) and key for key in row[:2]) for row in observed)
            or sorted(observed, key=lambda row: row[:2]) != sorted(expected, key=lambda row: row[:2])):
        raise refused('conversation_epoch_trigger_invalid')
    if value.get('conversation_indexes') != INDEXES:
        raise refused('conversation_epoch_index_invalid')


def check(value, refused):
    """Require typed resolver metadata, retained rows and actual commit fencing."""
    functions = {row[0]: row for row in value.get('functions', [])}
    for name, metadata in FUNCTIONS.items():
        row = functions.get(name)
        if row is None or len(row) != 11 or row[1:10] != metadata or not row[10]:
            raise refused('conversation_function_metadata_invalid')
    if value.get('conversation_function_defaults') != FUNCTION_DEFAULTS:
        raise refused('conversation_function_defaults_invalid')
    triggers = {(row[0], row[1]): row for row in value.get('triggers', [])}
    for table, name, kind, deferred, function in TRIGGERS:
        row = triggers.get((table, name))
        if (row is None or len(row) != 7 or row[2:6] != ['O', kind, deferred, deferred]
                or not row[6].endswith('EXECUTE FUNCTION ' + function + '()')):
            raise refused('conversation_storage_trigger_invalid')
    if not set(TABLES) <= {row[0] for row in value.get('fence_targets', [])}:
        raise refused('conversation_community_fence_missing')
    check_epoch_catalog(value, refused)
