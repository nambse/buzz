#!/usr/bin/env python3
"""Exercise G73 retention and archive restoration on two fresh disposable55432 DBs.

The exact frozen73 production tests populate signed local API workflows with
controlled runtime/memory adapters. No provider, live private stack, Docker or
existing database is touched. All fixture databases and bounded evidence remain.
"""
import argparse
from datetime import datetime, timezone
import json
import os
from pathlib import Path
from uuid import uuid4

import check_schema_parity as bounded
import private_recovery_obligations as obligations
import private_restore_credential_functions as compatibility
from backup_private_database import METADATA_SQL
from private_recovery_database_metadata import selected_content, selected_extras
from rehearse_private_recovery_obligations import Fixture, WitnessCommands, TRIGGER_DIGEST
from pause_private_recovery import overall_deadline

BUILD = Path('/private/tmp/ortak-v0-evidence/integrated73-build-47b0daec06fe4f4189ae8fbb31b936cb')
BINARIES = {
    'buzz_db-c03468a5c62edd00': '7041b9db9bafb642cb03af0255f413683336a5e35e2153df486bbcaedfe27d84',
    'postgres_authenticated_routes-6a216032cfe2faf9': '6116fafdae21e29bf7c05f1bd9479046f73ae4f06b7d7da4d87fdda83bcd6f48',
}
TESTS = (
    'work::decomposition::storage::retention::decomposition_retention_survives_canonical_purge_without_reusing_retained_links_as_authority',
    'work::reviewed_exports::retention::reviewed_export_canonical_purge_waits_for_cleanup_and_retains_exact_receipts',
    'work::reviewed_exports::runtime::json_compat::reviewed_runtime_json_scratch_keeps_exact_bytes_budget_and_reviewed_guards',
    'direct::private_dm_activity_is_participant_only_and_archived_recovery_remains_visible',
)
PG = Path('/Applications/Postgres.app/Contents/Versions/17/bin')
EVIDENCE = BUILD.parent
URL_ENV = 'ORTAK_RECOVERY_TEST_URL'
SOURCE_NAMES = ('rehearse_private_recovery_schema73.py', 'rehearse_private_recovery_obligations.py',
    'private_recovery_obligations.py', 'private_restore_credential_functions.py',
    'private_recovery_database_metadata.py', 'backup_private_database.py',
    'backup_private_honcho.py', 'check_schema_parity.py', 'pause_private_recovery.py')


def require(value, code):
    if not value:
        raise bounded.Refused(code)


class LocalCommands:
    """Only one newly created selected fixture can receive production read/restore calls."""

    def __init__(self, fixture):
        self.fixture, self.root = fixture, fixture.output

    def psql(self, database):
        require(database == self.fixture.database, 'fixture_database_scope')
        return ['selected_psql', database]

    def command(self, program, *args):
        require(program == 'pg_restore' and args[-1] == self.fixture.database
            and args[-6:] == ('-h', '/var/run/postgresql', '-U', 'ortak', '-d', self.fixture.database),
            'fixture_restore_command_scope')
        return [str(PG / program), *args[:-6], '-d', self.fixture.database]

    def run(self, label, args, *, sql=None, ceiling=512 * 1024, archive=None):
        if sql is not None:
            require(args == self.psql(self.fixture.database) and archive is None, 'fixture_sql_scope')
            return self.fixture.sql(sql, ceiling=ceiling)
        require(archive is not None and sql is None and Path(archive).name == 'source.dump'
            and Path(archive).parent == self.root.parent and args[0] == str(PG / 'pg_restore')
            and args[-1] == self.fixture.database, 'fixture_archive_scope')
        self.fixture.commands.run(label, [*args, str(archive)], self.fixture.environment(self.fixture.database))
        return b''


def metadata(fixture):
    return json.loads(fixture.sql('BEGIN READ ONLY;\n' + METADATA_SQL + '\nROLLBACK;', ceiling=512 * 1024))


def create(fixture):
    require(fixture.sql("SELECT count(*) FROM pg_database WHERE datname='" + fixture.database + "';", admin=True) == b'0',
            'fixture_database_already_exists')
    fixture.sql('CREATE DATABASE "' + fixture.database + '" TEMPLATE template0;', admin=True)


def rehearse(selected):
    require(selected['dbname'] == 'postgres' and selected['user'] == 'ortak', 'fixture_admin_selection')
    for name, digest in BINARIES.items():
        bounded.executable(BUILD / name)
        require(bounded.digest(BUILD / name) == digest, 'fixture_binary_hash')
    for name in ('pg_dump', 'pg_restore'):
        bounded.executable(PG / name)
    operation = uuid4().hex
    output = EVIDENCE / ('g-schema73-' + operation)
    output.mkdir(mode=0o700)
    source_dir, restore_dir = output / 'source', output / 'restore'
    source_dir.mkdir(mode=0o700); restore_dir.mkdir(mode=0o700)
    source = Fixture(source_dir, selected, 'ortak_g_obligations_' + operation)
    target = Fixture(restore_dir, selected, 'ortak_g_obligations_' + uuid4().hex)
    receipt = {'status': 'started', 'host': '127.0.0.1', 'port': 55432,
        'source_database': source.database, 'restore_database': target.database,
        'databases_retained': True, 'existing_database_mutations': False,
        'live_private_access': False, 'provider_calls': 0, 'automatic_activation': False,
        'binary_sha256': BINARIES, 'positive_fixture': 'frozen_production_tests_controlled_remote_adapters',
        'whole_operation_seconds': 900,
        'source_sha256': {name: bounded.digest(Path(__file__).parent / name) for name in SOURCE_NAMES}}
    bounded.document(output / 'intent.json', receipt)
    try:
        create(source)
        source.run_test('bootstrap73', BUILD / 'buzz_db-c03468a5c62edd00', bounded.TEST, ignored=True)
        initial = metadata(source)
        require(obligations.main_contract(initial)['schema_version'] == 73, 'fixture_exact_schema73')
        for index, test in enumerate(TESTS):
            source.run_test('production-' + str(index), BUILD / 'postgres_authenticated_routes-6a216032cfe2faf9', test, ignored=True)
        meta = metadata(source)
        require(meta['schema_sha256'] == initial['schema_sha256'], 'fixture_production_schema_changed')
        trigger_before = source.sql(TRIGGER_DIGEST)
        rows = json.loads(source.sql("SELECT jsonb_agg(jsonb_build_array(u.company_id,u.run_id,r.status,f.revoked_at IS NOT NULL) ORDER BY u.company_id,u.run_id) "
            "FROM run_reviewed_memory_uses u JOIN runs r ON r.company_id=u.company_id AND r.id=u.run_id "
            "JOIN reviewed_memory_facts f ON f.company_id=u.company_id AND f.id=u.fact_id;"))
        retired = [row for row in rows if row[2] in ('completed', 'failed', 'cancelled') and row[3]]
        active = [row for row in rows if row[2] not in ('completed', 'failed', 'cancelled')]
        require(retired and active, 'fixture_retired_and_active_uses_required')
        companies = {row[0] for row in retired}
        decomposition = json.loads(source.sql('SELECT jsonb_agg(DISTINCT company_id) FROM work_decomposition;'))
        require(decomposition, 'fixture_populated_decomposition_required')
        companies.update(decomposition)
        expected = {company: obligations.observe(WitnessCommands(source), source.database, meta, company, drained=True)
            for company in sorted(companies)}
        checks = ['terminal_revoked_reviewed_use_retained', 'company_decomposition_after_community_purge_retained']
        for company, _, _, _ in active:
            try: obligations.observe(WitnessCommands(source), source.database, meta, company, drained=True)
            except obligations.Refused as error:
                require(str(error) == 'recovery_obligations_not_drained', 'fixture_wrong_active_refusal')
            else: raise bounded.Refused('fixture_active_use_accepted')
        checks.append('actual_active_use_refuses_capture')
        company = retired[0][0]
        fault = "UPDATE run_reviewed_memory_uses SET expires_at='2000-01-01T00:00:00Z' WHERE company_id='" + company + "';"
        expired = obligations.observe(WitnessCommands(source, fault), source.database, meta, company, drained=True)
        require(expired != expected[company], 'fixture_expiry_fault_ineffective')
        obligations.verify_restore(WitnessCommands(source, fault), source.database, meta, company, expired)
        checks.append('historical_expired_use_bytes_are_evidence_not_authority')
        require(source.sql(TRIGGER_DIGEST) == trigger_before, 'fixture_trigger_catalog_changed')
        source_command = LocalCommands(source)
        content = selected_content(source_command, source.database, 'source-content', meta['tables'])
        extras = selected_extras(source_command, source.database, 'source-settings')
        archive = output / 'source.dump'
        require(source.sql("SELECT count(*) FROM pg_stat_activity WHERE datname=current_database() AND pid<>pg_backend_pid();") == b'0',
                'fixture_source_clients_remain')
        source.commands.run('dump', [str(PG / 'pg_dump'), '--no-password', '--format=custom', '--lock-wait-timeout=2s',
            '--file', str(archive), source.database], source.environment(source.database))
        archive.chmod(0o600)
        require(0 < archive.stat().st_size <= 32 * 1024**2, 'fixture_archive_bound')
        create(target)
        restored_compatibility = compatibility.restore_sections(LocalCommands(target), target.database, archive)
        restored = metadata(target)
        require(restored == meta, 'fixture_restored_full_metadata_changed')
        require(selected_content(LocalCommands(target), target.database, 'target-content', restored['tables']) == content,
                'fixture_restored_full_logical_bytes_changed')
        require(selected_extras(LocalCommands(target), target.database, 'target-settings') == extras,
                'fixture_restored_settings_or_sequences_changed')
        for company, witness in expected.items():
            obligations.verify_restore(WitnessCommands(target), target.database, restored, company, witness)
        require(metadata(source) == meta, 'fixture_source_changed_during_restore')
        checks += ['complete_archive_raw_schema_and_function_catalog_equal', 'all_table_counts_and_logical_bytes_equal',
                   'raw_json_snapshot_bytes_and_dm_pair_ttl_rows_preserved', 'settings_and_sequences_equal',
                   'historical_obligation_hashes_equal_after_real_pg_restore']
        for name, digest in receipt['source_sha256'].items():
            require(bounded.digest(Path(__file__).parent / name) == digest, 'fixture_source_changed')
        receipt.update(status='passed', checks=checks, checks_passed=len(checks), tables=len(meta['tables']),
            archive={'path': str(archive), 'bytes': archive.stat().st_size, 'sha256': bounded.digest(archive)},
            schema_sha256=meta['schema_sha256'], exact_obligation_witnesses=expected,
            logical_rows_sha256=content, restore_compatibility=restored_compatibility,
            fault_boundary='expired-use-only transaction-local replica fault rolled back; production positive guards active',
            full_stack_capture=False, completed_at=datetime.now(timezone.utc).isoformat())
        bounded.document(output / 'receipt.json', receipt)
        return output
    except Exception as error:
        code = str(error) if isinstance(error, (bounded.Refused, obligations.Refused)) else 'fixture_failed'
        if len(code) > 128 or not code.replace('_', '').isalnum(): code = 'fixture_failed'
        receipt.update(status='failed', error_type=type(error).__name__, error_code=code)
        bounded.document(output / 'failure.json', receipt)
        raise bounded.Refused('schema73_fixture_failed_retained_' + operation) from None


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--execute-disposable-fixture', action='store_true', required=True)
    parser.parse_args()
    with overall_deadline(900):
        result = rehearse(bounded.selected_url(os.environ.get(URL_ENV)))
    print(json.dumps({'status': 'passed', 'receipt': str(result / 'receipt.json'), 'live_private_access': False}))


if __name__ == '__main__':
    main()
