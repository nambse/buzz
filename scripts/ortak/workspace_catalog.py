"""Migration 74 catalog requirements, independent of migrated/desired equality.

Catalog-rendered definitions were read in an explicit read-only transaction and
reviewed against immutable 0074_ortak_workspace_text_tools.sql. Never normalize
SQL bodies/expressions or infer authority from equal but weakened catalogs.
"""

TABLES = ('workspace_bindings', 'workspace_files', 'run_workspace_uses', 'workspace_tool_actions', 'workspace_tool_receipts', 'workspace_reader_executions')

FUNCTIONS = {
    'ortak_lock_run_workspace': ['company uuid, run uuid, require_use boolean', 'plpgsql', 'v', False, False, False, 'u', None, 'boolean'],
    'ortak_run_workspace_current': ['company uuid, run uuid, require_use boolean', 'sql', 's', False, False, False, 'u', None, 'boolean'],
    'ortak_workspace_action_at_commit': ['', 'plpgsql', 'v', False, False, False, 'u', None, 'trigger'],
    'ortak_workspace_action_guard': ['', 'plpgsql', 'v', False, False, False, 'u', None, 'trigger'],
    'ortak_workspace_activation_at_commit': ['', 'plpgsql', 'v', False, False, False, 'u', None, 'trigger'],
    'ortak_workspace_binding_guard': ['', 'plpgsql', 'v', False, False, False, 'u', None, 'trigger'],
    'ortak_workspace_canonical': ['value jsonb', 'sql', 'i', True, False, False, 'u', None, 'text'],
    'ortak_workspace_manifest_consistent': ['', 'plpgsql', 'v', False, False, False, 'u', None, 'trigger'],
    'ortak_workspace_profile_available': ['company uuid, employee text, workspace text', 'sql', 's', False, False, False, 'u', None, 'boolean'],
    'ortak_workspace_reader_cancel_fence': ['', 'plpgsql', 'v', False, False, False, 'u', None, 'trigger'],
    'ortak_workspace_reader_guard': ['', 'plpgsql', 'v', False, False, False, 'u', None, 'trigger'],
    'ortak_workspace_receipt_at_commit': ['', 'plpgsql', 'v', False, False, False, 'u', None, 'trigger'],
    'ortak_workspace_run_admission': ['', 'plpgsql', 'v', False, False, False, 'u', None, 'trigger'],
    'ortak_workspace_use_at_commit': ['', 'plpgsql', 'v', False, False, False, 'u', None, 'trigger'],
}

TRIGGERS = [
    ['artifacts', 'workspace_artifact_admission', 'O', 5, True, True, 'public', 'ortak_workspace_run_admission', '', 0, [], True],
    ['employees', 'workspace_activation_at_commit', 'O', 21, True, True, 'public', 'ortak_workspace_activation_at_commit', '', 0, [], True],
    ['run_workspace_uses', 'community_write_fence_run_workspace_uses', 'O', 31, False, False, 'public', 'enforce_community_write_fence', '', 0, [], True],
    ['run_workspace_uses', 'workspace_immutable', 'O', 19, False, False, 'public', 'ortak_reject_row_mutation', '', 0, [], True],
    ['run_workspace_uses', 'workspace_no_delete', 'O', 11, False, False, 'public', 'ortak_reject_row_mutation', '', 0, [], True],
    ['run_workspace_uses', 'workspace_no_truncate', 'O', 34, False, False, 'public', 'ortak_reject_office_truncate', '', 0, [], True],
    ['run_workspace_uses', 'workspace_use_at_commit', 'O', 5, True, True, 'public', 'ortak_workspace_use_at_commit', '', 0, [], True],
    ['runs', 'workspace_run_admission', 'O', 17, True, True, 'public', 'ortak_workspace_run_admission', '', 0, [], True],
    ['runtime_cancellations', 'workspace_reader_cancel_fence', 'O', 17, True, True, 'public', 'ortak_workspace_reader_cancel_fence', '', 0, [], True],
    ['workspace_bindings', 'community_write_fence_workspace_bindings', 'O', 31, False, False, 'public', 'enforce_community_write_fence', '', 0, [], True],
    ['workspace_bindings', 'workspace_binding_authority', 'O', 31, False, False, 'public', 'ortak_fence_office_mutation', '636f6d70616e7900636f6d70616e795f6964007265766f6b65645f617400', 3, [], True],
    ['workspace_bindings', 'workspace_binding_guard', 'O', 23, False, False, 'public', 'ortak_workspace_binding_guard', '', 0, [], True],
    ['workspace_bindings', 'workspace_manifest_consistent', 'O', 5, True, True, 'public', 'ortak_workspace_manifest_consistent', '', 0, [], True],
    ['workspace_bindings', 'workspace_no_delete', 'O', 11, False, False, 'public', 'ortak_reject_row_mutation', '', 0, [], True],
    ['workspace_bindings', 'workspace_no_truncate', 'O', 34, False, False, 'public', 'ortak_reject_office_truncate', '', 0, [], True],
    ['workspace_files', 'community_write_fence_workspace_files', 'O', 31, False, False, 'public', 'enforce_community_write_fence', '', 0, [], True],
    ['workspace_files', 'workspace_files_consistent', 'O', 5, True, True, 'public', 'ortak_workspace_manifest_consistent', '', 0, [], True],
    ['workspace_files', 'workspace_immutable', 'O', 19, False, False, 'public', 'ortak_reject_row_mutation', '', 0, [], True],
    ['workspace_files', 'workspace_no_delete', 'O', 11, False, False, 'public', 'ortak_reject_row_mutation', '', 0, [], True],
    ['workspace_files', 'workspace_no_truncate', 'O', 34, False, False, 'public', 'ortak_reject_office_truncate', '', 0, [], True],
    ['workspace_reader_executions', 'community_write_fence_workspace_reader_executions', 'O', 31, False, False, 'public', 'enforce_community_write_fence', '', 0, [], True],
    ['workspace_reader_executions', 'workspace_reader_guard', 'O', 23, False, False, 'public', 'ortak_workspace_reader_guard', '', 0, [], True],
    ['workspace_reader_executions', 'workspace_reader_no_delete', 'O', 11, False, False, 'public', 'ortak_reject_row_mutation', '', 0, [], True],
    ['workspace_reader_executions', 'workspace_reader_no_truncate', 'O', 34, False, False, 'public', 'ortak_reject_office_truncate', '', 0, [], True],
    ['workspace_tool_actions', 'community_write_fence_workspace_tool_actions', 'O', 31, False, False, 'public', 'enforce_community_write_fence', '', 0, [], True],
    ['workspace_tool_actions', 'workspace_action_at_commit', 'O', 21, True, True, 'public', 'ortak_workspace_action_at_commit', '', 0, [], True],
    ['workspace_tool_actions', 'workspace_action_guard', 'O', 23, False, False, 'public', 'ortak_workspace_action_guard', '', 0, [], True],
    ['workspace_tool_actions', 'workspace_no_delete', 'O', 11, False, False, 'public', 'ortak_reject_row_mutation', '', 0, [], True],
    ['workspace_tool_actions', 'workspace_no_truncate', 'O', 34, False, False, 'public', 'ortak_reject_office_truncate', '', 0, [], True],
    ['workspace_tool_receipts', 'community_write_fence_workspace_tool_receipts', 'O', 31, False, False, 'public', 'enforce_community_write_fence', '', 0, [], True],
    ['workspace_tool_receipts', 'workspace_immutable', 'O', 19, False, False, 'public', 'ortak_reject_row_mutation', '', 0, [], True],
    ['workspace_tool_receipts', 'workspace_no_delete', 'O', 11, False, False, 'public', 'ortak_reject_row_mutation', '', 0, [], True],
    ['workspace_tool_receipts', 'workspace_no_truncate', 'O', 34, False, False, 'public', 'ortak_reject_office_truncate', '', 0, [], True],
    ['workspace_tool_receipts', 'workspace_receipt_at_commit', 'O', 5, True, True, 'public', 'ortak_workspace_receipt_at_commit', '', 0, [], True],
]

FUNCTION_DEFAULTS = [
    ['ortak_lock_run_workspace', 1, 'true'],
    ['ortak_run_workspace_current', 1, 'true'],
    ['ortak_workspace_action_at_commit', 0, None],
    ['ortak_workspace_action_guard', 0, None],
    ['ortak_workspace_activation_at_commit', 0, None],
    ['ortak_workspace_binding_guard', 0, None],
    ['ortak_workspace_canonical', 0, None],
    ['ortak_workspace_manifest_consistent', 0, None],
    ['ortak_workspace_profile_available', 0, None],
    ['ortak_workspace_reader_cancel_fence', 0, None],
    ['ortak_workspace_reader_guard', 0, None],
    ['ortak_workspace_receipt_at_commit', 0, None],
    ['ortak_workspace_run_admission', 0, None],
    ['ortak_workspace_use_at_commit', 0, None],
]

COLUMNS = [
    ['run_workspace_uses', 'admission_lease', 'uuid', True, '', '', None, None],
    ['run_workspace_uses', 'community_id', 'uuid', True, '', '', None, None],
    ['run_workspace_uses', 'company_id', 'uuid', True, '', '', None, None],
    ['run_workspace_uses', 'employee_lifecycle_epoch', 'bigint', True, '', '', None, None],
    ['run_workspace_uses', 'employee_revision_id', 'uuid', True, '', '', None, None],
    ['run_workspace_uses', 'manifest_hash', 'bytea', True, '', '', None, None],
    ['run_workspace_uses', 'outbox_id', 'uuid', True, '', '', None, None],
    ['run_workspace_uses', 'prepared_at', 'timestamp with time zone', True, '', '', 'clock_timestamp()', None],
    ['run_workspace_uses', 'run_id', 'uuid', True, '', '', None, None],
    ['run_workspace_uses', 'store_ref', 'text', True, '', '', None, 'default'],
    ['run_workspace_uses', 'workspace_id', 'uuid', True, '', '', None, None],
    ['workspace_bindings', 'community_id', 'uuid', True, '', '', None, None],
    ['workspace_bindings', 'company_id', 'uuid', True, '', '', None, None],
    ['workspace_bindings', 'created_at', 'timestamp with time zone', True, '', '', 'clock_timestamp()', None],
    ['workspace_bindings', 'employee_id', 'text', True, '', '', None, 'default'],
    ['workspace_bindings', 'expires_at', 'timestamp with time zone', True, '', '', None, None],
    ['workspace_bindings', 'grant_bytes', 'bytea', True, '', '', None, None],
    ['workspace_bindings', 'id', 'uuid', True, '', '', None, None],
    ['workspace_bindings', 'manifest_hash', 'bytea', True, '', '', None, None],
    ['workspace_bindings', 'project_id', 'uuid', True, '', '', None, None],
    ['workspace_bindings', 'revoked_at', 'timestamp with time zone', False, '', '', None, None],
    ['workspace_bindings', 'verification_id', 'uuid', True, '', '', None, None],
    ['workspace_bindings', 'verified_at', 'timestamp with time zone', True, '', '', None, None],
    ['workspace_bindings', 'workspace_ref', 'text', True, '', '', None, 'default'],
    ['workspace_files', 'byte_count', 'integer', True, '', '', None, None],
    ['workspace_files', 'community_id', 'uuid', True, '', '', None, None],
    ['workspace_files', 'company_id', 'uuid', True, '', '', None, None],
    ['workspace_files', 'content_hash', 'bytea', True, '', '', None, None],
    ['workspace_files', 'id', 'uuid', True, '', '', None, None],
    ['workspace_files', 'logical_name', 'text', True, '', '', None, 'default'],
    ['workspace_files', 'media_type', 'text', True, '', '', None, 'default'],
    ['workspace_files', 'ordinal', 'integer', True, '', '', None, None],
    ['workspace_files', 'workspace_id', 'uuid', True, '', '', None, None],
    ['workspace_reader_executions', 'community_id', 'uuid', True, '', '', None, None],
    ['workspace_reader_executions', 'company_id', 'uuid', True, '', '', None, None],
    ['workspace_reader_executions', 'created_at', 'timestamp with time zone', True, '', '', 'clock_timestamp()', None],
    ['workspace_reader_executions', 'executable', 'text', False, '', '', None, 'default'],
    ['workspace_reader_executions', 'executable_hash', 'bytea', False, '', '', None, None],
    ['workspace_reader_executions', 'id', 'uuid', True, '', '', None, None],
    ['workspace_reader_executions', 'operating_uid', 'bigint', False, '', '', None, None],
    ['workspace_reader_executions', 'owner_deadline', 'timestamp with time zone', True, '', '', None, None],
    ['workspace_reader_executions', 'owner_lease', 'uuid', True, '', '', None, None],
    ['workspace_reader_executions', 'pid', 'bigint', False, '', '', None, None],
    ['workspace_reader_executions', 'request_key', 'text', True, '', '', None, 'default'],
    ['workspace_reader_executions', 'run_id', 'uuid', True, '', '', None, None],
    ['workspace_reader_executions', 'state', 'text', True, '', '', "'planned'::text", 'default'],
    ['workspace_reader_executions', 'stop_proof', 'text', False, '', '', None, 'default'],
    ['workspace_reader_executions', 'stopped_at', 'timestamp with time zone', False, '', '', None, None],
    ['workspace_reader_executions', 'workspace_id', 'uuid', True, '', '', None, None],
    ['workspace_tool_actions', 'arguments_hash', 'bytea', True, '', '', None, None],
    ['workspace_tool_actions', 'attempt_count', 'integer', True, '', '', '0', None],
    ['workspace_tool_actions', 'call_id', 'text', True, '', '', None, 'default'],
    ['workspace_tool_actions', 'community_id', 'uuid', True, '', '', None, None],
    ['workspace_tool_actions', 'company_id', 'uuid', True, '', '', None, None],
    ['workspace_tool_actions', 'created_at', 'timestamp with time zone', True, '', '', 'clock_timestamp()', None],
    ['workspace_tool_actions', 'file_id', 'uuid', True, '', '', None, None],
    ['workspace_tool_actions', 'lease_expires_at', 'timestamp with time zone', False, '', '', None, None],
    ['workspace_tool_actions', 'lease_token', 'uuid', False, '', '', None, None],
    ['workspace_tool_actions', 'next_attempt_at', 'timestamp with time zone', True, '', '', 'clock_timestamp()', None],
    ['workspace_tool_actions', 'ordinal', 'integer', True, '', '', None, None],
    ['workspace_tool_actions', 'run_id', 'uuid', True, '', '', None, None],
    ['workspace_tool_actions', 'state', 'text', True, '', '', "'pending'::text", 'default'],
    ['workspace_tool_actions', 'updated_at', 'timestamp with time zone', True, '', '', 'clock_timestamp()', None],
    ['workspace_tool_receipts', 'arguments_hash', 'bytea', True, '', '', None, None],
    ['workspace_tool_receipts', 'attempt_count', 'integer', True, '', '', None, None],
    ['workspace_tool_receipts', 'call_id', 'text', True, '', '', None, 'default'],
    ['workspace_tool_receipts', 'community_id', 'uuid', True, '', '', None, None],
    ['workspace_tool_receipts', 'company_id', 'uuid', True, '', '', None, None],
    ['workspace_tool_receipts', 'created_at', 'timestamp with time zone', True, '', '', 'clock_timestamp()', None],
    ['workspace_tool_receipts', 'lease_token', 'uuid', True, '', '', None, None],
    ['workspace_tool_receipts', 'result_bytes', 'bytea', True, '', '', None, None],
    ['workspace_tool_receipts', 'result_hash', 'bytea', True, '', '', None, None],
    ['workspace_tool_receipts', 'run_id', 'uuid', True, '', '', None, None],
]

CHECKS = [
    ['run_workspace_uses', 'run_workspace_uses_check', 'c', "CHECK ((store_ref = ((('workspace-run:'::text || (company_id)::text) || ':'::text) || (run_id)::text)))", True, False, False],
    ['run_workspace_uses', 'run_workspace_uses_employee_lifecycle_epoch_check', 'c', 'CHECK ((employee_lifecycle_epoch >= 0))', True, False, False],
    ['run_workspace_uses', 'run_workspace_uses_manifest_hash_check', 'c', 'CHECK ((octet_length(manifest_hash) = 32))', True, False, False],
    ['run_workspace_uses', 'run_workspace_uses_store_ref_check', 'c', 'CHECK ((octet_length(store_ref) <= 128))', True, False, False],
    ['workspace_bindings', 'workspace_bindings_check', 'c', 'CHECK (((expires_at > verified_at) AND (verified_at <= created_at)))', True, False, False],
    ['workspace_bindings', 'workspace_bindings_grant_bytes_check', 'c', 'CHECK (((octet_length(grant_bytes) >= 1) AND (octet_length(grant_bytes) <= 16384)))', True, False, False],
    ['workspace_bindings', 'workspace_bindings_manifest_hash_check', 'c', 'CHECK ((octet_length(manifest_hash) = 32))', True, False, False],
    ['workspace_bindings', 'workspace_bindings_workspace_ref_check', 'c', "CHECK ((workspace_ref ~ '^[A-Za-z0-9][A-Za-z0-9._:-]{0,127}$'::text))", True, False, False],
    ['workspace_files', 'workspace_files_byte_count_check', 'c', 'CHECK (((byte_count >= 0) AND (byte_count <= 16384)))', True, False, False],
    ['workspace_files', 'workspace_files_content_hash_check', 'c', 'CHECK ((octet_length(content_hash) = 32))', True, False, False],
    ['workspace_files', 'workspace_files_logical_name_check', 'c', "CHECK ((((octet_length(logical_name) >= 1) AND (octet_length(logical_name) <= 256)) AND (logical_name ~ '^[A-Za-z0-9][A-Za-z0-9._/-]*$'::text) AND (logical_name !~ '(^|/)(\\.|\\.\\.|)(/|$)'::text)))", True, False, False],
    ['workspace_files', 'workspace_files_media_type_check', 'c', "CHECK ((media_type = 'text/plain'::text))", True, False, False],
    ['workspace_files', 'workspace_files_ordinal_check', 'c', 'CHECK (((ordinal >= 0) AND (ordinal <= 7)))', True, False, False],
    ['workspace_reader_executions', 'workspace_reader_executions_check', 'c', 'CHECK ((((executable IS NULL) = (executable_hash IS NULL)) AND ((executable IS NULL) = (operating_uid IS NULL))))', True, False, False],
    ['workspace_reader_executions', 'workspace_reader_executions_check1', 'c', 'CHECK (((executable IS NULL) OR (((octet_length(executable) >= 1) AND (octet_length(executable) <= 4096)) AND ("left"(executable, 1) = \'/\'::text) AND (octet_length(executable_hash) = 32) AND ((operating_uid >= 0) AND (operating_uid <= \'4294967295\'::bigint)))))', True, False, False],
    ['workspace_reader_executions', 'workspace_reader_executions_check2', 'c', "CHECK ((((state = 'stopped'::text) = (stopped_at IS NOT NULL)) AND ((state = 'stopped'::text) = (stop_proof IS NOT NULL))))", True, False, False],
    ['workspace_reader_executions', 'workspace_reader_executions_check3', 'c', "CHECK (((stop_proof IS NULL) OR ((stop_proof = 'in_process_returned'::text) = (executable IS NULL))))", True, False, False],
    ['workspace_reader_executions', 'workspace_reader_executions_pid_check', 'c', "CHECK (((pid >= 1) AND (pid <= '4294967295'::bigint)))", True, False, False],
    ['workspace_reader_executions', 'workspace_reader_executions_request_key_check', 'c', 'CHECK (((octet_length(request_key) >= 1) AND (octet_length(request_key) <= 160)))', True, False, False],
    ['workspace_reader_executions', 'workspace_reader_executions_state_check', 'c', "CHECK ((state = ANY (ARRAY['planned'::text, 'running'::text, 'stopped'::text])))", True, False, False],
    ['workspace_reader_executions', 'workspace_reader_executions_stop_proof_check', 'c', "CHECK ((stop_proof = ANY (ARRAY['reaped'::text, 'in_process_returned'::text, 'confirmed_absence'::text])))", True, False, False],
    ['workspace_tool_actions', 'workspace_tool_actions_arguments_hash_check', 'c', 'CHECK ((octet_length(arguments_hash) = 32))', True, False, False],
    ['workspace_tool_actions', 'workspace_tool_actions_attempt_count_check', 'c', 'CHECK (((attempt_count >= 0) AND (attempt_count <= 3)))', True, False, False],
    ['workspace_tool_actions', 'workspace_tool_actions_call_id_check', 'c', "CHECK ((call_id ~ '^[A-Za-z0-9][A-Za-z0-9_.:-]{0,127}$'::text))", True, False, False],
    ['workspace_tool_actions', 'workspace_tool_actions_check', 'c', 'CHECK (((lease_token IS NULL) = (lease_expires_at IS NULL)))', True, False, False],
    ['workspace_tool_actions', 'workspace_tool_actions_check1', 'c', 'CHECK ((arguments_hash = sha256(convert_to(((\'{"file_id":"\'::text || (file_id)::text) || \'"}\'::text), \'UTF8\'::name))))', True, False, False],
    ['workspace_tool_actions', 'workspace_tool_actions_ordinal_check', 'c', 'CHECK (((ordinal >= 1) AND (ordinal <= 4)))', True, False, False],
    ['workspace_tool_actions', 'workspace_tool_actions_state_check', 'c', "CHECK ((state = ANY (ARRAY['pending'::text, 'result_ready'::text, 'delivered'::text, 'interrupted'::text])))", True, False, False],
    ['workspace_tool_receipts', 'workspace_tool_receipts_arguments_hash_check', 'c', 'CHECK ((octet_length(arguments_hash) = 32))', True, False, False],
    ['workspace_tool_receipts', 'workspace_tool_receipts_attempt_count_check', 'c', 'CHECK (((attempt_count >= 1) AND (attempt_count <= 3)))', True, False, False],
    ['workspace_tool_receipts', 'workspace_tool_receipts_check', 'c', 'CHECK (((octet_length(result_hash) = 32) AND (result_hash = sha256(result_bytes))))', True, False, False],
    ['workspace_tool_receipts', 'workspace_tool_receipts_result_bytes_check', 'c', 'CHECK (((octet_length(result_bytes) >= 1) AND (octet_length(result_bytes) <= 131072)))', True, False, False],
]

UNIQUE_INDEXES = [
    ['run_workspace_uses', 'run_workspace_uses_pkey', 'CREATE UNIQUE INDEX run_workspace_uses_pkey ON public.run_workspace_uses USING btree (company_id, run_id)', '[0:1]={0,0}', True, True, True, True, False],
    ['workspace_bindings', 'workspace_bindings_company_id_verification_id_key', 'CREATE UNIQUE INDEX workspace_bindings_company_id_verification_id_key ON public.workspace_bindings USING btree (company_id, verification_id)', '[0:1]={0,0}', True, True, True, False, False],
    ['workspace_bindings', 'workspace_bindings_pkey', 'CREATE UNIQUE INDEX workspace_bindings_pkey ON public.workspace_bindings USING btree (company_id, id)', '[0:1]={0,0}', True, True, True, True, False],
    ['workspace_files', 'workspace_files_company_id_workspace_id_logical_name_key', 'CREATE UNIQUE INDEX workspace_files_company_id_workspace_id_logical_name_key ON public.workspace_files USING btree (company_id, workspace_id, logical_name)', '[0:2]={0,0,0}', True, True, True, False, False],
    ['workspace_files', 'workspace_files_company_id_workspace_id_ordinal_key', 'CREATE UNIQUE INDEX workspace_files_company_id_workspace_id_ordinal_key ON public.workspace_files USING btree (company_id, workspace_id, ordinal)', '[0:2]={0,0,0}', True, True, True, False, False],
    ['workspace_files', 'workspace_files_pkey', 'CREATE UNIQUE INDEX workspace_files_pkey ON public.workspace_files USING btree (company_id, workspace_id, id)', '[0:2]={0,0,0}', True, True, True, True, False],
    ['workspace_reader_executions', 'idx_workspace_reader_attempt', 'CREATE UNIQUE INDEX idx_workspace_reader_attempt ON public.workspace_reader_executions USING btree (company_id, run_id, request_key, owner_lease)', '[0:3]={0,0,0,0}', True, True, True, False, False],
    ['workspace_reader_executions', 'idx_workspace_reader_one_unresolved', "CREATE UNIQUE INDEX idx_workspace_reader_one_unresolved ON public.workspace_reader_executions USING btree (company_id, run_id) WHERE (state <> 'stopped'::text)", '[0:1]={0,0}', True, True, True, False, False],
    ['workspace_reader_executions', 'workspace_reader_executions_pkey', 'CREATE UNIQUE INDEX workspace_reader_executions_pkey ON public.workspace_reader_executions USING btree (company_id, id)', '[0:1]={0,0}', True, True, True, True, False],
    ['workspace_tool_actions', 'idx_workspace_actions_one_pending', "CREATE UNIQUE INDEX idx_workspace_actions_one_pending ON public.workspace_tool_actions USING btree (company_id, run_id) WHERE (state = 'pending'::text)", '[0:1]={0,0}', True, True, True, False, False],
    ['workspace_tool_actions', 'workspace_tool_actions_company_id_run_id_ordinal_key', 'CREATE UNIQUE INDEX workspace_tool_actions_company_id_run_id_ordinal_key ON public.workspace_tool_actions USING btree (company_id, run_id, ordinal)', '[0:2]={0,0,0}', True, True, True, False, False],
    ['workspace_tool_actions', 'workspace_tool_actions_pkey', 'CREATE UNIQUE INDEX workspace_tool_actions_pkey ON public.workspace_tool_actions USING btree (company_id, run_id, call_id)', '[0:2]={0,0,0}', True, True, True, True, False],
    ['workspace_tool_receipts', 'workspace_tool_receipts_pkey', 'CREATE UNIQUE INDEX workspace_tool_receipts_pkey ON public.workspace_tool_receipts USING btree (company_id, run_id, call_id)', '[0:2]={0,0,0}', True, True, True, True, False],
]

def check(value, refused):
    """Require the reviewed C2 contract before comparing the two full catalogs."""
    if value.get('workspace_triggers') != TRIGGERS:
        raise refused('workspace_trigger_contract_differs')
    triggers = {(row[0], row[1]): row for row in value['triggers']}
    for expected in TRIGGERS:
        actual = triggers.get(tuple(expected[:2]))
        if actual is None or actual[:6] != expected[:6]:
            raise refused('workspace_trigger_contract_differs')
    fences = sorted(row[:6] for row in value['fence_targets'] if row[0] in TABLES)
    expected_fences = [row[:6] for row in TRIGGERS if row[7] == 'enforce_community_write_fence']
    if fences != expected_fences:
        raise refused('workspace_community_fence_missing')
    functions = {row[0]: row for row in value['functions']}
    for name, metadata in FUNCTIONS.items():
        row = functions.get(name)
        if row is None or len(row) != 11 or row[1:10] != metadata or not row[10]:
            raise refused('workspace_function_contract_differs')
    if value.get('workspace_function_defaults') != FUNCTION_DEFAULTS:
        raise refused('workspace_function_default_differs')
    columns = sorted(row for row in value['columns'] if row[0] in TABLES)
    if columns != COLUMNS:
        raise refused('workspace_column_contract_differs')
    checks = sorted(row for row in value['constraints'] if row[0] in TABLES and row[2] == 'c')
    if checks != CHECKS:
        raise refused('workspace_check_contract_differs')
    indexes = sorted(row for row in value['indexes'] if row[0] in TABLES and row[6])
    if indexes != UNIQUE_INDEXES:
        raise refused('workspace_unique_index_contract_differs')
