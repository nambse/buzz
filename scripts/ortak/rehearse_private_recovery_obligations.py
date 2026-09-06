#!/usr/bin/env python3
"""Exercise production G69 SQL against a fresh disposable55432 database.

The positive synthetic SQL seed commits through all production69 constraints
and triggers, including the reciprocal same-key claim/ACK guards. It is not
signed API acceptance or provider proof; the separate D2b suite covers those.
Adversarial row faults are transaction-local, explicitly replica-mode, and
rolled back by the unchanged production witness query. No existing database,
live private service, provider, OAuth store or source credential is accessed.
"""

import argparse
from datetime import datetime, timezone
import json
import os
from pathlib import Path
import re
import sys
from urllib.parse import quote
from uuid import uuid4

import check_schema_parity as bounded
import private_recovery_obligations as obligations

EVIDENCE = Path('/private/tmp/ortak-v0-evidence')
PSQL = Path('/Applications/Postgres.app/Contents/Versions/17/bin/psql')
BOOTSTRAP = EVIDENCE / 'integrated69-build-f0a20075be3743e2bf8ac07d3d2d06d9/buzz_db-c03468a5c62edd00'
BOOTSTRAP_SHA = '63e58e3d5eb85fc47256ca55491cb69881bfa7e7e057d98c6732a22c5e174af8'
SEED_SQL = Path(__file__).resolve().parent / 'fixtures/recovery_obligations69.sql'
FORMAT = 'ortak-recovery-obligations-disposable69/1'
URL_ENV = 'ORTAK_RECOVERY_TEST_URL'
TRIGGER_DIGEST = "SELECT encode(sha256(convert_to(jsonb_agg(jsonb_build_array(c.relname,t.tgname,t.tgenabled,pg_get_triggerdef(t.oid)) ORDER BY c.relname,t.tgname)::text,'UTF8')),'hex') FROM pg_trigger t JOIN pg_class c ON c.oid=t.tgrelid JOIN pg_namespace n ON n.oid=c.relnamespace WHERE n.nspname='public';"


def require(value, code):
    if not value:
        raise bounded.Refused(code)


def generated_database(value):
    """Only this helper's fresh UUID database can receive fixture statements."""
    require(re.fullmatch(r'ortak_g_obligations_[0-9a-f]{32}', value), 'fresh_database_scope_required')
    return value


class Fixture:
    """Bounded selected connection with no inherited libpq, provider or auth environment."""

    def __init__(self, output, selected, database):
        self.output, self.selected, self.database = output, selected, generated_database(database)
        self.commands = bounded.Commands(output)
        self.serial = 0

    def environment(self, database):
        require(database in (self.database, 'postgres'), 'database_scope_refused')
        return {'PATH': '/usr/bin:/bin:/usr/sbin:/sbin', 'HOME': str(self.output), 'LANG': 'C', 'LC_ALL': 'C',
            'PGHOST': '127.0.0.1', 'PGPORT': '55432', 'PGUSER': self.selected['user'],
            'PGPASSWORD': self.selected['password'], 'PGDATABASE': database,
            'PGCONNECT_TIMEOUT': '3', 'PGSSLMODE': 'disable', 'PGGSSENCMODE': 'disable',
            'PGOPTIONS': '-c lock_timeout=2000 -c statement_timeout=15000 '
                '-c idle_in_transaction_session_timeout=15000 -c client_min_messages=error'}

    def sql(self, statement, *, admin=False, ceiling=obligations.MAX_BYTES):
        database = 'postgres' if admin else self.database
        label = f'sql-{self.serial:03d}'; self.serial += 1
        path = self.output / (label + '.sql')
        bounded.write_private(path, statement.encode())
        self.commands.run(label, [str(PSQL), '--no-psqlrc', '--quiet', '--no-align', '--tuples-only',
            '--no-password', '--set', 'ON_ERROR_STOP=1', '--file', str(path)], self.environment(database))
        raw = (self.output / (label + '.log')).read_bytes()
        require(len(raw) <= ceiling, 'fixture_result_bound')
        return raw.strip()

    def run_test(self, label, binary, test, *, ignored):
        env = self.environment(self.database)
        url = ('postgres://' + self.selected['user'] + ':' + quote(self.selected['password'], safe='')
            + '@127.0.0.1:55432/' + self.database)
        env.update(BUZZ_TEST_DATABASE_URL=url, ORTAK_TEST_DATABASE_URL=url)
        args = [str(binary), test, '--exact', '--test-threads=1', '--nocapture']
        if ignored: args.append('--ignored')
        self.commands.run(label, args, env)
        raw = (self.output / (label + '.log')).read_bytes()
        require(b'test result: ok. 1 passed;' in raw, 'exact_one_fixture_test_required')


class WitnessCommands:
    """Run the unmodified production witness; only fault cases add an outer rollback transaction."""

    def __init__(self, fixture, fault=''):
        self.fixture, self.fault = fixture, fault

    def psql(self, database):
        require(database == self.fixture.database, 'witness_database_scope')
        return [database]

    def run(self, label, args, *, sql, ceiling):
        require(label in ('recovery-obligations', 'restored-recovery-obligations')
            and args == [self.fixture.database], 'witness_call_scope')
        prefix = ('BEGIN ISOLATION LEVEL REPEATABLE READ; SET LOCAL session_replication_role=replica; '
            + self.fault + '\n') if self.fault else ''
        # PostgreSQL's nested BEGIN leaves the isolated fault transaction open;
        # production's final ROLLBACK removes every injected fault and local GUC.
        return self.fixture.sql(prefix + sql, ceiling=ceiling)


def scope_sql(company, fact, action='withdraw'):
    """Only generated UUIDs and exact fixed actions enter synthetic SQL."""
    for value in (company, fact): require(re.fullmatch(r'[0-9a-f-]{36}', value), 'fixture_uuid_scope')
    require(action in ('publish', 'withdraw'), 'fixture_action_scope')
    return f"company_id='{company}' AND fact_id='{fact}' AND action='{action}'"


def faults(company, fact, other_community):
    """Fault injection includes states denied by schema guards, to falsify G's independent refusal."""
    where, publish = scope_sql(company, fact), scope_sql(company, fact, 'publish')
    job = 'UPDATE reviewed_memory_export_jobs SET '
    receipt = 'UPDATE reviewed_memory_export_receipts SET '
    return {
        'due_withdrawal': job + f"next_attempt_at='2000-01-01T00:00:00Z'::timestamptz WHERE {where};",
        'leased_withdrawal': job + f"lease_token=gen_random_uuid(),lease_expires_at=clock_timestamp()+INTERVAL '20 seconds',attempt_count=1,total_attempts=1 WHERE {where};",
        'expired_lease': job + f"lease_token=gen_random_uuid(),lease_expires_at=clock_timestamp()-INTERVAL '1 second',attempt_count=1,total_attempts=1 WHERE {where};",
        'attempted_future_withdrawal': job + f"attempt_count=1,total_attempts=1 WHERE {where};",
        'uncertain_future_withdrawal': job + f"last_error_code='service_retry' WHERE {where};",
        'failed_cleanup': job + f"state='failed',last_error_code='service_refused',attempt_count=1,total_attempts=1 WHERE {where};",
        'pending_publication': job + f"state='pending' WHERE {publish};",
        'missing_withdrawal': f'DELETE FROM reviewed_memory_export_jobs WHERE {where};',
        'missing_publish_ack': f'DELETE FROM reviewed_memory_export_receipts WHERE {publish};',
        'wrong_ack_request_hash': receipt + f"request_hash=decode(repeat('dd',32),'hex') WHERE {publish};",
        'wrong_ack_binding_hash': receipt + f"binding_hash=decode(repeat('dd',32),'hex') WHERE {publish};",
        'wrong_ack_lease': receipt + f"lease_token=gen_random_uuid() WHERE {publish};",
        'wrong_ack_attempt': receipt + f"total_attempts=total_attempts+1 WHERE {publish};",
        'wrong_ack_community': receipt + f"community_id='{other_community}' WHERE {publish};",
        'wrong_canonical_key': job + f"idempotency_key='reviewed:publish:unrelated' WHERE {publish};",
        'missing_retained_target': f"DELETE FROM reviewed_memory_targets WHERE company_id='{company}';",
        'wrong_target_community': f"UPDATE reviewed_memory_targets SET community_id='{other_community}' WHERE company_id='{company}';",
        'source_loss_with_pending_publication': job + f"state='pending' WHERE {publish};"
            + f"UPDATE events SET deleted_at=clock_timestamp() WHERE community_id IN(SELECT community_id FROM reviewed_memory_exports WHERE company_id='{company}');",
        'expired_uncontained_probe': "INSERT INTO provisioning_runtime_probes(company_id,operation_id,employee_id,generation,probe_id,bridge_origin,bridge_token_env,state,created_at,deadline) "
            + f"VALUES('{company}',gen_random_uuid(),'cem',1,gen_random_uuid(),'http://fixture.invalid','SYNTHETIC_REF','running','2000-01-01T00:00:00Z','2000-01-01T00:01:00Z');",
    }


def rehearse(selected):
    """Create and retain one fresh fixture database plus private evidence; never drop/reset anything."""
    require(selected['dbname'] == 'postgres', 'admin_database_must_be_postgres')
    for path, digest in [(BOOTSTRAP, BOOTSTRAP_SHA)]:
        bounded.executable(path)
        require(bounded.digest(path) == digest, 'frozen_fixture_binary_mismatch')
    bounded.executable(PSQL)
    operation = uuid4().hex
    output = EVIDENCE / ('g-obligations69-' + operation)
    output.mkdir(mode=0o700)
    database = generated_database('ortak_g_obligations_' + operation)
    receipt = {'format': FORMAT, 'status': 'started', 'database': database, 'host': '127.0.0.1', 'port': 55432,
        'operation': operation, 'provider_calls': 0, 'source_private_access': False,
        'existing_databases_modified': False, 'database_retained': True,
        'bootstrap_sha256': BOOTSTRAP_SHA, 'positive_seed': 'synthetic_SQL_all_production_guards_active',
        'signed_api_acceptance': False, 'remote_ack': 'synthetic_matching_claim_identity',
        'source_sha256': {p.name: bounded.digest(p) for p in [Path(__file__), Path(obligations.__file__), Path(bounded.__file__), SEED_SQL]}}
    bounded.document(output / 'intent.json', receipt)
    fixture = Fixture(output, selected, database)
    try:
        require(fixture.sql(f"SELECT count(*) FROM pg_database WHERE datname='{database}';", admin=True) == b'0',
            'fresh_database_already_exists')
        fixture.sql(f'CREATE DATABASE "{database}" TEMPLATE template0;', admin=True)
        fixture.run_test('bootstrap69', BOOTSTRAP, bounded.TEST, ignored=True)
        metadata = json.loads(fixture.sql("SELECT jsonb_build_object('migration_checksums',"
            "(SELECT jsonb_agg(jsonb_build_array(version,encode(checksum,'hex'),success) ORDER BY version) FROM _sqlx_migrations),"
            "'tables',(SELECT jsonb_object_agg(table_name,0) FROM information_schema.tables WHERE table_schema='public' AND table_type='BASE TABLE'));"))
        require(metadata['migration_checksums'][-1][0] == 69, 'fixture_schema69_required')
        obligations.main_contract(metadata)
        trigger_digest = fixture.sql(TRIGGER_DIGEST).decode()
        seed = SEED_SQL.read_text()
        require(len(seed.encode()) < 32768 and 'session_replication_role' not in seed
            and 'DISABLE TRIGGER' not in seed and 'ALTER TABLE' not in seed, 'positive_seed_guard_bypass_refused')
        fixture.sql(seed)
        scopes = json.loads(fixture.sql("SELECT jsonb_agg(jsonb_build_array(company_id,fact_id,community_id) ORDER BY company_id) FROM reviewed_memory_exports;"))
        require(len(scopes) == 2 and scopes[0][0] != scopes[1][0], 'exact_two_generated_fixture_scopes_required')
        company, fact, community = scopes[0]; other, _, other_community = scopes[1]
        observe = lambda commands, company=company, drained=True: obligations.observe(
            commands, database, metadata, company, drained=drained)
        baseline = observe(WitnessCommands(fixture))
        other_baseline = observe(WitnessCommands(fixture), other)
        require(len(baseline['tables']['reviewed_memory_export_jobs']) == 2
            and len(baseline['tables']['reviewed_memory_export_receipts']) == 1, 'populated_positive_witness_required')
        checks = ['production_constraints_synthetic_publication_claim_ack_future_withdrawal_allowed', 'historical_ack_lease_allowed']
        for name, fault in faults(company, fact, other_community).items():
            try:
                observe(WitnessCommands(fixture, fault))
            except obligations.Refused as error:
                require(str(error) == 'recovery_obligations_not_drained', 'wrong_fault_refusal')
            else:
                raise bounded.Refused('fault_was_accepted_' + name)
            require(observe(WitnessCommands(fixture)) == baseline, 'fault_not_rolled_back')
            require(observe(WitnessCommands(fixture), other) == other_baseline, 'other_scope_changed')
            checks.append(name)
        # Retirement of the advertisement removes new execution authority but
        # cannot erase the already-acknowledged publication's recovery identity.
        retired = f"UPDATE reviewed_memory_targets SET enabled=false,valid_until=clock_timestamp()-INTERVAL '1 second' WHERE company_id='{company}';"
        preserved = observe(WitnessCommands(fixture, retired))
        require(preserved['tables']['reviewed_memory_exports'] == baseline['tables']['reviewed_memory_exports'],
            'retired_target_lost_export_identity')
        checks.append('retired_advertisement_retains_exact_recovery_identity')
        foreign_fault = faults(other, scopes[1][1], community)['due_withdrawal']
        require(observe(WitnessCommands(fixture, foreign_fault)) == baseline, 'cross_company_fault_leaked')
        checks.append('other_company_due_job_does_not_pollute_selected_witness')
        # Expiry changes capture readiness, not the offline bytes or activation gate.
        due = faults(company, fact, other_community)['due_withdrawal']
        frozen_due = observe(WitnessCommands(fixture, due), drained=False)
        restored = obligations.verify_restore(WitnessCommands(fixture, due), database, metadata, company, frozen_due)
        require(restored['automatic_activation'] is False, 'offline_activation_opened')
        try:
            obligations.verify_restore(WitnessCommands(fixture, due), database, metadata, company, baseline)
        except obligations.Refused as error:
            require(str(error) == 'restored_recovery_obligations_changed', 'wrong_restore_refusal')
        else: raise bounded.Refused('changed_offline_rows_accepted')
        checks += ['offline_due_rows_remain_inert', 'changed_offline_full_row_hash_refuses']
        require(observe(WitnessCommands(fixture)) == baseline, 'final_source_fixture_changed')
        # Every session and injected replica flag is gone; the production schema
        # and all enabled trigger flags remain as bootstrapped.
        require(fixture.sql("SHOW session_replication_role;") == b'origin', 'fixture_replica_mode_leaked')
        require(fixture.sql(TRIGGER_DIGEST).decode() == trigger_digest, 'fixture_trigger_catalog_changed')
        for path in [Path(__file__), Path(obligations.__file__), Path(bounded.__file__), SEED_SQL]:
            require(bounded.digest(path) == receipt['source_sha256'][path.name], 'fixture_source_changed')
        receipt.update(status='passed', checks=checks, checks_passed=len(checks), migration_ledger=metadata['migration_checksums'],
            company_id=company, other_company_id=other, positive_witness=baseline,
            fault_injection='fresh_database_outer_transaction_replica_mode_rolled_back',
            trigger_catalog_sha256=trigger_digest, trigger_catalog_unchanged=True,
            completed_at=datetime.now(timezone.utc).isoformat(), automatic_activation=False)
        bounded.document(output / 'receipt.json', receipt)
        return output
    except Exception as error:
        receipt.update(status='failed', error_code=str(error) if isinstance(error, (bounded.Refused, obligations.Refused)) else type(error).__name__)
        bounded.document(output / 'failure.json', receipt)
        raise bounded.Refused('fixture_failed_private_evidence_' + output.name) from None


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--execute-disposable-fixture', action='store_true', required=True)
    parser.parse_args()
    selected = bounded.selected_url(os.environ.get(URL_ENV))
    result = rehearse(selected)
    print(json.dumps({'status': 'passed', 'receipt': str(result / 'receipt.json'), 'live_private_mutations': False}))


if __name__ == '__main__':
    try: main()
    except bounded.Refused as error:
        print(json.dumps({'status': 'refused', 'code': str(error)})); sys.exit(1)
