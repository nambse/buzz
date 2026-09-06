#!/usr/bin/env python3
"""Freeze a bounded full-stack recovery plan for the selected dated private stack.

This is executable preparation and revalidation only. It never stops services,
copies a live store/secret, creates a restore container/database, starts an
executor or calls a provider. Capture/restore remain root-coordinated operations.
"""

import argparse
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import sys
from uuid import uuid4

from backup_private_database import Refused, private_directory, private_binary
from private_native_services import private_file, selected_root
import private_recovery_inventory as inventory
import private_recovery_obligations as obligations
import recovery_native_ingress as native_ingress
import private_recovery_journal as selected_journal
import private_recovery_native_confidential as native_ciphertext
import private_recovery_scorer as scorer
import private_recovery_deployment76 as deployment76

FORMAT = 'ortak-private-recovery-preparation/1'
MAX_PLAN = 1024 * 1024
CAPTURE_LIMITS = {
    'whole_seconds': 900, 'sql_statement_seconds': 60, 'lock_seconds': 3,
    'main_database_bytes': 256 * 1024**2, 'honcho_database_bytes': 256 * 1024**2,
    'redis_bytes': 256 * 1024**2, 'minio_bytes': 2 * 1024**3,
    'sqlite_bytes': 64 * 1024**2, 'configuration_secret_bytes': 32 * 1024**2,
    'journal_raw_bytes': 192 * 1024**2 + 64 * 1024,
    'native_ciphertext_bytes': 12 * 1024**2 + 16384,
    'native_artifacts_bytes': 512 * 1024**2, 'image_exports_bytes': 8 * 1024**3,
    'per_diagnostic_bytes': 64 * 1024, 'maximum_files': 100000,
}


def canonical(value):
    """Versioned deterministic bytes for the frozen reviewable operation plan."""
    return json.dumps(value, sort_keys=True, separators=(',', ':'), ensure_ascii=True).encode()


def sha(value):
    """Hash public structure only; callers never supply credential material."""
    return hashlib.sha256(canonical(value)).hexdigest()


def save(path, value):
    """Publish once with file and directory fsync; failed preparations remain retained."""
    payload = json.dumps(value, indent=2, ensure_ascii=True).encode() + b'\n'
    inventory.require(len(payload) <= MAX_PLAN, 'preparation_record_too_large')
    with private_binary(path) as stream:
        stream.write(payload)
        stream.flush()
        os.fsync(stream.fileno())
    parent = os.open(path.parent, os.O_RDONLY)
    try:
        os.fsync(parent)
    finally:
        os.close(parent)


def runtime_bindings(values):
    """Accept only the three selected public variants and their one original OAuth store."""
    if inventory.DEPLOYMENT76_SELECTION is not None:
        return deployment76.runtime_bindings(values)
    selection = values[str(inventory.EVIDENCE / 'private-hermes-controller-selection.json')]
    config = values[str(inventory.CONTROLLER_CONFIG / 'controller/config.json')]
    deployed = values[str(inventory.RUNTIME_RECEIPT)]
    if inventory.JOURNAL_VOLUME is None:
        inventory.require(deployed['status']=='controller_handoff_verified'
            and deployed['new_id']==inventory.SERVICES['controller'][0]
            and deployed['image']==inventory.SERVICES['controller'][2]
            and deployed['old_journal_rows_preserved'] is True and deployed['oauth_reused_in_place'] is True
            and 'workspace_text_read' in deployed['capabilities']['capabilities'], 'current_runtime_rollout_refused')
    else:
        volume=values[str(inventory.CONTROLLER_CONFIG/'receipt.json')]
        inventory.require(deployed['id']==inventory.SERVICES['controller'][0]
            and deployed['image']==inventory.SERVICES['controller'][2]
            and 'workspace_text_read' in deployed['capabilities']
            and volume['status']=='journal_volume_prepared_not_activated' and volume['original_untouched'] is True
            and volume['temporary_container_removed'] is True
            and volume['selection']==selected_journal.selection(inventory.JOURNAL_VOLUME)
            ==config['executor'].get('journal_volume'), 'current_runtime_rollout_refused')
    inventory.require(selection['company_id'] == inventory.COMPANY
                      and selection['root'] == str(inventory.RUNTIME)
                      and selection['config'] == str(inventory.RUNTIME / 'controller/config.json')
                      and selection['journal'] == str(inventory.RUNTIME / 'state/journal.sqlite')
                      and selection['oauth_directory'] == str(inventory.RUNTIME / 'oauth/ada-private')
                      and config['company_id'] == inventory.COMPANY
                      and isinstance(config['profiles'], list)
                      and len(config['profiles']) == len(inventory.RUNTIME_VARIANTS),
                      'runtime_selection_refused')
    inventory.require(config['executor']['image'] == inventory.WORKER_IMAGE
                      and config['executor']['validated_digest'] == inventory.WORKER_IMAGE
                      and config['executor']['workspace_validated_digest'] == inventory.WORKER_IMAGE,
                      'runtime_binding_mismatch')
    registered = {row['directory']: row for row in config['profiles']}
    inventory.require(set(registered) == {str(path) for path, _, _ in inventory.RUNTIME_VARIANTS},
                      'runtime_variant_inventory_mismatch')
    variants = []
    for path, model, effort in inventory.RUNTIME_VARIANTS:
        binding = {'adapter': 'hermes', 'profile_ref': 'ortak-private-20260905-ada-oauth-v0',
            'model': model, 'workspace_ref': inventory.WORKSPACE_REF,
            'credential_refs': ['secret://ortak-private-20260905/ada-codex-oauth-v0'],
            'options': {'reasoning_effort': effort}}
        row = registered[str(path)]
        inventory.require(row == {'employee_id': 'ada-private', 'directory': str(path),
            'oauth_directory': selection['oauth_directory'], 'binding': binding}
            and values[str(path / 'ORTAK_RUNTIME_BINDING.json')] == binding
            and values[str(path / 'ORTAK_DISPOSABLE_PROFILE.json')] == {
                'company_id': inventory.COMPANY, 'employee_id': 'ada-private',
                'profile_ref': binding['profile_ref']}
            and values[str(path / 'ORTAK_PROVIDER.json')] == {'provider': 'openai-codex',
                'credential_ref': binding['credential_refs'][0]}, 'runtime_variant_binding_mismatch')
        variants.append(row)
    inventory.require({**variants[0]['binding'],'workspace_ref':'none'} == selection['binding'], 'runtime_binding_mismatch')
    return variants


def files():
    """Freeze explicit public files and secret metadata, never expand scope recursively."""
    public = []
    values = {}
    scoring = scorer.selection()
    for root, paths in inventory.PUBLIC_FILES.items():
        for relative in paths:
            if (inventory.DEPLOYMENT76_SELECTION is not None
                    and root == inventory.CURRENT_ROLLOUT
                    and relative == f'main-migration{inventory.MAIN_SCHEMA_VERSION}/database-after.json'):
                value, record = inventory.public_json(root, relative, maximum=256 * 1024)
            elif inventory.DEPLOYMENT76_SELECTION is not None and deployment76.is_public_path(root / relative):
                value, record = deployment76.public_file(root, relative)
            else:
                value, record = scorer.public_file(root, relative, scoring)
            public.append(record)
            values[str(root / relative)] = value
    deployment = deployment_bindings(values)
    selected_hashes = {str(inventory.API_CONFIG): inventory.API_CONFIG_SHA,
        str(inventory.WORKER_CONFIG): inventory.WORKER_CONFIG_SHA,
        str(inventory.WORKER_OWNERS): inventory.WORKER_OWNERS_SHA,
        str(inventory.CURRENT_OWNERS): inventory.CURRENT_OWNERS_SHA}
    if inventory.DEPLOYMENT76_SELECTION is not None:
        selected_hashes.update(deployment76.selected_hashes())
    inventory.require(all(row.get('sha256') == selected_hashes[row['path']]
        for row in public if row['path'] in selected_hashes), 'current_service_configuration_changed')
    secret = [inventory.file_metadata(root, relative, service_readable=True)
              for root, paths in inventory.SECRET_FILES.items() for relative in paths]
    scorer.bind_files(scoring, values, secret)
    variants = runtime_bindings(values)
    memory = values[str(inventory.STATE / 'memory/prepared-memory.json')]
    inventory.require(memory['creation_receipt']['company_id'] == inventory.COMPANY
                      and memory['creation_receipt']['employee_id'] == 'ada-private'
                      and memory['origin'] == 'http://127.0.0.1:8009'
                      and memory['token_ref'] == 'secret://ortak-private-20260905/honcho-admin',
                      'memory_selection_refused')
    return {'public': public, 'secret_metadata_only': secret,
            'opaque_bindings': {'company_id': inventory.COMPANY, 'employee_id': 'ada-private',
              'runtime': variants[0]['binding'], 'runtime_variants': variants,
              'memory_deployment_id': memory['creation_receipt']['deployment_id'],
              'memory_receipt_sha256': sha(memory['creation_receipt']), 'memory_token_ref': memory['token_ref'],
              'current_deployment': deployment}}


def deployment_bindings(values):
    """Bind successive76 worker/native replacements; historical receipts never establish current drain."""
    if inventory.DEPLOYMENT76_SELECTION is not None:
        return deployment76.deployment_bindings(values)
    root=inventory.CURRENT_ROLLOUT
    migrated=values[str(root/'main-migration76/receipt.json')]
    after=values[str(root/'main-migration76/database-after.json')]['metadata']
    inventory.require(migrated.get('status')=='migrated_verified' and migrated.get('code')=='ok'
        and migrated.get('to_schema')==inventory.MAIN_SCHEMA_VERSION
        and obligations.schema_version(after)==inventory.MAIN_SCHEMA_VERSION,'current_migration_receipt_refused')
    honcho=values[str(inventory.HONCHO_ROLLOUT/'honcho-verified.json')]
    inventory.require(honcho.get('status')=='upgraded_verified'
        and honcho.get('new_api')==inventory.SERVICES['honcho_api'][0]
        and honcho.get('new_image')==inventory.SERVICES['honcho_api'][2]
        and honcho.get('metadata_unchanged') is True and honcho.get('settings_sequences_unchanged') is True,
        'current_honcho_receipt_refused')
    owners=values[str(inventory.CURRENT_OWNERS)]
    worker_owners=values[str(inventory.WORKER_OWNERS)]
    previous=values[str(root/'current-owners76.json')]
    health=values[str(root/'live76-proof-25a1ac11c7e041778cbe413baa681dcd/receipt.json')]
    deployed=values[str(inventory.WORKER_ROLLOUT/'deployed.json')]
    launchers=values[str(root/'launcher-selection.json')]
    inventory.require(set(owners)==set(inventory.NATIVE_WRITERS)|{'native'}
        and set(worker_owners)==set(previous)==set(owners) and health['status']=='passed'
        and health['schema']==inventory.MAIN_SCHEMA_VERSION
        and health['owners']==str(root/'current-owners76.json')
        and health['health']=={'relay_liveness':200,'relay_readiness':200,'api_unauthenticated':401}
        and deployed['status']=='passed' and deployed['schema']==inventory.MAIN_SCHEMA_VERSION
        and deployed['current_owners']==str(inventory.WORKER_OWNERS)
        and deployed['current_owners_sha256']==inventory.WORKER_OWNERS_SHA
        and deployed['worker']==worker_owners['ortak-worker'] and deployed['all_other_owners_unchanged'] is True
        and deployed['no_image_or_config_or_schema_change'] is True
        and all(worker_owners[name]==previous[name] for name in worker_owners if name!='ortak-worker')
        and launchers['worker_config_sha256']==inventory.WORKER_CONFIG_SHA
        and launchers['helper_import_root_retained']==str(inventory.CURRENT_LAUNCH_HELPERS)
        and all(owners[name]['launcher']==str(inventory.NATIVE_LAUNCHERS[name]) for name in inventory.NATIVE_WRITERS)
        and all(launchers['launchers'][name]=={'path':owners[name]['launcher'],
            'sha256':owners[name]['launcher_sha256']} for name in inventory.NATIVE_WRITERS if name!='ortak-worker'),
        'current_worker_selection_refused')
    native=launchers['launchers']['native']
    built=values[str(native_ingress.BUILD_RECEIPT)]
    native_deployed=values[str(inventory.NATIVE_ROLLOUT/'deployed.json')]
    inventory.require(native['path']==worker_owners['native']['launcher']
        and native['sha256']==worker_owners['native']['launcher_sha256']
        and native['binary_sha256']==worker_owners['native']['sha256']==built['previous_sha256']
        and built['status']=='built_policy_verified' and built['source_unchanged'] is True
        and built['launcher']==owners['native']['launcher']==str(native_ingress.LAUNCHER)
        and built['launcher_sha256']==owners['native']['launcher_sha256']==native_ingress.LAUNCHER_SHA
        and built['native_sha256']==owners['native']['sha256']==native_ingress.EXPECTED_SHA
        and native_deployed['status']=='passed' and native_deployed['schema']==inventory.MAIN_SCHEMA_VERSION
        and native_deployed['current_owners']==str(inventory.CURRENT_OWNERS)
        and native_deployed['current_owners_sha256']==inventory.CURRENT_OWNERS_SHA
        and native_deployed['build_receipt']==str(native_ingress.BUILD_RECEIPT)
        and native_deployed['native']==owners['native']
        and native_deployed['four_backend_owners_unchanged'] is True
        and all(owners[name]==worker_owners[name] for name in inventory.NATIVE_WRITERS),
        'current_native_launcher_refused')
    worker=values[str(inventory.WORKER_CONFIG)]
    employees=worker.get('memory',{}).get('employees')
    inventory.require(isinstance(employees,list) and len(employees)==1 and employees[0].get('employee_id')=='ada-private'
        and employees[0].get('reviewed_runtime_projects')==[inventory.REVIEWED_PROJECT]
        and employees[0].get('reviewed_conversations')==inventory.REVIEWED_CONVERSATIONS
        and launchers['reviewed_conversations']==inventory.REVIEWED_CONVERSATIONS,
        'current_reviewed_project_selection_refused')
    selected=worker.get('workspace',{})
    inventory.require(set(selected)=={'expires_at','grants','input_root','reader_binary','reader_sha256',
        'register_selected_inputs','run_root'} and selected['register_selected_inputs'] is False
        and {**{key:selected[key] for key in ('input_root','run_root','reader_binary','reader_sha256')},
            'company_id':inventory.COMPANY,'reader_uid':os.getuid()}==inventory.WORKSPACE_SELECTION,
        'current_workspace_selection_refused')
    registration=values[str(inventory.WORKSPACE_REGISTRATION)]
    retained=registration['registry']['bindings']
    inventory.require(registration['status']=='verified' and registration['worker_mode']=='retained'
        and registration['expiry_unchanged'] is True and registration['reader_sha256']==selected['reader_sha256']
        and len(retained)==1 and selected['grants']==[retained[0]['grant']]
        and retained[0]['expires_at']==selected['expires_at']
        and retained[0]['grant']==values[str(inventory.BACKEND_ROLLOUT/'config/grant.json')]
        and retained[0]['grant']['workspace_ref']==inventory.WORKSPACE_REF,
        'current_workspace_registration_refused')
    return {'schema':inventory.MAIN_SCHEMA_VERSION,'honcho_id':inventory.SERVICES['honcho_api'][0],
        'honcho_image':inventory.SERVICES['honcho_api'][2],'api_config':str(inventory.API_CONFIG),
        'worker_config':str(inventory.WORKER_CONFIG),'reviewed_runtime_projects':[inventory.REVIEWED_PROJECT],
        'reviewed_conversations':inventory.REVIEWED_CONVERSATIONS,
        'workspace_selection':inventory.WORKSPACE_SELECTION,'workspace_ref':inventory.WORKSPACE_REF,
        'retained_workspace_registration':str(inventory.WORKSPACE_REGISTRATION),
        'historical_target_epoch_is_not_current_authority':True,'scorer_or_additional_employee_scope_added':False}


def observe(output, inspector_type=inventory.Inventory):
    """Join fresh service ownership with narrow read-only Honcho and file evidence."""
    selected_root(inventory.STATE)
    inventory.directory(inventory.RUNTIME)
    inspector = inspector_type(output)
    file_set = files()
    scoring = scorer.prepare(inspector, scorer.selection())
    containers = {name: inspector.container(name) for name in inventory.SERVICES}
    natives = {name: inspector.native(name) for name in inventory.NATIVE_WRITERS}
    native_client = native_ingress.observe(inspector)
    children = inspector.children()
    honcho = inspector.honcho(containers)
    main_database = inspector.main_database()
    protected=selected_journal.require_confidential_schema(
        inventory.JOURNAL_CONFIDENTIAL,obligations.schema_version(main_database))
    native_store=native_ciphertext.prepare(inventory.NATIVE_CONFIDENTIAL_APP_DATA,
        obligations.schema_version(main_database),native_client)
    workspace_selection = inventory.WORKSPACE_SELECTION
    obligations.workspaces.require_capture_selection(main_database,workspace_selection,inventory.COMPANY)
    # No file or process observation alone is a quiescence witness. Refuse a
    # visibly torn inventory instead of silently accepting a new generation.
    inventory.require(file_set == files(), 'configuration_changed_during_preparation')
    for name, previous in containers.items():
        inventory.require(inspector.container(name) == previous, 'container_changed_during_preparation')
    for name, previous in natives.items():
        inventory.require(inspector.native(name) == previous, 'native_changed_during_preparation')
    inventory.require(inspector.children() == children, 'contained_inventory_changed')
    inventory.require(native_ingress.observe(inspector) == native_client, 'native_ingress_changed_during_preparation')
    inventory.require(scorer.prepare(inspector, scorer.selection()) == scoring, 'scorer_changed_during_preparation')
    result = {'files': file_set, 'containers': containers, 'native_processes': natives,
            'contained_children': children, 'honcho': honcho, 'native_ingress': native_client,
            'main_database': main_database,
            'observed_at': datetime.now(timezone.utc).isoformat(),
            'quiesced': False, 'cross_store_snapshot': False}
    if workspace_selection is not None: result['workspace_selection'] = dict(workspace_selection)
    if protected is not None: result['journal_confidential']=protected
    if native_store is not None: result['native_confidential']=native_store
    if scoring is not None: result['scorer_owner']=scoring
    return result


def authority(observation):
    """Exact reviewed authority, excluding changing row counts and observation time."""
    result = {key: observation[key] for key in
              ['files', 'containers', 'native_processes', 'contained_children', 'native_ingress']}
    if 'workspace_selection' in observation: result['workspace_selection']=observation['workspace_selection']
    if 'journal_confidential' in observation: result['journal_confidential']=observation['journal_confidential']
    if 'native_confidential' in observation: result['native_confidential']=observation['native_confidential']
    if 'scorer_owner' in observation: result['scorer_owner']=observation['scorer_owner']
    main = observation['main_database']
    honcho = observation['honcho']['catalog']
    result['database_schemas'] = {
        'main': {'schema_sha256': main['schema_sha256'], 'migration_checksums': main['migration_checksums'],
                 'tables': sorted(main['tables'])},
        'honcho': {'schema_sha256': honcho['schema_sha256'], 'extensions': honcho['extensions'],
                   'owners': honcho['owners'], 'tables': sorted(honcho['tables'])},
    }
    return result


def plan_steps(operation_id, observation):
    """Return a concrete bounded dependency plan, not shell commands to run implicitly."""
    prefix = 'ortak_recovery_' + operation_id
    images = sorted({item[2] for item in inventory.SERVICES.values()} | {inventory.WORKER_IMAGE})
    if observation.get('scorer_owner') is not None:
        images=sorted(set(images)|{observation['scorer_owner']['selection']['container']['image']})
    result = {
        'operation_id': operation_id, 'limits': CAPTURE_LIMITS,
        'source_authority_sha256': sha(authority(observation)),
        'recovery_contract': obligations.stack_contract(observation['main_database'], observation['honcho']['catalog']),
        'capture_mode': 'all_selected_application_writers_quiesced_then_explicit_capture',
        'destination': {'project': 'ortak-recovery-' + operation_id,
            'main_database': prefix + '_main', 'honcho_database': prefix + '_honcho',
            'must_be_fresh': True, 'source_database_restore_forbidden': True,
            'executor': False, 'provider_egress': False, 'office_egress': False,
            'docker_socket_mount': False,
            'separate_daemon_required_for_execution_rehearsal': True},
        'images': images,
        'capture': [
            {'step': 'quiescence', 'requires': ['root_coordinated_pause', 'exact_current_process_owners',
                'admission_closed', 'durable_runtime_cancellation_or_completion', 'contained_children_stopped',
                'management_executor_stopped_and_pending_commands_drained', 'pending_work_outputs_drained',
                'all_runtime_probes_contained', 'no_leased_uncertain_due_or_failed_export_jobs',
                'future_withdrawals_frozen_with_acknowledged_publication_and_exact_target',
                'office_delivery_acknowledgements_reconciled_or_durably_unknown', 'honcho_background_writers_stopped',
                'oauth_writer_stopped_and_profile_lock_held', 'schema_fence_held'],
             'refuse_if': ['only_worker_sigterm', 'unknown_process', 'new_config_generation', 'uncertain_refresh_phase']},
            {'step': 'postgres_dumps', 'requires': ['quiescence'], 'sources': [
                {'container_id': inventory.SERVICES['postgres'][0], 'database': 'ortak', 'role': 'ortak', 'output': 'main.dump'},
                {'container_id': inventory.SERVICES['honcho_postgres'][0], 'database': inventory.HONCHO_DATABASE,
                 'role': inventory.HONCHO_ROLE, 'output': 'honcho.dump'}],
             'transaction': 'REPEATABLE READ READ ONLY with exported snapshot',
             'dump_options': ['--format=custom', '--no-password', '--lock-wait-timeout=2s'],
             'additional_evidence': ['selected_role_attributes', 'tablespaces', 'database_settings', 'extensions',
                 'migration_checksums', 'all_selected_table_counts', 'retained_receipt_bytes', 'sequence_values',
                 'scoped_probe_and_export_primary_keys_and_complete_row_hashes',
                 'honcho_reviewed_headers_content_tombstones_and_operation_receipts'],
             'role_password_material': 'encrypted_secret_envelope_only'},
            {'step': 'cold_volumes', 'requires': ['quiescence', 'redis_gracefully_stopped', 'minio_gracefully_stopped'],
             'read_only_source_volumes': [inventory.SERVICES['redis'][3], inventory.SERVICES['minio'][3]],
             'outputs': ['redis.tar', 'minio.tar'], 'include': ['complete_multipart_aof_manifest_base_increments',
                 'complete_minio_dot_metadata_and_object_versions'],
             'forbid': ['live_mutable_copy', 'aof_repair', 'host_docker_vm_path_copy']},
            {'step': 'sqlite', 'requires': ['quiescence', 'controller_stopped', 'executor_lock_held'],
             'source': str(inventory.RUNTIME / 'state/journal.sqlite'), 'method': 'SQLite backup API into new file',
             'verify': ['integrity_check', 'dense_cursors', 'permanent_run_keys_and_tombstones'],
             'preserve': ['original_cold_WAL_set_when_present'], 'output': 'journal.sqlite'},
            {'step': 'configuration_and_artifacts', 'requires': ['quiescence'],
             'public_allowlist': [row['path'] for row in observation['files']['public']],
             'immutable_native_artifacts': sorted({str(Path(row['executable']).parent)
                 for row in observation['native_processes'].values()}),
             'retain_if_present': [str(inventory.STATE / 'repos')],
             'exclude': ['pack-cache', 'previous_backups', 'old_native_profile', 'unrelated_resources']},
            {'step': 'secret_envelope', 'requires': ['quiescence', 'root_exact_scope_packaging_operation'],
             'allowlist': [row['path'] for row in observation['files']['secret_metadata_only']],
             'mode': 'authenticated_encrypted_local_archive',
             'key_handling': 'local_owner_private_recovery_key_outside_bundle_never_printed',
             'forbid': ['plaintext_secret_report', 'oauth_refresh', 'host_profile_discovery', 'automatic_key_request']},
            {'step': 'seal_and_source_resume', 'requires': ['all_components_fsynced', 'same_barrier_rechecked'],
             'on_failure': 'retain_failed_bundle_and_resume_source_only_through_original_owned_registry',
             'source_order': ['stores', 'honcho_without_implicit_new_provisioning', 'controller', 'relay_api',
                              'management_executor_under_root_control',
                              'current_adapter_witnesses', 'worker', 'explicit_admission_release']},
        ],
        'offline_restore': [
            {'step': 'fresh_isolation', 'checks': ['new_empty_destination', 'new_internal_network', 'no_source_mounts',
                'no_production_networks', 'no_socket_in_restored_controller', 'no_provider_or_office_egress'],
             'startup': 'storage_only_no_application_entrypoints'},
            {'step': 'databases', 'method': 'new_template0_databases_in_fresh_pinned_containers',
             'restore_options': ['--exit-on-error', '--single-transaction'],
             'forbid': ['--clean', '--create', 'drop_existing_database'],
             'checks': ['schema_and_migrations', 'native_and_extension_tables_together', 'receipt_bytes_and_native_ids',
                        'sequence_values', 'role_ownership', 'database_settings']},
            {'step': 'volume_and_sqlite_validation', 'checks': ['bounded_safe_archive_extraction_no_links_or_traversal',
                'complete_minio_metadata_and_versions', 'aof_load_without_truncation_or_repair',
                'sqlite_integrity_and_cursor_tombstone_agreement', 'expiry_aware_redis_evidence']},
            {'step': 'offline_secret_recovery', 'mode': 'decrypt_exact_allowlist_into_private_offline_destination',
             'checks': ['envelope_integrity', 'file_scope_modes_owner', 'no_runtime_mounts_or_refresh'],
             'health': 'unvalidated_even_if_credentials_present'},
            {'step': 'compare_and_retain', 'checks': ['all_store_identity_and_receipt_links', 'no_new_runs_or_delivery',
                'no_provider_office_requests', 'source_unchanged_by_rehearsal'],
             'promotion': 'never_automatic', 'activation_requires': obligations.ACTIVATION_GATES,
             'remaining_gate': 'separate_daemon_or_independent_host_rehearsal'},
        ],
    }


    if observation.get('workspace_selection') is not None:
        from private_recovery_workspace_files import MAX_ARCHIVE,MAX_MANIFEST
        result['limits']={**CAPTURE_LIMITS,'workspace_files_bytes':MAX_ARCHIVE+MAX_MANIFEST}
        result['capture'].insert(4,{'step':'workspace_files','selection':observation['workspace_selection'],
            'requires':['same_transaction_database_layout_and_evidence','live_held_barrier_callbacks_before_after',
                'current_stopped_reader_identity','cold_workspace_journal_closed','all_workspace_companies_selected'],
            'process_seconds':60,'outputs':['workspace-files/workspace-files.tar','workspace-files/workspace-files.json']})
        result['offline_restore'].insert(3,{'step':'workspace_files','requires':['external_manifest_database_selection_binding'],
            'method':'fresh_descriptor_anchored_physical_extraction_and_exact_readback','process_seconds':60,
            'success_required':'workspace_files_restored_offline','automatic_activation':False})
    if observation.get('scorer_owner') is not None:
        result['capture'][0]['requires'].append('selected_scorer_process_and_oauth_maintenance_terminated')
        result['capture'][0]['refuse_if'].append('zero_active_scores_without_scorer_termination')
        result['scorer_resume']={'automatic':False,'create_or_replace':False,
            'selection_sha256':observation['scorer_owner']['receipt']['sha256'],
            'container_id':observation['scorer_owner']['selection']['container']['id'],
            'requires':['same_selected_config_and_both_original_service_token_files',
                'fresh_exact_stopped_owner_and_no_oauth_writer','root_explicit_resume_after_lease_release'],
            'read_only_verifier':'private_recovery_scorer.verify_resumed'}
        result['capture'][-1]['source_order'].insert(0,
            'scorer_original_owner_after_lease_release_and_fresh_stop_proof_before_any_other_oauth_writer')
    return result


def load_preparation(path):
    """Only this helper's immutable marked preparation may be revalidated."""
    inventory.require(path.name == 'preparation.json' and path.parent.parent == inventory.STATE / 'recovery-preparations'
                      and re.fullmatch(r'[0-9a-f]{32}', path.parent.name), 'preparation_path_refused')
    inventory.directory(path.parent.parent)
    inventory.directory(path.parent)
    value = json.loads(private_file(path, MAX_PLAN))
    inventory.require(set(value) == {'format', 'status', 'created_at', 'operation_id', 'observation',
                      'plan', 'plan_sha256', 'authority_sha256', 'disk', 'limitations'}
                      and value['format'] == FORMAT and value['status'] == 'prepared'
                      and value['operation_id'] == path.parent.name
                      and value['plan_sha256'] == sha(value['plan'])
                      and value['authority_sha256'] == sha(authority(value['observation']))
                      and value['plan'] == plan_steps(value['operation_id'], value['observation']),
                      'preparation_integrity_refused')
    return value


def prepare(root, previous=None, *, observer=observe):
    """Create one retained preparation; revalidation never adds authority to an old plan."""
    inventory.require(root == inventory.STATE, 'state_scope_refused')
    selected_root(root)
    old = load_preparation(previous) if previous else None
    parent = private_directory(root / 'recovery-preparations')
    operation_id = uuid4().hex
    output = private_directory(parent / operation_id, fresh=True)
    save(output / 'intent.json', {'format': FORMAT, 'operation_id': operation_id,
        'action': 'verify_preparation' if old else 'prepare',
        'previous': str(previous) if previous else None, 'read_only_sources': True})
    try:
        observation = observer(output)
        if old:
            inventory.require(authority(observation) == authority(old['observation']), 'prepared_authority_changed')
        plan = plan_steps(operation_id, observation)
        result = {'format': FORMAT, 'status': 'prepared', 'created_at': datetime.now(timezone.utc).isoformat(),
            'operation_id': operation_id, 'observation': observation, 'plan': plan,
            'plan_sha256': sha(plan), 'authority_sha256': sha(authority(observation)),
            'disk': {'available_bytes': shutil.disk_usage(output).free,
                     'capture_budget_bytes': sum(value for key, value in plan['limits'].items() if key.endswith('_bytes'))},
            'limitations': ['no_cross_store_snapshot', 'no_quiescence_claim', 'capture_not_executed',
                'secret_contents_not_copied', 'restore_not_executed', 'no_executor_or_provider',
                'final_owned_terminal_sessions_and_honcho_lifespan_drain_require_root_coordination']}
        save(output / 'preparation.json', result)
        return output
    except (Refused, OSError, ValueError, KeyError, TypeError) as error:
        save(output / 'failure.json', {'status': 'failed', 'operation_id': operation_id,
             'error_code': str(error) if isinstance(error, Refused) else 'preparation_failed',
             'source_mutations': False, 'restore_created': False})
        raise Refused('preparation_failed_private_evidence_retained') from None


def main():
    """No capture/restore/stop mode exists until its separate quiescence gates are implemented."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--state-dir', type=Path, required=True)
    parser.add_argument('--verify-preparation', type=Path)
    args = parser.parse_args()
    output = prepare(selected_root(args.state_dir), args.verify_preparation)
    print(json.dumps({'status': 'prepared', 'preparation': str(output / 'preparation.json'),
                      'quiesced': False, 'snapshot_created': False, 'restore_created': False}))


if __name__ == '__main__':
    try:
        main()
    except (Refused, OSError, ValueError, KeyError, TypeError):
        print('Recovery preparation refused; private evidence retained when initialized. No service or restore was changed.', file=sys.stderr)
        raise SystemExit(1) from None
