"""Production selection, retained receipt and command bounds without database access."""
import copy
from contextlib import redirect_stdout
import io
import json
import os
import re
from pathlib import Path
import signal
import subprocess
import tempfile
import time
import unittest
from unittest.mock import patch

import check_schema_parity as subject
import schema_source
import extensions77_catalog

MIGRATION77 = (subject.REPO / 'migrations/0077_ortak_employee_memory_encrypted_dm.sql').read_text()
MIGRATION77_FUNCTIONS = {entry.name: entry for entry in schema_source.functions(MIGRATION77)}
MIGRATION78 = (subject.REPO / 'migrations/0078_ortak_employee_memory_event_scope.sql').read_text()
MIGRATION78_FUNCTIONS = {entry.name: entry for entry in schema_source.functions(MIGRATION78)}
FINAL_EXTENSION_FUNCTIONS = MIGRATION77_FUNCTIONS | MIGRATION78_FUNCTIONS

URL = "postgres://fixture:selected-fixture@127.0.0.1:55432/postgres"
EXPECTED_FUNCTIONS = ["ortak_check_routing_claim_expiry", "ortak_check_work_api_receipt", "ortak_project_access_guard",
                      "ortak_assert_project_binding_purge", "ortak_guard_project_api_binding", "ortak_project_binding_purge_at_commit",
                      "ortak_check_activation_admission_at_commit", "ortak_guard_activation_operation", "ortak_guard_activation_receipt",
                      "ortak_office_profile_receipt_immutable", "ortak_provisioning_selection_immutable", "ortak_guard_routing_cohort_state",
                      "ortak_invalidate_routing_capture", "ortak_guard_inbox_reconciliation", "ortak_activity_notify", "ortak_guard_retained_office_authority", "work_acceptance_criteria_guard", "work_definition_criterion_history_guard"]

EXPECTED_FUNCTIONS += ['ortak_work_generation_guard', 'ortak_advance_work_authority', 'ortak_work_child_authority_guard', 'ortak_work_execution_guard', 'ortak_work_output_guard', 'ortak_schedule_completed_office_output', 'ortak_schedule_work_output', 'ortak_check_work_execution_request', 'ortak_work_run_identity_guard', 'ortak_check_run_work_authority', 'ortak_check_work_output_provenance', 'ortak_management_immutable', 'ortak_management_actor_allowed', 'ortak_management_guard', 'ortak_management_operation_fence']
EXPECTED_FUNCTIONS += ['ortak_guard_lifecycle_event_insert', 'ortak_pin_employee_lifecycle', 'ortak_check_run_lifecycle', 'ortak_check_provisioning_lifecycle', 'ortak_guard_employee_lifecycle', 'ortak_check_lifecycle_activation', 'ortak_check_output_lifecycle']
EXPECTED_FUNCTIONS += ['ortak_work_dependency_edit_guard', 'ortak_provisioning_runtime_probe_guard']
EXPECTED_FUNCTIONS += ['ortak_reviewed_fact_source_visible', 'ortak_reviewed_fact_guard', 'ortak_reviewed_fact_receipt_at_commit', 'ortak_reviewed_memory_operation_at_commit']

EXPORT_TABLES = ('reviewed_memory_targets', 'reviewed_memory_exports', 'reviewed_memory_export_jobs',
                 'reviewed_memory_export_commands', 'reviewed_memory_export_receipts')
EXPORT_DEFERRED = (
    ('reviewed_memory_exports', 'reviewed_export_at_commit', 5),
    ('reviewed_memory_export_jobs', 'reviewed_export_job_at_commit', 21),
    ('reviewed_memory_export_commands', 'reviewed_export_command_at_commit', 5),
    ('reviewed_memory_export_receipts', 'reviewed_export_receipt_at_commit', 5))
EXPORT_IMMEDIATE = tuple((table, name, kind) for table in EXPORT_TABLES for name, kind in
                         (('reviewed_export_no_delete', 11), ('reviewed_export_no_truncate', 34)))
EXPORT_IMMEDIATE += tuple((table, 'reviewed_export_immutable', 19) for table in
                         ('reviewed_memory_exports', 'reviewed_memory_export_commands', 'reviewed_memory_export_receipts'))
EXPORT_IMMEDIATE += (('reviewed_memory_targets', 'reviewed_target_guard', 23),
                     ('reviewed_memory_facts', 'reviewed_export_stop', 17),
                     ('reviewed_memory_export_jobs', 'reviewed_export_job_guard', 19))
EXPECTED_FUNCTIONS += ['ortak_reviewed_export_source_hash', 'ortak_reviewed_export_eligible', 'ortak_reviewed_target_guard',
                      'ortak_reviewed_export_at_commit', 'ortak_reviewed_export_stop', 'ortak_reviewed_export_job_guard',
                      'ortak_reviewed_export_job_at_commit', 'ortak_reviewed_export_command_at_commit',
                      'ortak_reviewed_export_receipt_at_commit', 'ortak_reviewed_export_view']

EXPECTED_FUNCTIONS += ['ortak_work_decomposition_reserve', 'ortak_work_decomposition_commit',
                       'ortak_reviewed_runtime_eligible', 'ortak_run_reviewed_memory_current',
                       'ortak_lock_run_reviewed_memory', 'ortak_reviewed_use_immutable',
                       'ortak_reviewed_snapshot_consistent', 'ortak_reviewed_run_admission']
EXPECTED_FUNCTIONS += ['ortak_snapshot_scratch_jsonb', 'ortak_private_dm_identity', 'ortak_fence_office_mutation']
EXPECTED_FUNCTIONS += ['ortak_workspace_canonical', 'ortak_workspace_binding_guard',
                       'ortak_workspace_manifest_consistent', 'ortak_workspace_profile_available',
                       'ortak_workspace_activation_at_commit', 'ortak_run_workspace_current',
                       'ortak_lock_run_workspace', 'ortak_workspace_use_at_commit',
                       'ortak_workspace_action_guard', 'ortak_workspace_action_at_commit',
                       'ortak_workspace_receipt_at_commit', 'ortak_workspace_run_admission',
                       'ortak_workspace_reader_guard', 'ortak_workspace_reader_cancel_fence']
EXPECTED_FUNCTIONS += ['ortak_conversation_json75', 'ortak_conversation_source_observation',
                       'ortak_conversation_scope_current', 'ortak_conversation_authority_guard',
                       'ortak_register_conversation_authority', 'ortak_conversation_fact_storage_at_commit',
                       'ortak_conversation_use_storage_at_commit', 'ortak_conversation_thread_insert_neutral75',
                       'ortak_advance_conversation_scopes75', 'ortak_conversation_epoch_mutation75']
EXPECTED_FUNCTIONS += ['ortak_conversation_run_origin', 'ortak_conversation_target_eligible76',
              'ortak_conversation_export_eligible', 'ortak_conversation_runtime_eligible',
              'ortak_conversation_effect_admission76', 'ortak_conversation_snapshot76']
EXPECTED_FUNCTIONS += [name for name in MIGRATION77_FUNCTIONS if name not in EXPECTED_FUNCTIONS]

CONVERSATION_EPOCHS = [
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
CONVERSATION_THREAD_ARGS = ('community', 'community_id', 'event_id', 'event_created_at', 'channel_id',
                            'parent_event_id', 'parent_event_created_at', 'root_event_id',
                            'root_event_created_at', 'depth')
CONVERSATION_EPOCH_FUNCTIONS = {
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


def conversation_trigger(table, name, kind, deferred, function, args=(), parent=None):
    """Independent fixture for the actual catalog projection, including clone links."""
    return [table, name, 'O', kind, deferred, deferred, 'public', function,
            ''.join(arg + '\0' for arg in args).encode().hex(), len(args), [], True, False, parent]
DIRECT_ARGUMENTS = ('community', 'community_id', 'id', 'channel_type', 'visibility',
                    'archived_at', 'deleted_at', 'participant_hash', 'ttl_seconds', 'ttl_deadline')
DIRECT_FUNCTIONS = {
    'ortak_snapshot_scratch_jsonb': ['value json', 'sql', 'i', True, False, False, 's', None, 'jsonb'],
    'ortak_reviewed_snapshot_consistent': ['', 'plpgsql', 'v', False, False, False, 'u', None, 'trigger'],
    'ortak_private_dm_identity': ['', 'plpgsql', 'v', False, False, False, 'u', None, 'trigger'],
    'ortak_fence_office_mutation': ['', 'plpgsql', 'v', False, False, False, 'u', None, 'trigger'],
}
USE_DEFERRED = (
    ('work_decomposition', 'work_decomposition_at_commit', 5),
    ('run_context_snapshots', 'ortak_reviewed_snapshot_consistent', 5),
    ('run_reviewed_memory_uses', 'ortak_reviewed_use_consistent', 5),
    ('runs', 'ortak_reviewed_run_admission', 17),
    ('artifacts', 'ortak_reviewed_artifact_admission', 5))
USE_IMMEDIATE = (
    ('work_decomposition', 'work_decomposition_reserve', 7),
    ('work_decomposition', 'work_decomposition_immutable', 27),
    ('work_decomposition', 'work_decomposition_no_truncate', 34),
    ('run_reviewed_memory_uses', 'ortak_reviewed_use_immutable', 27),
    ('run_reviewed_memory_uses', 'ortak_reviewed_use_no_truncate', 34),
    ('reviewed_memory_facts', 'trg_activity_reviewed_fact_use', 17),
    ('reviewed_memory_targets', 'trg_activity_reviewed_target_use', 17))

PROBE_DEFERRED = (
    ("provisioning_runtime_probes", "provisioning_runtime_probe_management_at_commit", 21),
    ("provisioning_runtime_probes", "provisioning_runtime_probe_live_at_commit", 21))
PROBE_IMMEDIATE = (
    ("provisioning_runtime_probes", "provisioning_runtime_probe_guard", 23),
    ("provisioning_runtime_probes", "provisioning_runtime_probe_no_delete", 11),
    ("provisioning_runtime_probes", "provisioning_runtime_probe_no_truncate", 34))

LIFECYCLE_DEFERRED = (
    ("runs", "lifecycle_run_admission", 21),
    ("employee_lifecycle_events", "lifecycle_activation_at_commit", 5),
    ("runtime_work_outputs", "lifecycle_work_output_at_commit", 21),
    ("artifacts", "lifecycle_artifact_at_commit", 5),
    ("runtime_office_outputs", "lifecycle_office_output_at_commit", 21),
    ("runtime_memory_writes", "lifecycle_memory_output_at_commit", 21))
LIFECYCLE_IMMEDIATE = (
    ("routing_recipients", "lifecycle_pin_recipient", 23),
    ("runs", "lifecycle_pin_run", 23),
    ("provisioning_operations", "lifecycle_pin_operation", 23),
    ("provisioning_operations", "lifecycle_provisioning_operation", 23),
    ("provisioning_operation_steps", "lifecycle_provisioning_step", 23),
    ("employees", "ortak_z_employee_lifecycle", 19),
    ("employee_lifecycle_events", "employee_lifecycle_event_transition", 7),
    ("employee_lifecycle_events", "employee_lifecycle_events_immutable", 27),
    ("employee_lifecycle_events", "employee_lifecycle_events_no_truncate", 34))

REVIEWED_DEFERRED = (
    ("reviewed_memory_facts", "reviewed_fact_receipt_at_commit", 21),
    ("reviewed_memory_operations", "reviewed_memory_operation_at_commit", 5))
DEPENDENCY_IMMEDIATE = (
    ("work_dependencies", "work_dependency_authority_guard", 23),
    ("work_dependencies", "work_authority_dependencies", 21),
    ("work_dependencies", "trg_work_dependencies_no_delete", 11),
    ("work_dependencies", "trg_work_dependencies_no_truncate", 34))
REVIEWED_IMMEDIATE = (
    ("reviewed_memory_facts", "reviewed_fact_guard", 23),
    ("reviewed_memory_facts", "reviewed_fact_no_delete", 11),
    ("reviewed_memory_facts", "reviewed_fact_no_truncate", 34),
    ("reviewed_memory_operations", "reviewed_memory_operation_immutable", 27),
    ("reviewed_memory_operations", "reviewed_memory_operation_no_truncate", 34))

def catalog():
    value = {"tables": sorted(subject.TABLES), "columns": [["fixture", "column", "uuid"]],
            "indexes": [["fixture", "index", "ORDER BY a DESC NULLS FIRST", "{3}"]],
            "constraints": [["fixture", "constraint", "FOREIGN KEY (a,b)", True, True]]
                + [[table, name, "c", definition, True, False, False]
                   for table, name, definition in subject.INTEGRATION_CHECKS],
            "triggers": [["routing_decisions", "ortak_routing_claim_expiry_at_commit", "O", 5, True, True, "definition"],
                         ["work_api_operations", "work_api_receipt_at_commit", "O", 5, True, True, "definition"],
                         ["project_api_bindings", "project_api_binding_purge_at_commit", "O", 9, True, True, "definition"],
                         ["provisioning_operations", "ortak_activation_admission_at_commit", "O", 21, True, True, "definition"],
                         ["provisioning_operations", "ortak_activation_operation_immutable", "O", 27, False, False, "definition"],
                         ["provisioning_operation_steps", "ortak_activation_receipt_immutable", "O", 27, False, False, "definition"],
                         ["provisioning_operations", "ortak_activation_operation_no_truncate", "O", 34, False, False, "definition"],
                         ["provisioning_operation_steps", "ortak_activation_receipt_no_truncate", "O", 34, False, False, "definition"],
                         ["office_identity_profiles", "trg_office_identity_profiles_immutable", "O", 27, False, False, "definition"],
                         ["provisioning_runner_selections", "trg_provisioning_runner_selections_immutable", "O", 27, False, False, "definition"],
                         ["office_identity_profiles", "trg_office_identity_profiles_no_truncate", "O", 34, False, False, "definition"],
                         ["provisioning_runner_selections", "trg_provisioning_runner_selections_no_truncate", "O", 34, False, False, "definition"],
                         ["office_identity_profiles", "ortak_retained_office_authority", "O", 23, False, False, "definition"],
                         ["office_inbox_reconciliations", "ortak_retained_office_authority", "O", 23, False, False, "definition"],
                         ["office_routing_cohorts", "ortak_routing_cohort_state", "O", 23, False, False, "definition"],
                         ["office_inbox_reconciliations", "ortak_inbox_reconciliation_evidence", "O", 31, False, False, "definition"]]
                        + [[table, name, "O", kind, False, False, "definition"] for table, name, kind in subject.ACTIVITY_GUARDS[:-2]]
                        + [["work_acceptance_criteria", "trg_work_definition_criterion_history", "O", 17, True, True, "definition"]] + [[table, name, "O", kind, True, True, "definition"] for table, name, kind in (
                            ("work_executions", "work_execution_request_at_commit", 5),
                            ("runs", "work_run_admission_at_commit", 21),
                            ("runtime_work_outputs", "work_output_provenance_at_commit", 21),
                            ("artifacts", "artifact_provenance_at_commit", 5),
                            ("provisioning_operations", "employee_management_operation_at_commit", 21),
                            ("provisioning_operation_steps", "employee_management_step_at_commit", 21))]
                        + [[table, name, "O", kind, True, True, "definition"] for table, name, kind in LIFECYCLE_DEFERRED]
                        + [[table, name, "O", kind, False, False, "definition"] for table, name, kind in LIFECYCLE_IMMEDIATE]
                        + [[table, name, "O", kind, True, True, "definition"] for table, name, kind in REVIEWED_DEFERRED]
                        + [[table, name, "O", kind, False, False, "definition"] for table, name, kind in REVIEWED_IMMEDIATE]
                        + [[table, name, "O", kind, False, False, "definition"] for table, name, kind in DEPENDENCY_IMMEDIATE]
                        + [[table, name, "O", kind, True, True, "definition"] for table, name, kind in PROBE_DEFERRED]
                        + [[table, name, "O", kind, False, False, "definition"] for table, name, kind in PROBE_IMMEDIATE]
                        + [[table, name, "O", kind, True, True, "definition"] for table, name, kind in EXPORT_DEFERRED]
                        + [[table, name, "O", kind, False, False, "definition"] for table, name, kind in EXPORT_IMMEDIATE]
                        + [[table, name, "O", kind, True, True, "definition"] for table, name, kind in USE_DEFERRED]
                        + [[table, name, "O", kind, False, False, "definition"] for table, name, kind in USE_IMMEDIATE]
                        + [['channels', name, 'O', kind, False, False, 'definition'] for name, kind in
                           (('ortak_private_dm_identity', 19), ('ortak_office_authority_channels', 31))],
            "direct_authority": [
                ['ortak_office_authority_channels', 'ortak_fence_office_mutation',
                 ''.join(argument + '\0' for argument in DIRECT_ARGUMENTS).encode().hex(), 10, '', True],
                ['ortak_private_dm_identity', 'ortak_private_dm_identity', '', 0, '', True]],
            "functions": [[name, *DIRECT_FUNCTIONS.get(name, subject.workspace_catalog.FUNCTIONS.get(name, subject.conversation_catalog.FUNCTIONS.get(name,
                ['', 'plpgsql', 'v', False, False, False, 'u', None, 'trigger']))), 'body']
                for name in sorted(EXPECTED_FUNCTIONS)],
            "cohort_event_index": "CREATE INDEX idx_events_ortak_reconciliation ON events (...) WHERE ...",
            "fence_targets": [[table, "fence", "O", 31, False, False, "definition"] for table in
                ("project_api_bindings", "office_identity_profiles", "office_routing_cohorts", "office_routing_channels", "office_inbox_reconciliations", "reviewed_memory_facts", "reviewed_memory_operations", "run_reviewed_memory_uses") + EXPORT_TABLES + ('conversation_memory_authorities', 'reviewed_memory_conversation_audiences')]}
    value['triggers'] += [[table, name, 'O', kind, deferred, deferred,
        'CREATE TRIGGER fixture EXECUTE FUNCTION ' + function + '()']
        for table, name, kind, deferred, function in subject.conversation_catalog.TRIGGERS]
    for row in value['functions']:
        if row[0] in CONVERSATION_EPOCH_FUNCTIONS:
            row[1:10] = copy.deepcopy(CONVERSATION_EPOCH_FUNCTIONS[row[0]])
    value['conversation_function_defaults'] = sorted([
        [name, 1, '0'] if name == 'ortak_conversation_json75' else [name, 0, None]
        for name in EXPECTED_FUNCTIONS if 'conversation' in name or name == 'ortak_fence_office_mutation'])
    value['conversation_event_relations'] = [
        ['public', 'events', None, None, 'p', False, None],
        ['public', 'events_fixture_early', 'public', 'events', 'r', True,
         "FOR VALUES FROM (MINVALUE) TO ('2026-01-01 00:00:00+00')"],
        ['public', 'events_fixture_late', 'public', 'events', 'r', True,
         "FOR VALUES FROM ('2026-01-01 00:00:00+00') TO (MAXVALUE)"],
    ]
    value['conversation_triggers'] = [conversation_trigger(*row)
        for row in subject.conversation_catalog.TRIGGERS]
    value['conversation_triggers'] += [conversation_trigger(table, 'community_write_fence_' + table,
        31, False, 'enforce_community_write_fence') for table in
        ('conversation_memory_authorities', 'reviewed_memory_conversation_audiences')]
    value['conversation_triggers'] += [conversation_trigger(table, name, kind, False,
        'ortak_conversation_epoch_mutation75', (arg,)) for table, name, kind, arg in CONVERSATION_EPOCHS]
    value['conversation_triggers'].append(conversation_trigger('thread_metadata',
        'ortak_office_authority_thread_metadata', 31, False, 'ortak_fence_office_mutation',
        CONVERSATION_THREAD_ARGS))
    value['conversation_triggers'] += [conversation_trigger(table, 'conversation_epoch_events', 25,
        False, 'ortak_conversation_epoch_mutation75', ('event',),
        ['public', 'events', 'conversation_epoch_events']) for table in
        ('events_fixture_early', 'events_fixture_late')]
    known_triggers = {tuple(row[:2]) for row in value['triggers']}
    value['triggers'] += [row[:6] + ['definition'] for row in value['conversation_triggers']
                          if tuple(row[:2]) not in known_triggers]
    value['conversation_indexes'] = copy.deepcopy(subject.conversation_catalog.INDEXES)
    workspace = subject.workspace_catalog
    value['columns'] += copy.deepcopy(workspace.COLUMNS)
    value['constraints'] += copy.deepcopy(workspace.CHECKS)
    value['indexes'] += copy.deepcopy(workspace.UNIQUE_INDEXES)
    value['workspace_triggers'] = copy.deepcopy(workspace.TRIGGERS)
    value['workspace_function_defaults'] = copy.deepcopy(workspace.FUNCTION_DEFAULTS)
    value['triggers'] += [row[:6] + ['definition'] for row in workspace.TRIGGERS]
    value['fence_targets'] += [row[:6] + ['definition'] for row in workspace.TRIGGERS
                               if row[7] == 'enforce_community_write_fence']
    extension = extensions77_catalog
    for row in value['functions']:
        if row[0] in extension.FUNCTIONS:
            row[1:10] = copy.deepcopy(extension.FUNCTIONS[row[0]])
            # Real immutable SQL bytes, including the sole78 replacement.
            row[10] = FINAL_EXTENSION_FUNCTIONS[row[0]].body
    value['columns'] += copy.deepcopy(extension.COLUMNS)
    value['constraints'] += copy.deepcopy(extension.CONSTRAINTS)
    value['indexes'] += [row[:9] for row in copy.deepcopy(extension.INDEXES)]
    value['extensions77_function_defaults'] = copy.deepcopy(extension.FUNCTION_DEFAULTS)
    value['extensions77_indexes'] = copy.deepcopy(extension.INDEXES)
    value['extensions77_triggers'] = copy.deepcopy(extension.TRIGGERS)
    event = next(row for row in value['extensions77_triggers']
                 if row[:2] == ['events', 'employee_memory_epoch_events'])
    value['extensions77_triggers'] += [[table, *event[1:13], ['public', 'events', event[1]]]
        for table in ('events_fixture_early', 'events_fixture_late')]
    value['triggers'] += [row[:6] + ['definition'] for row in value['extensions77_triggers']]
    value['fence_targets'] += [row[:6] + ['definition'] for row in extension.TRIGGERS
                               if row[7] == 'enforce_community_write_fence']
    return value


class FakeCommands:
    calls = []
    def __init__(self, directory):
        self.directory = directory
        self.deadline = time.monotonic() + 300
    def run(self, label, args, environment):
        self.calls.append((label, args, environment))
        assert "selected-fixture" not in " ".join(args)
        assert all(name not in environment for name in ("DATABASE_URL", "PGSERVICE", "PGHOSTADDR", "HTTPS_PROXY"))
        if label == "migration-test":
            assert args[1:] == ["--exact", subject.TEST, "--ignored", "--test-threads=1"]
            assert ":55432/ortak_parity_" in environment["BUZZ_TEST_DATABASE_URL"]
        else:
            assert args[args.index("--port") + 1] == "55432"
            assert args[args.index("--plan-port") + 1] == "55432"
        assert environment["PGPORT"] == environment["PGSCHEMA_PLAN_PORT"] == "55432"
        subject.database_name(environment["PGDATABASE"])


class FakeDatabase:
    calls = []
    failure = False
    difference = None
    desired_reads = 0
    versions = list(range(1, 79))
    def __init__(self, selected, deadline, directory):
        self.selected = selected
    def query(self, database, sql, parameters=None, admin=False):
        self.calls.append((database, sql, parameters, admin))
        if admin:
            assert database == "postgres" and sql.startswith('CREATE DATABASE "ortak_parity_')
            assert sql.endswith('" TEMPLATE template0')
            if self.failure: raise RuntimeError("fixture private error")
            return
        subject.database_name(database)
        if "json_agg(version" in sql: return self.versions
        if sql == subject.CATALOG:
            assert parameters == (subject.TABLES, subject.FUNCTIONS, subject.TABLES)
            value = catalog()
            if database.endswith("desired"):
                type(self).desired_reads += 1
            if self.difference and database.endswith("migrated"):
                if self.difference == "fence_targets":
                    value[self.difference][0][-1] += " semantic difference"
                else:
                    value[self.difference][0].append("semantic difference")
            return value


def hanging_query_worker(selected, database, sql, parameters, options, result_path):
    """A real unresponsive SQL-process seam, including ignored polite shutdown."""
    signal.signal(signal.SIGTERM, signal.SIG_IGN)
    subject.document(result_path, {"pid": os.getpid()})
    while True:
        time.sleep(60)


def completed_query_worker(selected, database, sql, parameters, options, result_path):
    subject.document(result_path, {"status": "ok", "value": {"database": database}})


def failed_query_worker(selected, database, sql, parameters, options, result_path):
    subject.document(result_path, {"status": "failed", "error_message": "private SQL diagnostic"})


class ParityTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name).resolve()
        self.binary = self.root / "test-binary"
        self.binary.write_bytes(b"fixture binary")
        self.binary.chmod(0o700)
        cached = patch.object(subject, "PGSCHEMA", self.binary)
        cached.start(); self.addCleanup(cached.stop)
        FakeCommands.calls = []
        FakeDatabase.calls = []
        FakeDatabase.failure = False
        FakeDatabase.difference = None
        FakeDatabase.desired_reads = 0
        FakeDatabase.versions = list(range(1, 79))

    def invoke(self, url=URL):
        return subject.probe(url, self.binary, self.root, FakeCommands, FakeDatabase)

    def test_wrong_or_absent_database_selection_stops_before_files_or_calls(self):
        for value in (None, "", URL.replace("55432", "55433"), URL.replace("55432", "5432"),
                      URL.replace("127.0.0.1", "localhost"), URL.replace("127.0.0.1", "remote.example"),
                      URL + "?host=remote", URL + "#fragment", URL.replace("/postgres", "/postgres/extra")):
            with self.subTest(value=value), self.assertRaises(subject.Refused): self.invoke(value)
        self.assertEqual(list(self.root.iterdir()), [self.binary])
        self.assertFalse(FakeCommands.calls)
        self.assertFalse(FakeDatabase.calls)
        with patch.dict(os.environ, {"DATABASE_URL": URL, "PGHOST": "remote", "PGSERVICE": "old"}):
            with self.assertRaises(subject.Refused): self.invoke(None)

    def test_cli_never_falls_back_to_database_url_or_reads_private_state(self):
        output = io.StringIO()
        args = ["check_schema_parity.py", "--migration-test-binary", str(self.binary),
                "--receipt-parent", str(self.root)]
        with patch.dict(os.environ, {"DATABASE_URL": URL, "PGPASSWORD": "must-not-read"}, clear=True), \
             patch("sys.argv", args), patch.object(subject, "probe") as probe, redirect_stdout(output):
            self.assertEqual(subject.main(), 1)
        probe.assert_not_called()
        self.assertEqual(json.loads(output.getvalue()), {"status": "failed", "code": "schema_parity_failed", "databases_retained": True})
        self.assertEqual(list(self.root.iterdir()), [self.binary])

    def test_success_retains_databases_and_protected_exact_receipt(self):
        with patch.dict(os.environ, {"DATABASE_URL": "remote", "PGSERVICE": "old", "HTTPS_PROXY": "remote"}):
            directory = self.invoke()
        intent = json.loads((directory / "intent.json").read_text())
        receipt = json.loads((directory / "receipt.json").read_text())
        self.assertEqual(receipt["status"], "verified")
        self.assertEqual(receipt["reconciliation_passes"], 2)
        self.assertEqual(receipt["migration_versions"], list(range(1, 79)))
        self.assertEqual(receipt["migration_target"], 78)
        self.assertTrue(receipt["databases_retained"])
        self.assertEqual(receipt["desired_database"], intent["desired_database"])
        self.assertNotEqual(receipt["desired_database"], receipt["migrated_database"])
        self.assertEqual(len([call for call in FakeDatabase.calls if call[3]]), 2)
        self.assertEqual(FakeDatabase.desired_reads, 2)
        for path in directory.iterdir():
            self.assertEqual(path.stat().st_mode & 0o777, 0o700 if path.is_dir() else 0o600)
            if path.is_file(): self.assertNotIn(b"selected-fixture", path.read_bytes())
        self.assertFalse(any("DROP DATABASE" in call[1] for call in FakeDatabase.calls))

    def test_creation_failure_retains_intent_and_failed_receipt_without_cleanup(self):
        FakeDatabase.failure = True
        with self.assertRaises(subject.Refused): self.invoke()
        directory = next(self.root.glob("schema-parity-*"))
        receipt = json.loads((directory / "receipt.json").read_text())
        self.assertEqual(receipt["status"], "failed")
        self.assertNotIn("fixture private error", json.dumps(receipt))
        self.assertTrue((directory / "intent.json").exists())
        self.assertFalse(FakeCommands.calls)
        self.assertFalse(any("DROP" in call[1] for call in FakeDatabase.calls))

    def test_semantic_catalog_differences_are_not_normalized_away(self):
        for component in ("columns", "indexes", "constraints", "functions", "fence_targets"):
            FakeDatabase.difference = component
            with self.assertRaises(subject.Refused): self.invoke()
        for directory in self.root.glob("schema-parity-*"):
            receipt = json.loads((directory / "receipt.json").read_text())
            self.assertEqual(receipt["status"], "failed")
            self.assertEqual(len(receipt["different_components"]), 1)
        value = copy.deepcopy(catalog())
        value["triggers"][0][5] = False
        with self.assertRaisesRegex(subject.Refused, "deferred_commit_guard"): subject.checked_catalog(value)

    def test_delete_commit_guard_must_remain_enabled_after_row_delete_and_deferred(self):
        for position, changed in ((2, "D"), (3, 5), (4, False), (5, False)):
            value = catalog()
            value["triggers"][2][position] = changed
            with self.assertRaisesRegex(subject.Refused, "deferred_commit_guard"):
                subject.checked_catalog(value)
        value = catalog()
        value["triggers"].pop(2)
        with self.assertRaisesRegex(subject.Refused, "deferred_commit_guard"):
            subject.checked_catalog(value)

    def test_activation_guards_are_bound_to_exact_table_events_and_deferred_mode(self):
        for index in range(3, 8):
            expected = "deferred_commit_guard" if index == 3 else "activation_mutation_guard"
            for position, changed in ((0, "other_table"), (2, "D"), (3, 5),
                                      (4, index != 3), (5, index != 3)):
                value = catalog()
                value["triggers"][index][position] = changed
                with self.subTest(index=index, position=position), self.assertRaisesRegex(subject.Refused, expected):
                    subject.checked_catalog(value)
            value = catalog()
            value["triggers"].pop(index)
            with self.assertRaisesRegex(subject.Refused, expected):
                subject.checked_catalog(value)
        for name in EXPECTED_FUNCTIONS[-3:]:
            value = catalog()
            value["functions"] = [row for row in value["functions"] if row[0] != name]
            with self.assertRaisesRegex(subject.Refused, "required_catalog_missing"):
                subject.checked_catalog(value)

    def test_every_universal_fence_requires_enabled_all_write_events_and_immediate_mode(self):
        for index in range(len(catalog()["fence_targets"])):
            for position, changed in ((2, "D"), (2, "R"), (3, 23), (3, 15),
                                      (4, True), (5, True)):
                value = catalog()
                value["fence_targets"][index][position] = changed
                with self.subTest(index=index, position=position, changed=changed), \
                     self.assertRaisesRegex(subject.Refused, "universal_community_fence_invalid"):
                    subject.checked_catalog(value)
        value = catalog()
        value["fence_targets"].append(["events", "community_write_fence_events", "D", 31,
                                       False, False, "definition"])
        with self.assertRaisesRegex(subject.Refused, "universal_community_fence_invalid"):
            subject.checked_catalog(value)
        value["fence_targets"][-1][2] = "O"
        self.assertEqual(subject.checked_catalog(value), value)

    def test_lifecycle_guards_reject_missing_or_weakened_commit_and_mutation_fences(self):
        for specs, deferred, expected in (
                (LIFECYCLE_DEFERRED, True, "deferred_commit_guard"),
                (LIFECYCLE_IMMEDIATE, False, "lifecycle_mutation_guard"),
                (REVIEWED_DEFERRED, True, "deferred_commit_guard"),
                (REVIEWED_IMMEDIATE, False, "reviewed_memory_mutation_guard"),
                (DEPENDENCY_IMMEDIATE, False, "dependency_mutation_guard"),
                (PROBE_DEFERRED, True, "deferred_commit_guard"),
                (PROBE_IMMEDIATE, False, "runtime_probe_mutation_guard"),
                (EXPORT_DEFERRED, True, "deferred_commit_guard"),
                (EXPORT_IMMEDIATE, False, "reviewed_export_mutation_guard"),
                (USE_DEFERRED, True, "deferred_commit_guard"),
                (USE_IMMEDIATE, False, "decomposition_or_reviewed_use_guard")):
            for table, name, kind in specs:
                index = next(i for i, row in enumerate(catalog()["triggers"]) if row[:2] == [table, name])
                for position, changed in ((0, "other_table"), (2, "D"), (3, 0), (4, not deferred), (5, not deferred)):
                    value = catalog()
                    value["triggers"][index][position] = changed
                    with self.subTest(name=name, position=position), self.assertRaisesRegex(subject.Refused, expected):
                        subject.checked_catalog(value)
                value = catalog()
                value["triggers"].pop(index)
                with self.subTest(name=name, missing=True), self.assertRaisesRegex(subject.Refused, expected):
                    subject.checked_catalog(value)

    def test_exact78_target_rejects_old77_or_unreviewed79(self):
        for versions in (list(range(1, 78)), list(range(1, 80))):
            FakeDatabase.versions = versions
            with self.assertRaises(subject.Refused): self.invoke()
        for directory in self.root.glob("schema-parity-*"):
            receipt = json.loads((directory / "receipt.json").read_text())
            self.assertEqual(receipt["error_code"], "migration78_not_proven")
            self.assertEqual(receipt["migration_target"], 78)

    def test_conversation_storage_rejects_disabled_immediate_or_wrong_commit_guards(self):
        for table, name, kind, deferred, function in subject.conversation_catalog.TRIGGERS:
            index = next(i for i, row in enumerate(catalog()['triggers']) if row[:2] == [table, name])
            for position, changed in ((2, 'D'), (3, 0), (4, not deferred), (5, not deferred),
                                      (6, 'CREATE TRIGGER fixture EXECUTE FUNCTION wrong_guard()')):
                value = catalog()
                value['triggers'][index][position] = changed
                with self.subTest(name=name, position=position), self.assertRaisesRegex(
                        subject.Refused, 'conversation_storage_trigger_invalid'):
                    subject.checked_catalog(value)
        for name in subject.conversation_catalog.FUNCTIONS:
            value = catalog()
            row = next(row for row in value['functions'] if row[0] == name)
            row[5] = True  # A security-definer substitute must not pass equal-catalog checks.
            with self.subTest(function=name), self.assertRaisesRegex(
                    subject.Refused, 'conversation_function_metadata_invalid'):
                subject.checked_catalog(value)

    def test_conversation_epoch_scope_and_sql_projection_are_explicit(self):
        self.assertEqual(subject.conversation_catalog.EPOCH_TRIGGERS, CONVERSATION_EPOCHS)
        self.assertEqual(subject.conversation_catalog.THREAD_ARGUMENTS, CONVERSATION_THREAD_ARGS)
        for name, metadata in CONVERSATION_EPOCH_FUNCTIONS.items():
            self.assertEqual(subject.conversation_catalog.FUNCTIONS[name], metadata)
        self.assertIn('WITH RECURSIVE selected AS', subject.CATALOG)
        for text in ('conversation_event_relations parent JOIN pg_inherits h ON h.inhparent=parent.oid',
                     'pg_get_expr(c.relpartbound,c.oid,false)', 'parent_t.oid=t.tgparentid',
                     'parent_n.nspname,parent_c.relname,parent_t.tgname',
                     't.tgqual IS NULL,t.tgisinternal', 'unnest(t.tgattr::smallint[])',
                     "left(t.tgname,13)='conversation_'", "'conversation_function_defaults'",
                     'pg_get_expr(proargdefaults,0)', "'conversation_indexes'",
                     "'idx_conversation_thread_parent_exact'", "'idx_conversation_thread_root_exact'",
                     "'idx_conversation_office_employee_keys'"):
            self.assertIn(text, subject.CATALOG)
        # The D4 expansion must not replace the existing73 DM selection/gate.
        self.assertIn("c.relname='channels' AND t.tgname IN('ortak_private_dm_identity','ortak_office_authority_channels')",
                      subject.CATALOG)

    def test_every_conversation_trigger_requires_presence_and_unconditional_all_columns(self):
        for index, original in enumerate(catalog()['conversation_triggers']):
            for position, replacement in ((10, ['content']), (11, False)):
                value = catalog()
                value['conversation_triggers'][index][position] = replacement
                with self.subTest(trigger=original[:2], position=position), self.assertRaisesRegex(
                        subject.Refused, 'conversation_epoch_trigger_invalid'):
                    subject.checked_catalog(value)
            value = catalog()
            value['conversation_triggers'].pop(index)
            with self.subTest(missing=original[:2]), self.assertRaisesRegex(
                    subject.Refused, 'conversation_epoch_trigger_invalid'):
                subject.checked_catalog(value)

    def test_conversation_event_clone_metadata_and_parent_link_are_not_inferred_from_name(self):
        table = 'events_fixture_early'
        index = next(i for i, row in enumerate(catalog()['conversation_triggers']) if row[0] == table)
        for position, replacement in ((0, 'events_unattached'), (1, 'unknown'), (2, 'D'),
                (3, 29), (4, True), (5, True), (6, 'other_schema'), (7, 'wrong_function'),
                (8, ''), (9, 0), (12, True), (13, None),
                (13, ['public', 'events_fixture_late', 'conversation_epoch_events'])):
            value = catalog()
            value['conversation_triggers'][index][position] = replacement
            with self.subTest(position=position, replacement=replacement), self.assertRaisesRegex(
                    subject.Refused, 'conversation_epoch_trigger_invalid'):
                subject.checked_catalog(value)
        value = catalog()
        value['conversation_triggers'].append(copy.deepcopy(value['conversation_triggers'][index]))
        with self.assertRaisesRegex(subject.Refused, 'conversation_epoch_trigger_invalid'):
            subject.checked_catalog(value)

    def test_conversation_root_trigger_modes_and_exact_thread_arguments_are_fixed(self):
        for table, name, _, _ in CONVERSATION_EPOCHS:
            for position, replacement in ((3, 31), (8, '00'), (9, 2)):
                value = catalog()
                row = next(row for row in value['conversation_triggers'] if row[:2] == [table, name])
                row[position] = replacement
                with self.subTest(name=name, position=position), self.assertRaisesRegex(
                        subject.Refused, 'conversation_epoch_trigger_invalid'):
                    subject.checked_catalog(value)
        for missing in range(len(CONVERSATION_THREAD_ARGS)):
            args = CONVERSATION_THREAD_ARGS[:missing] + CONVERSATION_THREAD_ARGS[missing + 1:]
            value = catalog()
            row = next(row for row in value['conversation_triggers']
                       if row[1] == 'ortak_office_authority_thread_metadata')
            row[8:10] = [''.join(arg + '\0' for arg in args).encode().hex(), len(args)]
            with self.subTest(missing_argument=missing), self.assertRaisesRegex(
                    subject.Refused, 'conversation_epoch_trigger_invalid'):
                subject.checked_catalog(value)

    def test_conversation_actual_partition_inventory_requires_root_and_every_attached_clone(self):
        for relations in ([], catalog()['conversation_event_relations'][:1],
                          catalog()['conversation_event_relations'][1:]):
            value = catalog()
            value['conversation_event_relations'] = relations
            with self.assertRaisesRegex(subject.Refused, 'conversation_event_partition_inventory_invalid'):
                subject.checked_catalog(value)
        for position, replacement in ((0, 'other_schema'), (2, 'other_schema'),
                (3, 'missing_parent'), (3, 'events_fixture_early'), (4, 'v'), (5, False), (6, None)):
            value = catalog()
            value['conversation_event_relations'][1][position] = replacement
            with self.subTest(position=position), self.assertRaisesRegex(
                    subject.Refused, 'conversation_event_partition_inventory_invalid'):
                subject.checked_catalog(value)
        value = catalog()
        value['conversation_event_relations'] = [value['conversation_event_relations'][0]] + [
            ['public', f'events_bound_{i}', 'public', 'events', 'r', True, 'DEFAULT']
            for i in range(1024)]
        with self.assertRaisesRegex(subject.Refused, 'conversation_event_partition_inventory_invalid'):
            subject.checked_catalog(value)
        value = catalog()
        value['conversation_event_relations'] = [value['conversation_event_relations'][0]] + [
            ['public', f'events_depth_{i}', 'public', 'events' if i == 0 else f'events_depth_{i - 1}',
             'p', True, 'DEFAULT'] for i in range(34)]
        with self.assertRaisesRegex(subject.Refused, 'conversation_event_partition_inventory_invalid'):
            subject.checked_catalog(value)
        value = catalog()
        value['conversation_event_relations'].append(
            ['public', 'events_new_partition', 'public', 'events', 'r', True, 'DEFAULT'])
        with self.assertRaisesRegex(subject.Refused, 'conversation_epoch_trigger_invalid'):
            subject.checked_catalog(value)
        value['conversation_triggers'].append(conversation_trigger('events_new_partition',
            'conversation_epoch_events', 25, False, 'ortak_conversation_epoch_mutation75',
            ('event',), ['public', 'events', 'conversation_epoch_events']))
        with self.assertRaisesRegex(subject.Refused, 'extensions77_trigger_invalid'):
            subject.checked_catalog(value)
        root = next(row for row in value['extensions77_triggers']
                    if row[:2] == ['events', 'employee_memory_epoch_events'])
        value['extensions77_triggers'].append(['events_new_partition', *root[1:13],
            ['public', 'events', 'employee_memory_epoch_events']])
        subject.checked_catalog(value)

    def test_conversation_epoch_function_metadata_and_defaults_cannot_weaken_on_both_sides(self):
        for name in CONVERSATION_EPOCH_FUNCTIONS:
            for position in range(1, 11):
                value = catalog()
                row = next(row for row in value['functions'] if row[0] == name)
                row[position] = (not row[position] if isinstance(row[position], bool)
                                 else '' if position == 10 else 'different')
                with self.subTest(name=name, position=position), self.assertRaisesRegex(
                        subject.Refused, 'conversation_function_metadata_invalid'):
                    subject.checked_catalog(value)
        for index, original in enumerate(catalog()['conversation_function_defaults']):
            value = catalog()
            value['conversation_function_defaults'][index][1:] = [1, 'false']
            with self.subTest(default=original[0]), self.assertRaisesRegex(
                    subject.Refused, 'conversation_function_defaults_invalid'):
                subject.checked_catalog(value)
            value = catalog()
            value['conversation_function_defaults'].pop(index)
            with self.subTest(missing_default=original[0]), self.assertRaisesRegex(
                    subject.Refused, 'conversation_function_defaults_invalid'):
                subject.checked_catalog(value)

    def test_conversation_epoch_indexes_require_order_predicate_and_valid_metadata(self):
        for index, original in enumerate(catalog()['conversation_indexes']):
            for position in range(len(original)):
                value = catalog()
                row = value['conversation_indexes'][index]
                row[position] = not row[position] if isinstance(row[position], bool) else 'different'
                with self.subTest(index=original[1], position=position), self.assertRaisesRegex(
                        subject.Refused, 'conversation_epoch_index_invalid'):
                    subject.checked_catalog(value)
            value = catalog()
            value['conversation_indexes'].pop(index)
            with self.subTest(missing_index=original[1]), self.assertRaisesRegex(
                    subject.Refused, 'conversation_epoch_index_invalid'):
                subject.checked_catalog(value)

    def test_activity_inventory_rejects_equal_count_substitution_and_unknown_notifications(self):
        for table, name, kind in subject.ACTIVITY_GUARDS:
            index = next(i for i, row in enumerate(catalog()["triggers"]) if row[:2] == [table, name])
            for position, changed in ((0, "wrong_table"), (1, "trg_activity_unknown"), (2, "D"),
                                      (3, 31), (4, True), (5, True)):
                value = catalog()
                value["triggers"][index][position] = changed
                # New reviewed triggers also have specific immediate guards.
                with self.subTest(name=name, position=position), self.assertRaises(subject.Refused):
                    subject.checked_catalog(value)
        value = catalog()
        value["triggers"].append(["runs", "trg_activity_extra", "O", 21, False, False, "definition"])
        with self.assertRaisesRegex(subject.Refused, "activity_notification_guard"):
            subject.checked_catalog(value)

    def test_new_context_bounds_cannot_be_missing_disabled_or_weakened(self):
        for table, name, _ in subject.INTEGRATION_CHECKS:
            index = next(i for i, row in enumerate(catalog()["constraints"]) if row[:2] == [table, name])
            for position, changed in ((2, "f"), (3, "CHECK (true)"), (4, False), (5, True), (6, True)):
                value = catalog()
                value["constraints"][index][position] = changed
                with self.subTest(name=name, position=position), self.assertRaisesRegex(subject.Refused, "decomposition_or_reviewed_use_check"):
                    subject.checked_catalog(value)
            value = catalog()
            value["constraints"].pop(index)
            with self.assertRaisesRegex(subject.Refused, "decomposition_or_reviewed_use_check"):
                subject.checked_catalog(value)
        value = catalog()
        value["fence_targets"] = [row for row in value["fence_targets"] if row[0] != "run_reviewed_memory_uses"]
        with self.assertRaisesRegex(subject.Refused, "reviewed_use_community_fence"):
            subject.checked_catalog(value)

    def test_final_migration_function_bodies_replace_bootstrap_and_old_convergence(self):
        # Bind this release's real desired/reconciler source to the immutable
        # chain's final definition, including named dollar quotes and replaced
        # functions. Live PG parity still proves the installed catalog.
        reconcile = (subject.REPO / "scripts/reconcile-schema-after-pgschema.sql").read_text()
        desired = (subject.REPO / "schema/schema.sql").read_text()
        closed_sql_bootstrap = {
            'ortak_run_workspace_current', 'ortak_conversation_scope_current',
            'ortak_conversation_thread_insert_neutral75', 'ortak_reviewed_export_eligible',
            'ortak_reviewed_runtime_eligible', 'ortak_run_reviewed_memory_current',
            'ortak_conversation_target_eligible76', 'ortak_conversation_export_eligible',
            'ortak_conversation_runtime_eligible',
        }
        closed_typed_sql_bootstrap = {
            'ortak_conversation_run_origin': '\n    SELECT NULL::bytea, NULL::bytea, NULL::timestamptz, NULL::timestamptz WHERE false\n',
            'ortak_reviewed_export_source_hash': '\n    SELECT NULL::bytea\n',
            'ortak_reviewed_export_view': '\n    SELECT NULL::jsonb\n',
        }
        closed_typed_sql_bootstrap.update(extensions77_catalog.CLOSED_BODIES)
        closed_typed_sql_bootstrap.update(dict.fromkeys(
            extensions77_catalog.CLOSED_PLPGSQL_ROW_TYPES,
            extensions77_catalog.CLOSED_PLPGSQL_BODY))
        # These two original74 SQL bodies survive pgschema unchanged. The
        # other12 workspace functions have reviewed explicit restoration;
        # a later replacement or closed stub must never inherit this exception.
        desired_only74 = {'ortak_workspace_canonical', 'ortak_workspace_profile_available'}
        final = {}
        for version in range(70, 79):
            files = list((subject.REPO / 'migrations').glob(f'{version:04}_*.sql'))
            self.assertEqual(len(files), 1, f'immutable migration {version} required')
            source = files[0].read_text()
            for match in schema_source.functions(source):
                final[match.name] = (version, match)
        reconciled = list(schema_source.functions(reconcile))
        declared = list(schema_source.functions(desired))
        extension = 'CREATE EXTENSION IF NOT EXISTS pgcrypto WITH SCHEMA public;'
        self.assertEqual(reconcile.count(extension), 1)
        for function in reconciled:
            if 'public.digest(' in function.body:
                self.assertLess(reconcile.index(extension), reconcile.index(function.statement))
        for function in declared:
            if re.search(r'\bLANGUAGE\s+SQL\b', function.with_body(''), re.I):
                self.assertNotIn('public.digest(', function.body, function.name)
        for identifier, (version, original) in final.items():
            current = [match for match in reconciled if match.name == identifier]
            required = (version != 74 or identifier not in desired_only74
                        or identifier in closed_sql_bootstrap
                        or original.statement.startswith('CREATE OR REPLACE FUNCTION'))
            if required:
                self.assertEqual(len(current), 1, identifier)
            else:
                self.assertLessEqual(len(current), 1, identifier)
            if current:
                self.assertEqual(current[0].body, original.body, identifier)
            self.assertIn(identifier, subject.FUNCTIONS)
            if version >= 72:
                current_desired = [match for match in declared if match.name == identifier]
                if identifier in extensions77_catalog.RECONCILER_ONLY:
                    self.assertEqual(version, 77, identifier)
                    self.assertEqual(current_desired, [], identifier)
                    self.assertIn('CREATE TABLE ' + extensions77_catalog.RECONCILER_ONLY[identifier], desired)
                    self.assertEqual(current[0].statement.replace('CREATE OR REPLACE FUNCTION', 'CREATE FUNCTION'),
                                     original.statement.replace('CREATE OR REPLACE FUNCTION', 'CREATE FUNCTION'), identifier)
                    continue
                self.assertEqual(len(current_desired), 1, identifier)
                expected_desired = original.statement
                if identifier in closed_sql_bootstrap:
                    self.assertEqual(current_desired[0].body, '\n    SELECT false\n', identifier)
                    expected_desired = original.with_body(current_desired[0].body)
                elif identifier in closed_typed_sql_bootstrap:
                    self.assertEqual(current_desired[0].body, closed_typed_sql_bootstrap[identifier], identifier)
                    expected_desired = original.with_body(current_desired[0].body)
                self.assertEqual(current_desired[0].statement.replace('CREATE OR REPLACE FUNCTION', 'CREATE FUNCTION'),
                                 expected_desired.replace('CREATE OR REPLACE FUNCTION', 'CREATE FUNCTION'), identifier)
        for table in ("work_decomposition", "run_reviewed_memory_uses", "run_context_snapshots"):
            self.assertIn(table, subject.TABLES)

    def test77_inventory_and_projection_cover_the_immutable_nineteen_tables(self):
        declared = re.findall(r'^CREATE TABLE (\w+)\s*\(', MIGRATION77, re.MULTILINE)
        self.assertEqual(len(declared), 19)
        self.assertEqual(set(declared), set(extensions77_catalog.TABLES))
        self.assertEqual(set(MIGRATION77_FUNCTIONS), set(extensions77_catalog.FUNCTION_NAMES))
        self.assertEqual(set(MIGRATION77_FUNCTIONS), set(extensions77_catalog.FUNCTIONS))
        self.assertEqual(set(MIGRATION77_FUNCTIONS), set(extensions77_catalog.FUNCTION_BODY_SHA256))
        # pgschema does not order dependencies hidden inside SQL dollar bodies.
        # Every final77 SQL function must be a builtin-only helper, a typed
        # closed port, or a rowtype helper created only after all tables exist.
        sql_functions = {
            name: function for name, function in MIGRATION77_FUNCTIONS.items()
            if re.search(r'\bLANGUAGE\s+SQL\b', function.with_body(''), re.I)
        }
        self.assertEqual(set(sql_functions), set(extensions77_catalog.CLOSED_BODIES)
                         | set(extensions77_catalog.BOOTSTRAP_BUILTIN_SQL)
                         | set(extensions77_catalog.RECONCILER_ONLY))
        self.assertFalse(set(extensions77_catalog.CLOSED_BODIES)
                         & set(extensions77_catalog.BOOTSTRAP_BUILTIN_SQL))
        self.assertFalse(set(extensions77_catalog.RECONCILER_ONLY)
                         & (set(extensions77_catalog.CLOSED_BODIES)
                            | set(extensions77_catalog.BOOTSTRAP_BUILTIN_SQL)))
        rowtype_signatures = {}
        for name, function in MIGRATION77_FUNCTIONS.items():
            signature = function.with_body('')
            rows = [table for table in subject.TABLES
                    if re.search(r'\b' + re.escape(table) + r'\b', signature)]
            if rows:
                self.assertEqual(len(rows), 1, name)
                rowtype_signatures[name] = rows[0]
        self.assertEqual(rowtype_signatures, extensions77_catalog.RECONCILER_ONLY)
        rowtype_declarations = {}
        for name, function in MIGRATION77_FUNCTIONS.items():
            if not re.search(r'\bLANGUAGE\s+plpgsql\b', function.with_body(''), re.I):
                continue
            declaration = re.split(r'\bBEGIN\b', function.body, maxsplit=1, flags=re.I)[0]
            rows = tuple(sorted(table for table in extensions77_catalog.TABLES
                                if re.search(r'\b' + re.escape(table) + r'\b', declaration)))
            if rows:
                rowtype_declarations[name] = rows
        self.assertEqual(rowtype_declarations, extensions77_catalog.CLOSED_PLPGSQL_ROW_TYPES)
        for name in extensions77_catalog.BOOTSTRAP_BUILTIN_SQL:
            self.assertNotRegex(sql_functions[name].body,
                                r'(?i)\b(?:FROM|JOIN|WITH)\b|\bpublic\.|\bortak_[a-z0-9_]+\s*\(')
        for key in ('extensions77_function_defaults', 'extensions77_indexes', 'extensions77_triggers'):
            self.assertIn("'" + key + "'", subject.CATALOG)
        for expression in ('t.tgqual IS NULL', 't.tgparentid', 't.tgattr::smallint[]',
                           'encode(t.tgargs', 'i.indoption::int2[]::text', 'pronargdefaults'):
            self.assertIn(expression, subject.CATALOG)
        self.assertNotIn('__EXTENSIONS77_', subject.CATALOG)
        subject.checked_catalog(catalog())
        for name in declared:
            value = catalog()
            value['tables'].remove(name)
            with self.subTest(table=name), self.assertRaisesRegex(subject.Refused, 'required_catalog_missing'):
                subject.checked_catalog(value)

    def test77_function_body_security_and_default_drift_refuses_equal_catalogs(self):
        self.assertEqual(set(MIGRATION78_FUNCTIONS), {'ortak_employee_memory_epoch_mutation'})
        name = 'ortak_employee_memory_epoch_mutation'
        value = catalog()
        row = next(row for row in value['functions'] if row[0] == name)
        self.assertEqual(row[10], MIGRATION78_FUNCTIONS[name].body)
        row[10] = MIGRATION77_FUNCTIONS[name].body
        with self.assertRaisesRegex(subject.Refused, 'extensions77_function_body_invalid'):
            subject.checked_catalog(value)
        for name in ('ortak_confidential_dm_current', 'ortak_employee_reviewed_runtime_eligible',
                     'ortak_check_routing_claim_expiry', 'ortak_routing_notify'):
            for position, changed in ((2, 'sql'), (3, 'i'), (5, True), (6, True),
                                      (8, ['search_path=pg_temp, public']), (10, '\nSELECT true\n')):
                value = catalog()
                row = next(row for row in value['functions'] if row[0] == name)
                if row[position] == changed:
                    changed = 'changed metadata'
                row[position] = changed
                with self.subTest(name=name, position=position), self.assertRaises(subject.Refused):
                    subject.checked_catalog(value)
        value = catalog()
        value['extensions77_function_defaults'][0][1:] = [1, 'true']
        with self.assertRaisesRegex(subject.Refused, 'extensions77_function_defaults_invalid'):
            subject.checked_catalog(value)

    def test77_columns_foreign_keys_checks_and_index_options_are_exact(self):
        for component, name, position, changed in (
            ('columns', ['runs', 'payload_mode'], 6, "'confidential_dm_v1'::text"),
            ('columns', ['employee_reviewed_memory_targets', 'runtime_consumption_enabled'], 6, 'true'),
            ('columns', ['confidential_run_payloads', 'envelope_bytes'], 3, False),
        ):
            value = catalog()
            row = next(row for row in value[component] if row[:2] == name)
            row[position] = changed
            with self.subTest(column=name), self.assertRaisesRegex(subject.Refused, 'extensions77_column_invalid'):
                subject.checked_catalog(value)
        for kind, changed in (('c', 'CHECK (true)'), ('f', 'FOREIGN KEY (company_id) REFERENCES companies(id)'),
                              ('p', 'PRIMARY KEY (company_id)')):
            value = catalog()
            row = next(row for row in value['constraints']
                       if row[0] == 'confidential_run_payloads' and row[2] == kind)
            row[3] = changed
            with self.subTest(constraint=kind), self.assertRaisesRegex(subject.Refused, 'extensions77_constraint_invalid'):
                subject.checked_catalog(value)
        for position, changed in ((3, '[0:1]={3,0}'), (4, False), (5, False), (6, False),
                                  (9, False), (12, 'hash'), (13, ['fillfactor=50'])):
            value = catalog()
            value['extensions77_indexes'][0][position] = changed
            with self.subTest(index_field=position), self.assertRaisesRegex(subject.Refused, 'extensions77_index_invalid'):
                subject.checked_catalog(value)

    def test77_retention_commit_modes_and_actual_event_clones_cannot_be_omitted(self):
        keys = [
            ['encrypted_dm_selections', 'encrypted_dm_selections_no_truncate'],
            ['encrypted_dm_decrypt_jobs', 'encrypted_dm_decrypt_jobs_no_truncate'],
            ['confidential_execution_leases', 'confidential_execution_at_commit'],
            ['employee_reviewed_memory_facts', 'employee_memory_fact_at_commit'],
            ['run_workspace_uses', 'confidential_no_workspace_use'],
            ['events_fixture_early', 'employee_memory_epoch_events'],
        ]
        for key in keys:
            value = catalog()
            value['extensions77_triggers'] = [row for row in value['extensions77_triggers'] if row[:2] != key]
            with self.subTest(missing=key), self.assertRaisesRegex(subject.Refused, 'extensions77_trigger_invalid'):
                subject.checked_catalog(value)
        for position, changed in ((2, 'D'), (3, 21), (4, False), (5, False),
                                  (7, 'ortak_reject_row_mutation'), (8, '00'), (9, 1),
                                  (10, ['state']), (11, False), (12, True),
                                  (13, ['public', 'events', 'unreviewed_parent'])):
            value = catalog()
            row = next(row for row in value['extensions77_triggers']
                       if row[:2] == ['confidential_execution_leases', 'confidential_execution_at_commit'])
            if row[position] == changed:
                changed = 31
            row[position] = changed
            with self.subTest(trigger_field=position), self.assertRaisesRegex(subject.Refused, 'extensions77_trigger_invalid'):
                subject.checked_catalog(value)

    def test_private_dm_triggers_require_exact_events_mode_and_function_arguments(self):
        self.assertEqual(subject.DIRECT_AUTHORITY_ARGS, DIRECT_ARGUMENTS)
        for name in ('ortak_private_dm_identity', 'ortak_office_authority_channels'):
            index = next(i for i, row in enumerate(catalog()['triggers']) if row[:2] == ['channels', name])
            for position, changed in ((0, 'events'), (1, 'unknown'), (2, 'D'), (3, 0), (4, True), (5, True)):
                value = catalog()
                value['triggers'][index][position] = changed
                with self.subTest(name=name, position=position), self.assertRaisesRegex(subject.Refused, 'private_dm_authority_guard'):
                    subject.checked_catalog(value)
            value = catalog()
            value['triggers'].pop(index)
            with self.assertRaisesRegex(subject.Refused, 'private_dm_authority_guard'):
                subject.checked_catalog(value)
        for index in (0, 1):
            for position in range(6):
                value = catalog()
                value['direct_authority'][index][position] = 'unknown'
                with self.subTest(index=index, position=position), self.assertRaisesRegex(subject.Refused, 'private_dm_authority_arguments'):
                    subject.checked_catalog(value)
        for index in range(len(DIRECT_ARGUMENTS)):
            arguments = DIRECT_ARGUMENTS[:index] + DIRECT_ARGUMENTS[index + 1:]
            value = catalog()
            value['direct_authority'][0][2] = ''.join(argument + '\0' for argument in arguments).encode().hex()
            with self.subTest(missing_argument=DIRECT_ARGUMENTS[index]), self.assertRaisesRegex(subject.Refused, 'private_dm_authority_arguments'):
                subject.checked_catalog(value)
        for changed in (None, [], catalog()['direct_authority'] + [['unknown', 'unknown', '']]):
            value = catalog()
            value['direct_authority'] = changed
            with self.assertRaisesRegex(subject.Refused, 'private_dm_authority_arguments'):
                subject.checked_catalog(value)
        value = catalog()
        value['direct_authority'][0][2] = ''.join(argument + '\0' for argument in reversed(DIRECT_ARGUMENTS)).encode().hex()
        with self.assertRaisesRegex(subject.Refused, 'private_dm_authority_arguments'):
            subject.checked_catalog(value)

    def test_snapshot_and_dm_function_metadata_cannot_be_weakened_equally(self):
        self.assertEqual(subject.SNAPSHOT_DM_FUNCTIONS, DIRECT_FUNCTIONS)
        for name, metadata in DIRECT_FUNCTIONS.items():
            index = next(i for i, row in enumerate(catalog()['functions']) if row[0] == name)
            for position, original in enumerate(metadata, 1):
                value = catalog()
                value['functions'][index][position] = not original if isinstance(original, bool) else 'different'
                with self.subTest(name=name, position=position), self.assertRaisesRegex(subject.Refused, 'snapshot_or_dm_function_contract'):
                    subject.checked_catalog(value)
            value = catalog()
            value['functions'][index][-1] = ''
            with self.assertRaisesRegex(subject.Refused, 'snapshot_or_dm_function_contract'):
                subject.checked_catalog(value)

    def test_direct_sql_hang_hits_wall_deadline_and_kills_owned_worker(self):
        selected = subject.selected_url(URL)
        database = "ortak_parity_" + "a" * 32 + "_desired"
        started = time.monotonic()
        client = subject.Database(selected, started + 1.0, self.root, hanging_query_worker)
        with self.assertRaisesRegex(subject.Refused, "sql_deadline_exceeded"):
            client.query(database, "SELECT 1")
        self.assertLess(time.monotonic() - started, 2.5)
        marker = next(self.root.glob("sql-*.json"))
        pid = json.loads(marker.read_text())["pid"]
        with self.assertRaises(ProcessLookupError):
            os.kill(pid, 0)
        self.assertEqual(marker.stat().st_mode & 0o777, 0o600)

    def test_direct_sql_result_and_private_failure_cross_only_after_worker_exit(self):
        selected = subject.selected_url(URL)
        database = "ortak_parity_" + "b" * 32 + "_migrated"
        client = subject.Database(selected, time.monotonic() + 5, self.root, completed_query_worker)
        self.assertEqual(client.query(database, "SELECT 1"), {"database": database})
        client.worker = failed_query_worker
        with self.assertRaisesRegex(subject.Refused, "^sql_query_failed$"):
            client.query(database, "SELECT invalid")
        results = [json.loads(path.read_text()) for path in self.root.glob("sql-*.json")]
        self.assertTrue(any(result.get("error_message") == "private SQL diagnostic" for result in results))

    def test_actual_output_reader_caps_mocked_child_and_stops_whole_group(self):
        read, write = os.pipe()
        os.write(write, b"oversized")
        os.close(write)
        class Child:
            pid = 987654321
            stdout = os.fdopen(read, "rb")
            def wait(self, timeout): return 0
        child = Child()
        with patch.object(subject.subprocess, "Popen", return_value=child) as spawn, \
             patch.object(subject.os, "killpg") as stop, patch.object(subject, "MAX_OUTPUT", 4):
            with self.assertRaisesRegex(subject.Refused, "output_limit"):
                subject.Commands(self.root).run("fixture", [str(self.binary)], {"PATH": "/usr/bin:/bin"})
        self.assertEqual((self.root / "fixture.log").stat().st_size, 0)
        self.assertTrue(spawn.call_args.kwargs["start_new_session"])
        self.assertEqual(spawn.call_args.kwargs["stdin"], subprocess.DEVNULL)
        stop.assert_called_once_with(child.pid, signal.SIGKILL)


if __name__ == "__main__": unittest.main()
