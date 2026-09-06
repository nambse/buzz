#!/usr/bin/env python3
"""Retained G74 SQL and real archive restore, only on two fresh disposable55432 DBs.

Root selects exact frozen test binaries and SHA256s. Positive rows come through
the signed server fixture with production guards active and controlled adapters.
Adversarial mutations use the existing transaction-local replica fault seam and
always roll back. No live private stack, input files, provider or executor runs.
"""
import argparse
from datetime import datetime, timezone
import json
import os
from pathlib import Path
import re
from uuid import uuid4

import check_schema_parity as bounded
import private_recovery_obligations as obligations
import private_recovery_workspaces as workspace
import private_restore_credential_functions as compatibility
from private_recovery_database_metadata import selected_content, selected_extras
from rehearse_private_recovery_obligations import Fixture, WitnessCommands, TRIGGER_DIGEST
from rehearse_private_recovery_schema73 import LocalCommands, metadata, create, PG, EVIDENCE, URL_ENV
from pause_private_recovery import overall_deadline

TEST = 'work::workspace::retention::workspace_canonical_purge_requires_stop_and_preserves_all_six_tables'
SOURCE_NAMES = ('rehearse_private_recovery_schema74.py', 'rehearse_private_recovery_schema73.py',
    'rehearse_private_recovery_obligations.py', 'private_recovery_obligations.py',
    'private_recovery_workspaces.py', 'private_restore_credential_functions.py',
    'private_recovery_database_metadata.py', 'backup_private_database.py', 'backup_private_honcho.py',
    'check_schema_parity.py', 'workspace_catalog.py', 'pause_private_recovery.py')


def require(value, code):
    if not value:
        raise bounded.Refused(code)


def faults(company, other_community):
    """Only retained fixture scopes enter the rollback-only adversarial statements."""
    for value in (company, other_community):
        require(re.fullmatch(r'[0-9a-f]{8}(-[0-9a-f]{4}){3}-[0-9a-f]{12}', value),
                'fixture_scope_refused')
    where = " WHERE company_id='" + company + "'"
    return {
        'pending_action': "UPDATE workspace_tool_actions SET state='pending'" + where,
        'result_ready_action': "UPDATE workspace_tool_actions SET state='result_ready'" + where,
        'expired_reader_not_stopped': "UPDATE workspace_reader_executions SET state='running',stop_proof=NULL,stopped_at=NULL,owner_deadline='2000-01-01T00:00:00Z'" + where
            + " AND id=(SELECT id FROM workspace_reader_executions" + where + " ORDER BY id LIMIT 1)",
        'missing_prepare_proof': "DELETE FROM workspace_reader_executions" + where + " AND request_key='prepare'",
        'wrong_original_receipt_lease': 'UPDATE workspace_tool_receipts SET lease_token=gen_random_uuid()' + where,
        'receipt_attempt_newer_than_action': 'UPDATE workspace_tool_actions SET attempt_count=0' + where,
        'missing_result_receipt': 'DELETE FROM workspace_tool_receipts' + where,
        'wrong_action_community': "UPDATE workspace_tool_actions SET community_id='" + other_community + "'" + where,
        'noncanonical_grant_bytes': "UPDATE workspace_bindings SET grant_bytes=grant_bytes||convert_to(E'\\n','UTF8')" + where,
        'wrong_use_manifest_hash': "UPDATE run_workspace_uses SET manifest_hash=decode(repeat('ee',32),'hex')" + where,
        'noncanonical_result_bytes': "UPDATE workspace_tool_receipts SET result_bytes=result_bytes||convert_to(E'\\n','UTF8'),result_hash=sha256(result_bytes||convert_to(E'\\n','UTF8'))" + where,
        'wrong_result_content': "UPDATE workspace_tool_receipts SET result_bytes=convert_to(ortak_workspace_canonical(jsonb_build_object('status','failed','code','unreviewed_code')),'UTF8'),result_hash=sha256(convert_to(ortak_workspace_canonical(jsonb_build_object('status','failed','code','unreviewed_code')),'UTF8'))" + where,
    }


def rehearse(selected, binaries):
    require(selected['dbname'] == 'postgres' and selected['user'] == 'ortak', 'fixture_admin_selection')
    for path, digest in binaries.values():
        require(path.is_absolute() and path.is_relative_to(EVIDENCE)
            and re.fullmatch(r'[0-9a-f]{64}', digest), 'fixture_binary_selection')
        bounded.executable(path)
        require(bounded.digest(path) == digest, 'fixture_binary_hash')
    operation = uuid4().hex
    output = EVIDENCE / ('g-schema74-' + operation)
    output.mkdir(mode=0o700)
    for name in ('source', 'restore'): (output / name).mkdir(mode=0o700)
    source = Fixture(output / 'source', selected, 'ortak_g_obligations_' + operation)
    target = Fixture(output / 'restore', selected, 'ortak_g_obligations_' + uuid4().hex)
    receipt = {'status': 'started', 'source_database': source.database, 'restore_database': target.database,
        'host': '127.0.0.1', 'port': 55432, 'databases_retained': True,
        'existing_database_mutations': False, 'live_private_access': False, 'provider_calls': 0,
        'binary_sha256': {name: {'path': str(path), 'sha256': digest} for name, (path, digest) in binaries.items()},
        'source_sha256': {name: bounded.digest(Path(__file__).parent / name) for name in SOURCE_NAMES},
        'positive_fixture': 'signed_server_workflow_controlled_adapters_all_production_guards_active',
        'whole_operation_seconds': 900, 'automatic_activation': False}
    bounded.document(output / 'intent.json', receipt)
    try:
        create(source)
        source.run_test('bootstrap74', binaries['bootstrap'][0], bounded.TEST, ignored=True)
        initial = metadata(source)
        require(obligations.main_contract(initial)['schema_version'] == 74, 'fixture_exact_schema74')
        obligations.observe(WitnessCommands(source), source.database, initial, str(uuid4()), drained=True)
        source.run_test('populated-retention', binaries['server'][0], TEST, ignored=True)
        meta = metadata(source)
        require(meta['schema_sha256'] == initial['schema_sha256'], 'fixture_schema_changed')
        trigger_before = source.sql(TRIGGER_DIGEST)
        scopes = json.loads(source.sql("SELECT jsonb_agg(jsonb_build_array(b.company_id,b.community_id,c.deletion_state) ORDER BY b.company_id) "
            "FROM (SELECT DISTINCT company_id,community_id FROM workspace_bindings) b JOIN communities c ON c.id=b.community_id;"))
        retired = [r for r in scopes if r[2] != 'active']
        active = [r for r in scopes if r[2] == 'active']
        require(len(retired) == 1 and len(active) == 1, 'fixture_exact_retired_and_active_scopes')
        company, _, _ = retired[0]
        baseline = obligations.observe(WitnessCommands(source), source.database, meta, company, drained=True)
        require(all(baseline['tables'][name] for name in workspace.TABLE_KEYS), 'fixture_all_six_populated')
        other = obligations.observe(WitnessCommands(source), source.database, meta, active[0][0], drained=False)
        try: obligations.observe(WitnessCommands(source), source.database, meta, active[0][0], drained=True)
        except obligations.Refused as error: require(str(error) == 'recovery_obligations_not_drained', 'fixture_wrong_refusal')
        else: raise bounded.Refused('fixture_active_workspace_accepted')
        checks = ['empty74_scope_passes', 'all_six_populated_history_passes', 'active_other_scope_refuses']
        for name, statement in faults(company, active[0][1]).items():
            try: obligations.observe(WitnessCommands(source, statement + ';'), source.database, meta, company, drained=True)
            except obligations.Refused as error: require(str(error) == 'recovery_obligations_not_drained', 'fixture_wrong_fault_refusal')
            else: raise bounded.Refused('fixture_fault_not_rejected_' + name)
            require(obligations.observe(WitnessCommands(source), source.database, meta, company, drained=True) == baseline,
                    'fixture_fault_not_rolled_back')
            require(obligations.observe(WitnessCommands(source), source.database, meta, active[0][0], drained=False) == other,
                    'fixture_other_scope_changed')
            checks.append(name)
        expired = "UPDATE workspace_bindings SET verified_at='2000-01-01T00:00:00Z',expires_at='2001-01-01T00:00:00Z',revoked_at=clock_timestamp() WHERE company_id='" + company + "';"
        old = obligations.observe(WitnessCommands(source, expired), source.database, meta, company, drained=True)
        require(old != baseline, 'fixture_expiry_fault_ineffective')
        require(source.sql(TRIGGER_DIGEST) == trigger_before, 'fixture_trigger_catalog_changed')
        commands = LocalCommands(source)
        content = selected_content(commands, source.database, 'source-content', meta['tables'])
        extras = selected_extras(commands, source.database, 'source-settings')
        archive = output / 'source.dump'
        require(source.sql('SELECT count(*) FROM pg_stat_activity WHERE datname=current_database() AND pid<>pg_backend_pid();') == b'0',
                'fixture_source_clients_remain')
        source.commands.run('dump', [str(PG / 'pg_dump'), '--no-password', '--format=custom', '--lock-wait-timeout=2s',
            '--file', str(archive), source.database], source.environment(source.database))
        archive.chmod(0o600)
        require(0 < archive.stat().st_size <= 32 * 1024**2, 'fixture_archive_bound')
        create(target)
        compat = compatibility.restore_sections(LocalCommands(target), target.database, archive)
        restored = metadata(target)
        require(restored == meta, 'fixture_restored_full_metadata_changed')
        require(selected_content(LocalCommands(target), target.database, 'target-content', restored['tables']) == content,
                'fixture_restored_full_logical_bytes_changed')
        require(selected_extras(LocalCommands(target), target.database, 'target-settings') == extras, 'fixture_restored_settings_changed')
        obligations.verify_restore(WitnessCommands(target), target.database, restored, company, baseline)
        obligations.verify_restore(WitnessCommands(target), target.database, restored, active[0][0], other)
        require(metadata(source) == meta, 'fixture_source_changed')
        for name, digest in receipt['source_sha256'].items():
            require(bounded.digest(Path(__file__).parent / name) == digest, 'fixture_operator_changed')
        checks += ['expired_revoked_binding_history_passes', 'real_pg_restore_metadata_rows_settings_sequences_equal',
                   'all_six_full_row_hashes_equal', 'active_other_scope_restored_inert']
        receipt.update(status='passed', checks=checks, checks_passed=len(checks), tables=len(meta['tables']),
            schema_sha256=meta['schema_sha256'], exact_obligation_witness=baseline, restore_compatibility=compat,
            archive={'path': str(archive), 'bytes': archive.stat().st_size, 'sha256': bounded.digest(archive)},
            fault_boundary='transaction-local replica faults rolled back; positive production guards active',
            full_stack_capture=False, workspace_filesystem_capture=False,
            completed_at=datetime.now(timezone.utc).isoformat())
        bounded.document(output / 'receipt.json', receipt)
        return output
    except Exception as error:
        code = str(error) if isinstance(error, (bounded.Refused, obligations.Refused)) else 'fixture_failed'
        if len(code) > 128 or not code.replace('_', '').isalnum(): code = 'fixture_failed'
        receipt.update(status='failed', error_type=type(error).__name__, error_code=code)
        bounded.document(output / 'failure.json', receipt)
        raise bounded.Refused('schema74_fixture_failed_retained_' + operation) from None


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--execute-disposable-fixture', required=True, action='store_true')
    for name in ('bootstrap', 'server'):
        parser.add_argument('--' + name, type=Path, required=True)
        parser.add_argument('--' + name + '-sha256', required=True)
    args = parser.parse_args()
    with overall_deadline(900):
        output = rehearse(bounded.selected_url(os.environ.get(URL_ENV)),
            {name: (getattr(args, name), getattr(args, name + '_sha256')) for name in ('bootstrap', 'server')})
    print(json.dumps({'status': 'passed', 'receipt': str(output / 'receipt.json'), 'live_private_access': False}))


if __name__ == '__main__': main()
