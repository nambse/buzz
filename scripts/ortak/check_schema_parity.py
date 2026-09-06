#!/usr/bin/env python3
"""Probe migration 79 versus real pgschema on disposable port 55432; retain both DBs.

Requires Python >=3.11 with psycopg2, the cached pgschema 1.7.4, and an explicitly
selected compiled buzz-db test binary. Reads only ORTAK_SCHEMA_PARITY_TEST_URL.
Never use live port 55433. Receipt and bounded command logs stay owner-private.
"""
import argparse
from datetime import datetime, timezone
import hashlib
import json
import multiprocessing
import os
from pathlib import Path
import re
import selectors
import signal
import stat
import subprocess
import time
from urllib.parse import unquote, urlsplit
from uuid import uuid4

import workspace_catalog
import conversation_catalog
import extensions77_catalog
import context79_catalog

PGSCHEMA = Path("/Users/nambse/Library/Caches/hermit/pkg/pgschema-1.7.4/pgschema")
URL_ENV = "ORTAK_SCHEMA_PARITY_TEST_URL"
REPO = Path(__file__).resolve().parents[2]
TEST = "runtime::migration::postgres_tests::run_migrations_applies_consolidated_initial_schema_on_fresh_database"
MAX_OUTPUT = 4 * 1024 * 1024
MAX_SECONDS = 300
MAX_SQL_SECONDS = 30
TABLES = ["project_api_bindings", "project_access_grants", "work_api_operations", "office_company_bindings",
          "provisioning_operations", "provisioning_operation_steps", "office_identity_profiles", "provisioning_runner_selections", "office_routing_cohorts", "office_routing_channels",
          "office_routing_employees", "office_inbox_reconciliations", "work_acceptance_criteria", "work_item_history"]
FUNCTIONS = ["ortak_check_routing_claim_expiry", "ortak_check_work_api_receipt", "ortak_project_access_guard",
             "ortak_assert_project_binding_purge", "ortak_guard_project_api_binding", "ortak_project_binding_purge_at_commit",
             "ortak_check_activation_admission_at_commit", "ortak_guard_activation_operation", "ortak_guard_activation_receipt",
             "ortak_office_profile_receipt_immutable", "ortak_provisioning_selection_immutable", "ortak_guard_routing_cohort_state",
             "ortak_invalidate_routing_capture", "ortak_guard_inbox_reconciliation", "ortak_activity_notify", "ortak_guard_retained_office_authority", "work_acceptance_criteria_guard", "work_definition_criterion_history_guard"]

TABLES += ['projects', 'work_items', 'work_assignments', 'work_dependencies', 'work_approvals', 'work_attachments', 'work_authority_generations', 'work_executions', 'artifacts', 'runtime_work_outputs', 'runs', 'outbox', 'runtime_cancellations', 'employee_management_policies', 'prepared_employee_catalog', 'employee_configuration_drafts', 'employee_management_commands', 'employee_management_audit']
FUNCTIONS += ['ortak_work_generation_guard', 'ortak_advance_work_authority', 'ortak_work_child_authority_guard', 'ortak_work_execution_guard', 'ortak_work_output_guard', 'ortak_schedule_completed_office_output', 'ortak_schedule_work_output', 'ortak_check_work_execution_request', 'ortak_work_run_identity_guard', 'ortak_check_run_work_authority', 'ortak_check_work_output_provenance', 'ortak_management_immutable', 'ortak_management_actor_allowed', 'ortak_management_guard', 'ortak_management_operation_fence']

TABLES += ['employees', 'routing_recipients', 'employee_lifecycle_events', 'runtime_office_outputs', 'runtime_memory_writes']
FUNCTIONS += ['ortak_guard_lifecycle_event_insert', 'ortak_pin_employee_lifecycle', 'ortak_check_run_lifecycle', 'ortak_check_provisioning_lifecycle', 'ortak_guard_employee_lifecycle', 'ortak_check_lifecycle_activation', 'ortak_check_output_lifecycle']
FUNCTIONS += ['ortak_work_dependency_edit_guard']
FUNCTIONS += ['ortak_conversation_plaintext79', 'ortak_run_conversation_context_current', 'ortak_conversation_snapshot_admission79']
TABLES += ['provisioning_runtime_probes']
FUNCTIONS += ['ortak_provisioning_runtime_probe_guard']
TABLES += ['reviewed_memory_facts', 'reviewed_memory_operations']
FUNCTIONS += ['ortak_reviewed_fact_source_visible', 'ortak_reviewed_fact_guard', 'ortak_reviewed_fact_receipt_at_commit', 'ortak_reviewed_memory_operation_at_commit']
EXPORT_TABLES = ['reviewed_memory_targets', 'reviewed_memory_exports', 'reviewed_memory_export_jobs',
                 'reviewed_memory_export_commands', 'reviewed_memory_export_receipts']
TABLES += EXPORT_TABLES
FUNCTIONS += ['ortak_reviewed_export_source_hash', 'ortak_reviewed_export_eligible', 'ortak_reviewed_target_guard',
              'ortak_reviewed_export_at_commit', 'ortak_reviewed_export_stop', 'ortak_reviewed_export_job_guard',
              'ortak_reviewed_export_job_at_commit', 'ortak_reviewed_export_command_at_commit',
              'ortak_reviewed_export_receipt_at_commit', 'ortak_reviewed_export_view']

TABLES += ['work_decomposition', 'run_reviewed_memory_uses', 'run_context_snapshots']
FUNCTIONS += ['ortak_work_decomposition_reserve', 'ortak_work_decomposition_commit',
              'ortak_reviewed_runtime_eligible', 'ortak_run_reviewed_memory_current',
              'ortak_lock_run_reviewed_memory', 'ortak_reviewed_use_immutable',
              'ortak_reviewed_snapshot_consistent', 'ortak_reviewed_run_admission']
FUNCTIONS += ['ortak_snapshot_scratch_jsonb', 'ortak_private_dm_identity', 'ortak_fence_office_mutation']
DIRECT_AUTHORITY_ARGS = ('community', 'community_id', 'id', 'channel_type', 'visibility',
                         'archived_at', 'deleted_at', 'participant_hash', 'ttl_seconds', 'ttl_deadline')
# Catalog identity/language/volatility/strict/security/leakproof/parallel/config/result.
# Equal but weakened metadata on both databases must also fail the proof.
SNAPSHOT_DM_FUNCTIONS = {
    'ortak_snapshot_scratch_jsonb': ['value json', 'sql', 'i', True, False, False, 's', None, 'jsonb'],
    'ortak_reviewed_snapshot_consistent': ['', 'plpgsql', 'v', False, False, False, 'u', None, 'trigger'],
    'ortak_private_dm_identity': ['', 'plpgsql', 'v', False, False, False, 'u', None, 'trigger'],
    'ortak_fence_office_mutation': ['', 'plpgsql', 'v', False, False, False, 'u', None, 'trigger'],
}
ACTIVITY_GUARDS = (
    ('run_events', 'trg_activity_events', 5),
    ('runs', 'trg_activity_runs', 21),
    ('run_cancel_requests', 'trg_activity_cancel_requests', 21),
    ('runtime_cancellations', 'trg_activity_cancellations', 21),
    ('runtime_office_outputs', 'trg_activity_office_outputs', 21),
    ('outbox', 'trg_activity_outbox', 21),
    ('runtime_memory_writes', 'trg_activity_memory_writes', 21),
    ('run_context_snapshots', 'trg_activity_context', 5),
    ('office_authority_generations', 'trg_activity_authority', 21),
    ('work_authority_generations', 'trg_activity_work_authority', 21),
    ('runtime_work_outputs', 'trg_activity_work_outputs', 21),
    ('reviewed_memory_facts', 'trg_activity_reviewed_fact_use', 17),
    ('reviewed_memory_targets', 'trg_activity_reviewed_target_use', 17),
)
INTEGRATION_CHECKS = (
    ('work_decomposition', 'work_decomposition_child_id_check', "CHECK ((child_id <> '00000000-0000-0000-0000-000000000000'::uuid))"),
    ('work_decomposition', 'work_decomposition_parent_version_check', 'CHECK ((parent_version > 1))'),
    ('work_decomposition', 'work_decomposition_depth_check', 'CHECK (((depth >= 1) AND (depth <= 8)))'),
    ('work_decomposition', 'work_decomposition_actor_pubkey_check', "CHECK ((actor_pubkey ~ '^[0-9a-f]{64}$'::text))"),
    ('work_decomposition', 'work_decomposition_check', 'CHECK ((parent_id <> child_id))'),
    ('reviewed_memory_targets', 'reviewed_memory_targets_consumption_epoch_check', 'CHECK ((consumption_epoch >= 0))'),
    ('run_reviewed_memory_uses', 'run_reviewed_memory_uses_ordinal_check', 'CHECK (((ordinal >= 0) AND (ordinal <= 7)))'),
    ('run_reviewed_memory_uses', 'run_reviewed_memory_uses_fact_version_check', 'CHECK ((fact_version = 1))'),
    ('run_reviewed_memory_uses', 'run_reviewed_memory_uses_consumption_epoch_check', 'CHECK ((consumption_epoch >= 0))'),
    ('run_reviewed_memory_uses', 'run_reviewed_memory_uses_content_hash_check', 'CHECK ((octet_length(content_hash) = 32))'),
    ('run_reviewed_memory_uses', 'run_reviewed_memory_uses_source_hash_check', 'CHECK ((octet_length(source_hash) = 32))'),
    ('run_reviewed_memory_uses', 'run_reviewed_memory_uses_binding_hash_check', 'CHECK ((octet_length(binding_hash) = 32))'),
    ('run_reviewed_memory_uses', 'run_reviewed_memory_uses_approved_by_check', "CHECK ((approved_by ~ '^[0-9a-f]{64}$'::text))"),
)

TABLES += ['workspace_bindings', 'workspace_files', 'run_workspace_uses', 'workspace_tool_actions', 'workspace_tool_receipts', 'workspace_reader_executions']
FUNCTIONS += ['ortak_workspace_canonical', 'ortak_workspace_binding_guard', 'ortak_workspace_manifest_consistent', 'ortak_workspace_profile_available', 'ortak_workspace_activation_at_commit', 'ortak_run_workspace_current', 'ortak_lock_run_workspace', 'ortak_workspace_use_at_commit', 'ortak_workspace_action_guard', 'ortak_workspace_action_at_commit', 'ortak_workspace_receipt_at_commit', 'ortak_workspace_run_admission', 'ortak_workspace_reader_guard', 'ortak_workspace_reader_cancel_fence']

CONVERSATION_TABLES = ['conversation_memory_authorities', 'reviewed_memory_conversation_audiences']
TABLES += CONVERSATION_TABLES
FUNCTIONS += ['ortak_conversation_json75', 'ortak_conversation_source_observation',
              'ortak_conversation_scope_current', 'ortak_conversation_authority_guard',
              'ortak_register_conversation_authority', 'ortak_conversation_fact_storage_at_commit',
              'ortak_conversation_use_storage_at_commit', 'ortak_conversation_thread_insert_neutral75',
              'ortak_advance_conversation_scopes75', 'ortak_conversation_epoch_mutation75']
FUNCTIONS += ['ortak_conversation_run_origin', 'ortak_conversation_target_eligible76',
              'ortak_conversation_export_eligible', 'ortak_conversation_runtime_eligible',
              'ortak_conversation_effect_admission76', 'ortak_conversation_snapshot76']
TABLES += list(extensions77_catalog.TABLES)
FUNCTIONS += [name for name in extensions77_catalog.FUNCTION_NAMES if name not in FUNCTIONS]

# Columns are keyed by name: ALTER versus inline creation can reorder physical
# attnums. Ordered index/constraint keys, sort/null options and all deferred
# flags remain exact. Function bodies and catalog-rendered SQL are not rewritten.
CATALOG = r"""
WITH RECURSIVE selected AS (
 SELECT c.oid,c.relname FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace
 WHERE n.nspname='public' AND c.relname=ANY(%s)
), functions AS (
 SELECT p.*,l.lanname FROM pg_proc p JOIN pg_namespace n ON n.oid=p.pronamespace
 JOIN pg_language l ON l.oid=p.prolang
 WHERE n.nspname='public' AND p.proname=ANY(%s)
), conversation_event_relations AS (
 SELECT c.oid,n.nspname,c.relname,NULL::name AS parent_schema,NULL::name AS parent_name,
   c.relkind,c.relispartition,pg_get_expr(c.relpartbound,c.oid,false) AS partition_bound
 FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace
 WHERE c.oid='public.events'::regclass
 UNION ALL
 SELECT c.oid,n.nspname,c.relname,parent.nspname,parent.relname,
   c.relkind,c.relispartition,pg_get_expr(c.relpartbound,c.oid,false)
 FROM conversation_event_relations parent JOIN pg_inherits h ON h.inhparent=parent.oid
 JOIN pg_class c ON c.oid=h.inhrelid JOIN pg_namespace n ON n.oid=c.relnamespace
)
SELECT jsonb_build_object(
 'tables',(SELECT jsonb_agg(relname ORDER BY relname) FROM selected),
 'columns',(SELECT jsonb_agg(jsonb_build_array(c.relname,a.attname,format_type(a.atttypid,a.atttypmod),
   a.attnotnull,a.attidentity,a.attgenerated,pg_get_expr(d.adbin,d.adrelid),coll.collname)
   ORDER BY c.relname,a.attname) FROM selected c JOIN pg_attribute a ON a.attrelid=c.oid
   LEFT JOIN pg_attrdef d ON d.adrelid=a.attrelid AND d.adnum=a.attnum
   LEFT JOIN pg_collation coll ON coll.oid=a.attcollation
   WHERE a.attnum>0 AND NOT a.attisdropped),
 'indexes',(SELECT jsonb_agg(jsonb_build_array(c.relname,ic.relname,pg_get_indexdef(i.indexrelid),
   i.indoption::int2[]::text,i.indisvalid,i.indisready,i.indisunique,i.indisprimary,i.indisexclusion)
   ORDER BY c.relname,ic.relname) FROM selected c JOIN pg_index i ON i.indrelid=c.oid
   JOIN pg_class ic ON ic.oid=i.indexrelid),
 'constraints',(SELECT jsonb_agg(jsonb_build_array(c.relname,k.conname,k.contype,
   pg_get_constraintdef(k.oid,false),k.convalidated,k.condeferrable,k.condeferred)
   ORDER BY c.relname,k.conname) FROM selected c JOIN pg_constraint k ON k.conrelid=c.oid),
 'triggers',(SELECT jsonb_agg(jsonb_build_array(c.relname,t.tgname,t.tgenabled,t.tgtype,
   t.tgdeferrable,t.tginitdeferred,pg_get_triggerdef(t.oid,false)) ORDER BY c.relname,t.tgname)
   FROM pg_trigger t JOIN pg_class c ON c.oid=t.tgrelid JOIN pg_namespace n ON n.oid=c.relnamespace
   WHERE n.nspname='public' AND NOT t.tgisinternal AND
    (t.tgfoid='ortak_activity_notify()'::regprocedure OR c.relname=ANY(%s)
      OR (c.relname='channels' AND t.tgname IN('ortak_private_dm_identity','ortak_office_authority_channels'))
      OR left(t.tgname,13)='conversation_'
      OR t.tgname='ortak_z_conversation_epoch_communities'
      OR left(t.tgname,16)='employee_memory_'
      OR left(t.tgname,18)='employee_reviewed_'
      OR left(t.tgname,13)='confidential_'
      OR left(t.tgname,13)='encrypted_dm_'
      OR t.tgname IN('ortak_z_employee_memory_epoch_communities',
        'trg_routing_decisions_notify','trg_routing_authority_notify')
      OR (c.relname='thread_metadata' AND t.tgname='ortak_office_authority_thread_metadata')
      OR (c.relname='routing_decisions' AND t.tgname='ortak_routing_claim_expiry_at_commit'))),
 'direct_authority',(SELECT jsonb_agg(jsonb_build_array(t.tgname,p.proname,encode(t.tgargs,'hex'),
   t.tgnargs,t.tgattr::text,t.tgqual IS NULL) ORDER BY t.tgname)
   FROM pg_trigger t JOIN pg_proc p ON p.oid=t.tgfoid JOIN pg_namespace n ON n.oid=p.pronamespace
   WHERE t.tgrelid='channels'::regclass AND NOT t.tgisinternal AND n.nspname='public'
     AND t.tgname IN('ortak_private_dm_identity','ortak_office_authority_channels')),
 'workspace_triggers',(SELECT jsonb_agg(jsonb_build_array(c.relname,t.tgname,t.tgenabled,t.tgtype,
   t.tgdeferrable,t.tginitdeferred,pn.nspname,p.proname,encode(t.tgargs,'hex'),t.tgnargs,
   ARRAY(SELECT a.attname FROM unnest(t.tgattr::smallint[]) WITH ORDINALITY k(attnum,ord)
     JOIN pg_attribute a ON a.attrelid=t.tgrelid AND a.attnum=k.attnum ORDER BY ord),t.tgqual IS NULL)
   ORDER BY c.relname,t.tgname)
   FROM pg_trigger t JOIN pg_class c ON c.oid=t.tgrelid JOIN pg_namespace n ON n.oid=c.relnamespace
   JOIN pg_proc p ON p.oid=t.tgfoid JOIN pg_namespace pn ON pn.oid=p.pronamespace
   WHERE n.nspname='public' AND NOT t.tgisinternal AND
    (c.relname IN('workspace_bindings','workspace_files','run_workspace_uses','workspace_tool_actions',
      'workspace_tool_receipts','workspace_reader_executions') OR t.tgname IN('workspace_activation_at_commit',
      'workspace_run_admission','workspace_artifact_admission','workspace_reader_cancel_fence'))
     AND t.tgname NOT IN('confidential_no_workspace_use','confidential_no_workspace_action',
       'confidential_no_workspace_receipt','confidential_no_workspace_reader')),
 'workspace_function_defaults',(SELECT jsonb_agg(jsonb_build_array(proname,pronargdefaults,
   pg_get_expr(proargdefaults,0)) ORDER BY proname) FROM functions
   WHERE position('workspace' in proname)>0),
 'conversation_function_defaults',(SELECT jsonb_agg(jsonb_build_array(proname,pronargdefaults,
   pg_get_expr(proargdefaults,0)) ORDER BY proname) FROM functions
   WHERE position('conversation' in proname)>0 OR proname='ortak_fence_office_mutation'),
 'conversation_event_relations',(SELECT jsonb_agg(jsonb_build_array(nspname,relname,
   parent_schema,parent_name,relkind,relispartition,partition_bound) ORDER BY nspname,relname)
   FROM conversation_event_relations),
 'conversation_triggers',(SELECT jsonb_agg(jsonb_build_array(c.relname,t.tgname,t.tgenabled,t.tgtype,
   t.tgdeferrable,t.tginitdeferred,pn.nspname,p.proname,encode(t.tgargs,'hex'),t.tgnargs,
   ARRAY(SELECT a.attname FROM unnest(t.tgattr::smallint[]) WITH ORDINALITY k(attnum,ord)
     JOIN pg_attribute a ON a.attrelid=t.tgrelid AND a.attnum=k.attnum ORDER BY ord),
   t.tgqual IS NULL,t.tgisinternal,
   CASE WHEN t.tgparentid=0 THEN NULL ELSE jsonb_build_array(parent_n.nspname,parent_c.relname,parent_t.tgname) END)
   ORDER BY c.relname,t.tgname)
   FROM pg_trigger t JOIN pg_class c ON c.oid=t.tgrelid JOIN pg_namespace n ON n.oid=c.relnamespace
   JOIN pg_proc p ON p.oid=t.tgfoid JOIN pg_namespace pn ON pn.oid=p.pronamespace
   LEFT JOIN pg_trigger parent_t ON parent_t.oid=t.tgparentid
   LEFT JOIN pg_class parent_c ON parent_c.oid=parent_t.tgrelid
   LEFT JOIN pg_namespace parent_n ON parent_n.oid=parent_c.relnamespace
   WHERE n.nspname='public' AND (left(t.tgname,13)='conversation_'
     OR t.tgname='ortak_z_conversation_epoch_communities'
     OR t.tgname IN('community_write_fence_conversation_memory_authorities',
       'community_write_fence_reviewed_memory_conversation_audiences')
     OR (c.relname='thread_metadata' AND t.tgname='ortak_office_authority_thread_metadata'))),
 'conversation_indexes',(SELECT jsonb_agg(jsonb_build_array(c.relname,ic.relname,pg_get_indexdef(i.indexrelid),
   i.indoption::int2[]::text,i.indisvalid,i.indisready,i.indisunique,i.indisprimary,i.indisexclusion,
   i.indislive,i.indnkeyatts,i.indnatts,am.amname,ic.reloptions) ORDER BY c.relname,ic.relname)
   FROM pg_index i JOIN pg_class c ON c.oid=i.indrelid JOIN pg_namespace n ON n.oid=c.relnamespace
   JOIN pg_class ic ON ic.oid=i.indexrelid JOIN pg_am am ON am.oid=ic.relam
   WHERE n.nspname='public' AND ic.relname IN('idx_conversation_thread_parent_exact',
     'idx_conversation_thread_root_exact','idx_conversation_office_employee_keys')),
 'extensions77_function_defaults',(SELECT jsonb_agg(jsonb_build_array(proname,pronargdefaults,
   pg_get_expr(proargdefaults,0)) ORDER BY proname) FROM functions
   WHERE proname=ANY(ARRAY[__EXTENSIONS77_FUNCTIONS__])),
 'context79_triggers',(SELECT jsonb_agg(jsonb_build_array(c.relname,t.tgname,t.tgenabled,t.tgtype,
   t.tgdeferrable,t.tginitdeferred,pn.nspname,p.proname,encode(t.tgargs,'hex'),t.tgnargs,
   ARRAY(SELECT a.attname FROM unnest(t.tgattr::smallint[]) WITH ORDINALITY k(attnum,ord)
     JOIN pg_attribute a ON a.attrelid=t.tgrelid AND a.attnum=k.attnum ORDER BY ord),
   t.tgqual IS NULL,t.tgisinternal,
   CASE WHEN t.tgparentid=0 THEN NULL ELSE jsonb_build_array(parent_n.nspname,parent_c.relname,parent_t.tgname) END)
   ORDER BY c.relname,t.tgname)
   FROM pg_trigger t JOIN pg_class c ON c.oid=t.tgrelid JOIN pg_namespace n ON n.oid=c.relnamespace
   JOIN pg_proc p ON p.oid=t.tgfoid JOIN pg_namespace pn ON pn.oid=p.pronamespace
   LEFT JOIN pg_trigger parent_t ON parent_t.oid=t.tgparentid
   LEFT JOIN pg_class parent_c ON parent_c.oid=parent_t.tgrelid
   LEFT JOIN pg_namespace parent_n ON parent_n.oid=parent_c.relnamespace
   WHERE n.nspname='public' AND NOT t.tgisinternal
     AND (t.tgname='ortak_conversation_snapshot_admission79' OR p.proname='ortak_conversation_snapshot_admission79')),
 'extensions77_triggers',(SELECT jsonb_agg(jsonb_build_array(c.relname,t.tgname,t.tgenabled,t.tgtype,
   t.tgdeferrable,t.tginitdeferred,pn.nspname,p.proname,encode(t.tgargs,'hex'),t.tgnargs,
   ARRAY(SELECT a.attname FROM unnest(t.tgattr::smallint[]) WITH ORDINALITY k(attnum,ord)
     JOIN pg_attribute a ON a.attrelid=t.tgrelid AND a.attnum=k.attnum ORDER BY ord),
   t.tgqual IS NULL,t.tgisinternal,
   CASE WHEN t.tgparentid=0 THEN NULL ELSE jsonb_build_array(parent_n.nspname,parent_c.relname,parent_t.tgname) END)
   ORDER BY c.relname,t.tgname)
   FROM pg_trigger t JOIN pg_class c ON c.oid=t.tgrelid JOIN pg_namespace n ON n.oid=c.relnamespace
   JOIN pg_proc p ON p.oid=t.tgfoid JOIN pg_namespace pn ON pn.oid=p.pronamespace
   LEFT JOIN pg_trigger parent_t ON parent_t.oid=t.tgparentid
   LEFT JOIN pg_class parent_c ON parent_c.oid=parent_t.tgrelid
   LEFT JOIN pg_namespace parent_n ON parent_n.oid=parent_c.relnamespace
   WHERE n.nspname='public' AND NOT t.tgisinternal AND
    (c.relname=ANY(ARRAY[__EXTENSIONS77_TABLES__])
      OR left(t.tgname,16)='employee_memory_' OR left(t.tgname,18)='employee_reviewed_'
      OR left(t.tgname,13)='confidential_' OR left(t.tgname,13)='encrypted_dm_'
      OR t.tgname IN('ortak_z_employee_memory_epoch_communities',
        'trg_routing_decisions_notify','trg_routing_authority_notify'))),
 'extensions77_indexes',(SELECT jsonb_agg(jsonb_build_array(c.relname,ic.relname,pg_get_indexdef(i.indexrelid),
   i.indoption::int2[]::text,i.indisvalid,i.indisready,i.indisunique,i.indisprimary,i.indisexclusion,
   i.indislive,i.indnkeyatts,i.indnatts,am.amname,ic.reloptions) ORDER BY c.relname,ic.relname)
   FROM pg_index i JOIN pg_class c ON c.oid=i.indrelid JOIN pg_namespace n ON n.oid=c.relnamespace
   JOIN pg_class ic ON ic.oid=i.indexrelid JOIN pg_am am ON am.oid=ic.relam
   WHERE n.nspname='public' AND c.relname=ANY(ARRAY[__EXTENSIONS77_TABLES__])),
 'functions',(SELECT jsonb_agg(jsonb_build_array(proname,pg_get_function_identity_arguments(oid),
   lanname,provolatile,proisstrict,prosecdef,proleakproof,proparallel,proconfig,
   pg_get_function_result(oid),prosrc) ORDER BY proname,pg_get_function_identity_arguments(oid)) FROM functions),
 'cohort_event_index',pg_get_indexdef('idx_events_ortak_reconciliation'::regclass),
 'fence_targets',(SELECT jsonb_agg(jsonb_build_array(c.relname,t.tgname,t.tgenabled,t.tgtype,
   t.tgdeferrable,t.tginitdeferred,pg_get_triggerdef(t.oid,false)) ORDER BY c.relname,t.tgname)
   FROM pg_trigger t JOIN pg_proc p ON p.oid=t.tgfoid JOIN pg_class c ON c.oid=t.tgrelid
   JOIN pg_namespace n ON n.oid=c.relnamespace WHERE n.nspname='public'
   AND NOT t.tgisinternal AND p.proname='enforce_community_write_fence')
);
"""
CATALOG = CATALOG.replace('__EXTENSIONS77_TABLES__', ','.join("'" + name + "'" for name in extensions77_catalog.TABLES))
CATALOG = CATALOG.replace('__EXTENSIONS77_FUNCTIONS__', ','.join("'" + name + "'" for name in extensions77_catalog.FUNCTION_NAMES))


class Refused(Exception):
    """Only fixed, non-sensitive error codes cross the command boundary."""


def selected_url(value):
    """Reject alternate ports/hosts, libpq parameters and ambient selection."""
    try:
        url = urlsplit(value)
        if (url.scheme not in ("postgres", "postgresql") or url.hostname != "127.0.0.1"
                or url.port != 55432 or url.query or url.fragment
                or not re.fullmatch(r"/[a-zA-Z_][a-zA-Z0-9_]{0,62}", url.path)
                or not re.fullmatch(r"[a-zA-Z_][a-zA-Z0-9_]{0,62}", url.username or "")
                or url.password is None or not url.password or len(value) > 1024
                or any(ord(c) < 32 for c in unquote(url.password))):
            raise ValueError()
        return {"host": "127.0.0.1", "port": 55432, "user": url.username,
                "password": unquote(url.password), "dbname": url.path[1:]}
    except (ValueError, TypeError, AttributeError):
        raise Refused("explicit_disposable_test_url_required") from None


def database_name(value):
    """Only this probe's fresh generated names may receive schema writes."""
    if not re.fullmatch(r"ortak_parity_[0-9a-f]{32}_(desired|migrated)", value):
        raise Refused("generated_database_name_required")
    return value


def executable(path):
    """Require an explicit absolute regular executable, without following links."""
    if not path.is_absolute() or path.resolve() != path:
        raise Refused("absolute_executable_required")
    info = path.lstat()
    if not stat.S_ISREG(info.st_mode) or not os.access(path, os.X_OK):
        raise Refused("regular_executable_required")
    return path


def write_private(path, content):
    """Persist one fresh protected file; existing probe evidence is never overwritten."""
    descriptor = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600)
    with os.fdopen(descriptor, "wb") as stream:
        stream.write(content)
        stream.flush()
        os.fsync(stream.fileno())
    descriptor = os.open(path.parent, os.O_RDONLY | os.O_DIRECTORY)
    try:
        os.fsync(descriptor)
    finally:
        os.close(descriptor)


def document(path, value):
    write_private(path, (json.dumps(value, sort_keys=True, indent=2) + "\n").encode())


def digest(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


class Commands:
    """Bound each local process group to 120s, all commands to 300s and logs to 4 MiB."""
    def __init__(self, directory):
        self.directory = directory
        self.deadline = time.monotonic() + MAX_SECONDS

    def run(self, label, args, environment):
        deadline = min(self.deadline, time.monotonic() + 120)
        if deadline <= time.monotonic():
            raise Refused("probe_deadline_exceeded")
        descriptor = os.open(self.directory / (label + ".log"),
                             os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600)
        with os.fdopen(descriptor, "wb") as log:
            child = subprocess.Popen(args, cwd=REPO, env=environment, stdin=subprocess.DEVNULL,
                                     stdout=subprocess.PIPE, stderr=subprocess.STDOUT, start_new_session=True)
            try:
                size = 0
                with selectors.DefaultSelector() as ready:
                    ready.register(child.stdout, selectors.EVENT_READ)
                    while ready.get_map():
                        remaining = deadline - time.monotonic()
                        if remaining <= 0 or not ready.select(remaining):
                            raise Refused("child_deadline_exceeded")
                        block = os.read(child.stdout.fileno(), min(65536, MAX_OUTPUT - size + 1))
                        if not block:
                            ready.unregister(child.stdout)
                            continue
                        size += len(block)
                        if size > MAX_OUTPUT:
                            raise Refused("child_output_limit_exceeded")
                        log.write(block)
                if child.wait(timeout=max(0.001, deadline - time.monotonic())) != 0:
                    raise Refused("child_failed")
                if time.monotonic() >= deadline:
                    raise Refused("child_deadline_exceeded")
                log.flush()
                os.fsync(log.fileno())
            finally:
                # Stop the owned process group even if the leader already exited.
                try:
                    os.killpg(child.pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass
                child.wait(timeout=3)
                child.stdout.close()


def environment(connection, database, home):
    """Reconstruct both target and plan selection; never inherit libpq/service/proxy settings."""
    database_name(database)
    result = {"PATH": "/usr/bin:/bin:/usr/sbin:/sbin", "LANG": "C", "LC_ALL": "C", "HOME": str(home),
              "PGHOST": "127.0.0.1", "PGPORT": "55432", "PGUSER": connection["user"],
              "PGPASSWORD": connection["password"], "PGDATABASE": database, "PGCONNECT_TIMEOUT": "3",
              "PGOPTIONS": "-c lock_timeout=2000 -c statement_timeout=110000 -c idle_in_transaction_session_timeout=110000"}
    for key in ("HOST", "PORT", "USER", "PASSWORD", "DB"):
        result["PGSCHEMA_PLAN_" + key] = result["PGDATABASE" if key == "DB" else "PG" + key]
    return result


def query_worker(selected, database, sql, parameters, options, result_path):
    """One owned SQL process; retain bounded diagnostics only in its private result."""
    # Neither native libpq diagnostics nor an unexpected child traceback may
    # escape to the operator's terminal. Explicit results remain available.
    descriptor = os.open(os.devnull, os.O_RDWR)
    try:
        for destination in (0, 1, 2):
            os.dup2(descriptor, destination)
    finally:
        if descriptor > 2:
            os.close(descriptor)
    os.environ.clear()
    os.environ.update({"PATH": "/usr/bin:/bin:/usr/sbin:/sbin", "LANG": "C", "LC_ALL": "C"})
    connection = None
    try:
        import psycopg2
        connection = psycopg2.connect(**{**selected, "dbname": database}, connect_timeout=3,
            options=options, application_name="ortak_schema_parity_55432",
            sslmode="disable", gssencmode="disable")
        connection.autocommit = True
        with connection.cursor() as cursor:
            cursor.execute(sql, parameters)
            value = cursor.fetchone()[0] if cursor.description else None
        result = {"status": "ok", "value": value}
    except Exception as error:
        result = {"status": "failed", "error_type": type(error).__name__,
                  "error_message": str(error)[:8192]}
    finally:
        if connection is not None:
            connection.close()
    encoded = (json.dumps(result, sort_keys=True) + "\n").encode()
    if len(encoded) > MAX_OUTPUT:
        encoded = b'{"status":"failed","error_type":"sql_result_limit_exceeded"}\n'
    write_private(result_path, encoded)


class Database:
    """Bound direct SQL by a spawned-process wall clock and server-side deadlines."""
    def __init__(self, selected, deadline, directory, worker=query_worker):
        self.selected, self.deadline, self.directory = selected, deadline, directory
        self.worker = worker

    def query(self, database, sql, parameters=None, *, admin=False):
        if not admin:
            database_name(database)
        elif database != self.selected["dbname"]:
            raise Refused("admin_database_changed")
        remaining = self.deadline - time.monotonic()
        if remaining <= 0:
            raise Refused("probe_deadline_exceeded")
        deadline = time.monotonic() + min(MAX_SQL_SECONDS, remaining)
        options = (f"-c lock_timeout=2000 -c statement_timeout={max(1, min(30000, int(remaining * 1000)))} "
                   "-c idle_in_transaction_session_timeout=30000")
        result_path = self.directory / ("sql-" + uuid4().hex + ".json")
        process = multiprocessing.get_context("spawn").Process(target=self.worker,
            args=(self.selected, database, sql, parameters, options, result_path))
        try:
            process.start()
            process.join(timeout=max(0, deadline - time.monotonic()))
            if process.is_alive() or time.monotonic() >= deadline:
                raise Refused("sql_deadline_exceeded")
            if process.exitcode != 0:
                raise Refused("sql_worker_failed")
        finally:
            if process.pid is not None:
                if process.is_alive():
                    process.terminate()
                    process.join(timeout=0.2)
                if process.is_alive():
                    process.kill()
                    process.join(timeout=0.8)
                if process.is_alive():
                    raise Refused("sql_worker_containment_failed")
                process.close()
        # Read only after the writer has exited: a partial pipe or file cannot
        # block the parent. A dead worker never yields a partial success.
        with result_path.open("rb") as stream:
            encoded = stream.read(MAX_OUTPUT + 1)
        if len(encoded) > MAX_OUTPUT:
            raise Refused("sql_result_limit_exceeded")
        result = json.loads(encoded)
        if result.get("status") != "ok":
            raise Refused("sql_query_failed")
        return result["value"]


def checked_catalog(value):
    """Presence matters: equal empty catalogs do not prove the guards exist."""
    if (not isinstance(value, dict) or sorted(value.get("tables") or []) != sorted(TABLES)
            or sorted(row[0] for row in value.get("functions") or []) != sorted(FUNCTIONS)
            or not all(value.get(key) for key in ("columns", "indexes", "constraints", "triggers", "fence_targets"))):
        raise Refused("required_catalog_missing")
    triggers = {(row[0], row[1]): row for row in value["triggers"]}
    for name, kind in (('ortak_private_dm_identity', 19), ('ortak_office_authority_channels', 31)):
        row = triggers.get(('channels', name))
        if row is None or row[2:6] != ['O', kind, False, False]:
            raise Refused('private_dm_authority_guard_missing')
    expected_direct = [
        ['ortak_office_authority_channels', 'ortak_fence_office_mutation',
         ''.join(argument + '\0' for argument in DIRECT_AUTHORITY_ARGS).encode().hex(), 10, '', True],
        ['ortak_private_dm_identity', 'ortak_private_dm_identity', '', 0, '', True],
    ]
    if value.get('direct_authority') != expected_direct:
        raise Refused('private_dm_authority_arguments_differ')
    functions = {row[0]: row for row in value['functions']}
    for name, metadata in SNAPSHOT_DM_FUNCTIONS.items():
        row = functions[name]
        if len(row) != 11 or row[1:10] != metadata or not row[10]:
            raise Refused('snapshot_or_dm_function_contract_differs')
    for table, name, trigger_type in (
            ("routing_decisions", "ortak_routing_claim_expiry_at_commit", 5),
            ("work_acceptance_criteria", "trg_work_definition_criterion_history", 17),
            ("work_api_operations", "work_api_receipt_at_commit", 5),
            ("project_api_bindings", "project_api_binding_purge_at_commit", 9),
            ("provisioning_operations", "ortak_activation_admission_at_commit", 21),
            ("work_executions", "work_execution_request_at_commit", 5),
            ("runs", "work_run_admission_at_commit", 21),
            ("runtime_work_outputs", "work_output_provenance_at_commit", 21),
            ("artifacts", "artifact_provenance_at_commit", 5),
            ("provisioning_operations", "employee_management_operation_at_commit", 21),
            ("provisioning_operation_steps", "employee_management_step_at_commit", 21),
            ("runs", "lifecycle_run_admission", 21),
            ("employee_lifecycle_events", "lifecycle_activation_at_commit", 5),
            ("runtime_work_outputs", "lifecycle_work_output_at_commit", 21),
            ("artifacts", "lifecycle_artifact_at_commit", 5),
            ("runtime_office_outputs", "lifecycle_office_output_at_commit", 21),
            ("runtime_memory_writes", "lifecycle_memory_output_at_commit", 21),
            ("reviewed_memory_facts", "reviewed_fact_receipt_at_commit", 21),
            ("reviewed_memory_operations", "reviewed_memory_operation_at_commit", 5),
            ("provisioning_runtime_probes", "provisioning_runtime_probe_management_at_commit", 21),
            ("provisioning_runtime_probes", "provisioning_runtime_probe_live_at_commit", 21),
            ("reviewed_memory_exports", "reviewed_export_at_commit", 5),
            ("reviewed_memory_export_jobs", "reviewed_export_job_at_commit", 21),
            ("reviewed_memory_export_commands", "reviewed_export_command_at_commit", 5),
            ("reviewed_memory_export_receipts", "reviewed_export_receipt_at_commit", 5),
            ("work_decomposition", "work_decomposition_at_commit", 5),
            ("run_context_snapshots", "ortak_reviewed_snapshot_consistent", 5),
            ("run_reviewed_memory_uses", "ortak_reviewed_use_consistent", 5),
            ("runs", "ortak_reviewed_run_admission", 17),
            ("artifacts", "ortak_reviewed_artifact_admission", 5)):
        row = triggers.get((table, name))
        if row is None or row[2:6] != ["O", trigger_type, True, True]:
            raise Refused("deferred_commit_guard_missing")
    for table, name, trigger_type in (
            ("provisioning_operations", "ortak_activation_operation_immutable", 27),
            ("provisioning_operation_steps", "ortak_activation_receipt_immutable", 27),
            ("provisioning_operations", "ortak_activation_operation_no_truncate", 34),
            ("provisioning_operation_steps", "ortak_activation_receipt_no_truncate", 34),
            ("office_identity_profiles", "trg_office_identity_profiles_immutable", 27),
            ("provisioning_runner_selections", "trg_provisioning_runner_selections_immutable", 27),
            ("office_identity_profiles", "trg_office_identity_profiles_no_truncate", 34),
            ("provisioning_runner_selections", "trg_provisioning_runner_selections_no_truncate", 34)):
        row = triggers.get((table, name))
        if row is None or row[2:6] != ["O", trigger_type, False, False]:
            raise Refused("activation_mutation_guard_missing")
    for table, name, trigger_type in (
            ("routing_recipients", "lifecycle_pin_recipient", 23),
            ("runs", "lifecycle_pin_run", 23),
            ("provisioning_operations", "lifecycle_pin_operation", 23),
            ("provisioning_operations", "lifecycle_provisioning_operation", 23),
            ("provisioning_operation_steps", "lifecycle_provisioning_step", 23),
            ("employees", "ortak_z_employee_lifecycle", 19),
            ("employee_lifecycle_events", "employee_lifecycle_event_transition", 7),
            ("employee_lifecycle_events", "employee_lifecycle_events_immutable", 27),
            ("employee_lifecycle_events", "employee_lifecycle_events_no_truncate", 34)):
        row = triggers.get((table, name))
        if row is None or row[2:6] != ["O", trigger_type, False, False]:
            raise Refused("lifecycle_mutation_guard_missing")
    for table, name, trigger_type in (
            ("work_dependencies", "work_dependency_authority_guard", 23),
            ("work_dependencies", "work_authority_dependencies", 21),
            ("work_dependencies", "trg_work_dependencies_no_delete", 11),
            ("work_dependencies", "trg_work_dependencies_no_truncate", 34)):
        row = triggers.get((table, name))
        if row is None or row[2:6] != ["O", trigger_type, False, False]:
            raise Refused("dependency_mutation_guard_missing")
    for name, trigger_type in (("provisioning_runtime_probe_guard", 23),
                               ("provisioning_runtime_probe_no_delete", 11),
                               ("provisioning_runtime_probe_no_truncate", 34)):
        row = triggers.get(("provisioning_runtime_probes", name))
        if row is None or row[2:6] != ["O", trigger_type, False, False]:
            raise Refused("runtime_probe_mutation_guard_missing")
    for table, name, kind in (
            ("work_decomposition", "work_decomposition_reserve", 7),
            ("work_decomposition", "work_decomposition_immutable", 27),
            ("work_decomposition", "work_decomposition_no_truncate", 34),
            ("run_reviewed_memory_uses", "ortak_reviewed_use_immutable", 27),
            ("run_reviewed_memory_uses", "ortak_reviewed_use_no_truncate", 34),
            ("reviewed_memory_facts", "trg_activity_reviewed_fact_use", 17),
            ("reviewed_memory_targets", "trg_activity_reviewed_target_use", 17)):
        row = triggers.get((table, name))
        if row is None or row[2:6] != ["O", kind, False, False]:
            raise Refused("decomposition_or_reviewed_use_guard_missing")
    constraints = {(row[0], row[1]): row for row in value["constraints"]}
    for table, name, definition in INTEGRATION_CHECKS:
        row = constraints.get((table, name))
        if row is None or row[2:] != ["c", definition, True, False, False]:
            raise Refused("decomposition_or_reviewed_use_check_missing")
    if not value.get("cohort_event_index"):
        raise Refused("cohort_reconciliation_index_missing")
    export_guards = [(table, name, kind) for table in EXPORT_TABLES for name, kind in
                     (("reviewed_export_no_delete", 11), ("reviewed_export_no_truncate", 34))]
    export_guards += [(table, "reviewed_export_immutable", 19) for table in
                      ('reviewed_memory_exports', 'reviewed_memory_export_commands', 'reviewed_memory_export_receipts')]
    export_guards += [("reviewed_memory_targets", "reviewed_target_guard", 23),
                      ("reviewed_memory_facts", "reviewed_export_stop", 17),
                      ("reviewed_memory_export_jobs", "reviewed_export_job_guard", 19)]
    for table, name, kind in export_guards:
        row = triggers.get((table, name))
        if row is None or row[2:6] != ["O", kind, False, False]:
            raise Refused("reviewed_export_mutation_guard_missing")
    for table, name, trigger_type in (
            ("reviewed_memory_facts", "reviewed_fact_guard", 23),
            ("reviewed_memory_facts", "reviewed_fact_no_delete", 11),
            ("reviewed_memory_facts", "reviewed_fact_no_truncate", 34),
            ("reviewed_memory_operations", "reviewed_memory_operation_immutable", 27),
            ("reviewed_memory_operations", "reviewed_memory_operation_no_truncate", 34)):
        row = triggers.get((table, name))
        if row is None or row[2:6] != ["O", trigger_type, False, False]:
            raise Refused("reviewed_memory_mutation_guard_missing")
    for table, name, trigger_type in (
            ("office_identity_profiles", "ortak_retained_office_authority", 23),
            ("office_inbox_reconciliations", "ortak_retained_office_authority", 23),
            ("office_routing_cohorts", "ortak_routing_cohort_state", 23),
            ("office_inbox_reconciliations", "ortak_inbox_reconciliation_evidence", 31)):
        row = triggers.get((table, name))
        if row is None or row[2:6] != ["O", trigger_type, False, False]:
            raise Refused("cohort_mutation_guard_missing")
    activity = [row for row in value["triggers"] if row[1].startswith("trg_activity_")]
    if (len(activity) != len(ACTIVITY_GUARDS)
            or {(row[0], row[1], row[3]) for row in activity} != set(ACTIVITY_GUARDS)
            or any(row[2] != "O" or row[4:6] != [False, False] for row in activity)):
        raise Refused("activity_notification_guard_missing")
    # Equality alone cannot prove enforcement: two disabled or incomplete
    # universal fences must not produce a successful parity receipt. Apply the
    # full event/mode requirement to every catalogued fence, including legacy
    # tables outside this integration's focused table inventory.
    if any(not isinstance(row, list) or len(row) != 7 or row[2:6] != ["O", 31, False, False]
           for row in value["fence_targets"]):
        raise Refused("universal_community_fence_invalid")
    if not {"office_identity_profiles", "office_routing_cohorts", "office_routing_channels", "office_inbox_reconciliations"}.issubset(
            {row[0] for row in value["fence_targets"]}):
        raise Refused("cohort_community_fence_missing")
    if not any(row[0] == "project_api_bindings" for row in value["fence_targets"]):
        raise Refused("work_community_fence_missing")
    if not {"reviewed_memory_facts", "reviewed_memory_operations"}.issubset({row[0] for row in value["fence_targets"]}):
        raise Refused("reviewed_memory_community_fence_missing")
    if not set(EXPORT_TABLES).issubset({row[0] for row in value["fence_targets"]}):
        raise Refused("reviewed_export_community_fence_missing")
    if not any(row[0] == "run_reviewed_memory_uses" for row in value["fence_targets"]):
        raise Refused("reviewed_use_community_fence_missing")
    if not set(CONVERSATION_TABLES).issubset({row[0] for row in value['fence_targets']}):
        raise Refused('conversation_community_fence_missing')
    workspace_catalog.check(value, Refused)
    conversation_catalog.check(value, Refused)
    extensions77_catalog.check(value, Refused)
    context79_catalog.check(value, Refused)
    return value


def probe(value, binary, receipt_parent, commands_type=Commands, database_type=Database):
    """Create exactly two fresh databases and retain them on every outcome."""
    selected = selected_url(value)  # Before even creating local files/children.
    executable(binary)
    executable(PGSCHEMA)
    if not receipt_parent.is_absolute() or receipt_parent.resolve() != receipt_parent:
        raise Refused("absolute_receipt_parent_required")
    info = receipt_parent.lstat()
    if not stat.S_ISDIR(info.st_mode) or info.st_uid != os.getuid() or stat.S_IMODE(info.st_mode) != 0o700:
        raise Refused("private_receipt_parent_required")
    identifier = uuid4().hex
    directory = receipt_parent / ("schema-parity-" + identifier)
    directory.mkdir(mode=0o700)
    home = directory / "home"
    home.mkdir(mode=0o700)
    desired, migrated = [database_name(f"ortak_parity_{identifier}_{kind}") for kind in ("desired", "migrated")]
    for relative, name in (("schema/schema.sql", "schema.sql"),
                           ("scripts/reconcile-schema-after-pgschema.sql", "reconcile.sql")):
        with (REPO / relative).open("rb") as stream:
            source = stream.read(MAX_OUTPUT + 1)
        if len(source) > MAX_OUTPUT:
            raise Refused("source_size_limit_exceeded")
        write_private(directory / name, source)
    receipt = {"format": "ortak-schema-parity/v1", "status": "started", "host": "127.0.0.1", "port": 55432,
               "desired_database": desired, "migrated_database": migrated, "databases_retained": True,
               "created_at": datetime.now(timezone.utc).isoformat(), "migration_target": 79,
               "migration_test_binary": str(binary), "migration_test_sha256": digest(binary),
               "pgschema_binary": str(PGSCHEMA), "pgschema_sha256": digest(PGSCHEMA),
               "schema_sha256": digest(directory / "schema.sql"),
               "reconciliation_sha256": digest(directory / "reconcile.sql")}
    document(directory / "intent.json", receipt)
    commands = commands_type(directory)
    db = database_type(selected, commands.deadline, directory)
    try:
        for name in (desired, migrated):
            # The generated grammar contains no quotes; never reuse/drop/clean a DB.
            db.query(selected["dbname"], f'CREATE DATABASE "{name}" TEMPLATE template0', admin=True)
        env = environment(selected, desired, home)
        commands.run("pgschema-apply", [str(PGSCHEMA), "apply", "--auto-approve", "--file", str(directory / "schema.sql"),
            "--host", "127.0.0.1", "--port", "55432", "--db", desired,
            "--plan-host", "127.0.0.1", "--plan-port", "55432", "--plan-db", desired], env)
        reconcile = (directory / "reconcile.sql").read_text()
        snapshots = []
        for _ in range(2):
            db.query(desired, reconcile)
            snapshots.append(checked_catalog(db.query(desired, CATALOG, (TABLES, FUNCTIONS, TABLES))))
        if snapshots[0] != snapshots[1]:
            raise Refused("reconciliation_not_idempotent")
        desired_catalog = snapshots[1]
        document(directory / "desired-catalog.json", desired_catalog)
        from urllib.parse import quote
        env = environment(selected, migrated, home)
        env["BUZZ_TEST_DATABASE_URL"] = (f"postgres://{selected['user']}:{quote(selected['password'], safe='')}@127.0.0.1:55432/{migrated}")
        commands.run("migration-test", [str(binary), "--exact", TEST, "--ignored", "--test-threads=1"], env)
        versions = db.query(migrated, "SELECT json_agg(version ORDER BY version) FROM _sqlx_migrations WHERE success")
        if versions != list(range(1, 80)):
            raise Refused("migration79_not_proven")
        migrated_catalog = checked_catalog(db.query(migrated, CATALOG, (TABLES, FUNCTIONS, TABLES)))
        document(directory / "migrated-catalog.json", migrated_catalog)
        different = sorted(key for key in desired_catalog if desired_catalog[key] != migrated_catalog.get(key))
        receipt["different_components"] = different
        if different:
            raise Refused("schema_catalog_mismatch")
        receipt.update(status="verified", migration_versions=versions, reconciliation_passes=2,
                       compared_components=sorted(desired_catalog), provider_calls=0)
    except Exception as error:
        receipt.update(status="failed", error_code=str(error) if isinstance(error, Refused) else "schema_probe_failed")
        raise Refused("schema_probe_failed_databases_retained") from None
    finally:
        receipt["finished_at"] = datetime.now(timezone.utc).isoformat()
        document(directory / "receipt.json", receipt)
    return directory


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--migration-test-binary", type=Path, required=True)
    parser.add_argument("--receipt-parent", type=Path, required=True, help="existing real owner-private0700 directory")
    args = parser.parse_args()
    try:
        value = os.environ.get(URL_ENV)
        selected_url(value)
        # Dedicated CLI process: libpq must not consult ambient services, SSL
        # credentials, PGOPTIONS or alternate URL selectors during direct SQL.
        os.environ.clear()
        os.environ.update({"PATH": "/usr/bin:/bin:/usr/sbin:/sbin", "LANG": "C", "LC_ALL": "C"})
        directory = probe(value, args.migration_test_binary, args.receipt_parent)
        print(json.dumps({"status": "verified", "receipt": str(directory / "receipt.json"), "databases_retained": True}))
    except Exception:
        print(json.dumps({"status": "failed", "code": "schema_parity_failed", "databases_retained": True}))
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
