#!/usr/bin/env python3
"""Verify one fresh retained native+extension Honcho backup; never restore a source."""

import argparse
from contextlib import contextmanager
from datetime import datetime, timezone
import json
import os
from pathlib import Path
import re
import selectors
import shutil
import subprocess
from uuid import uuid4

from backup_private_database import Commands, Refused, MAX_DUMP, digest, environment, private_directory
from private_native_services import selected_root
from prepare_private_recovery import save
import private_recovery_inventory as inventory

FORMAT = 'ortak-private-honcho-backup/1'
SOURCE = inventory.HONCHO_DATABASE
ROLE = inventory.HONCHO_ROLE
PREFIX = 'ortak_honcho_verify_'
MAX_ROWS = 200000
SNAPSHOT = r'[0-9A-F]{8}-[0-9A-F]{8}-[0-9]+'

# All row bytes stay in the selected database. Deterministic JSONB row hashes
# cover native IDs and complete receipt bodies, preserving duplicate rows.
# These hashes are private recovery evidence, never console output.
CONTENT_SQL = r"""
SELECT jsonb_object_agg(name,digest) FROM (
 SELECT format('%I.%I',n.nspname,c.relname) name,
 (xpath('/table/row/digest/text()',query_to_xml(format(
  'SELECT encode(sha256(convert_to(COALESCE(string_agg(h,'''' ORDER BY h),''''),''UTF8'')),''hex'') digest FROM (SELECT encode(sha256(convert_to(to_jsonb(t)::text,''UTF8'')),''hex'') h FROM %I.%I t) hashes',
   n.nspname,c.relname),false,false,'')))[1]::text digest
 FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace
 WHERE n.nspname='public' AND c.relkind IN ('r','p')
) hashes;
"""
SCOPES_SQL = "SELECT jsonb_agg(nspname ORDER BY nspname) FROM pg_namespace WHERE nspname NOT LIKE 'pg_%' AND nspname<>'information_schema';"


def verification_name(value):
    """Only a generated new verification database is a possible restore target."""
    inventory.require(isinstance(value, str) and re.fullmatch(PREFIX + r'[0-9a-f]{32}', value),
                      'honcho_verification_name_refused')
    return value


def snapshot_name(value):
    """Snapshot identifiers cannot carry SQL or choose another connection."""
    inventory.require(isinstance(value, str) and re.fullmatch(SNAPSHOT, value), 'snapshot_identifier_refused')
    return value


class HonchoCommands(Commands):
    """Reuse bounded child-process handling, with a separate exact Honcho authority."""

    def __init__(self, root):
        super().__init__(root)
        self.restore_authority = None

    def inspect(self):
        """Require saved/live API setting, exact database image/mount and SQL identity."""
        inspector = inventory.Inventory(self.root)
        inspector.commands = self
        containers = {name: inspector.container(name) for name in ['honcho_api', 'honcho_postgres']}
        selection = inspector.honcho(containers)
        self.container = containers['honcho_postgres']['id']
        return {'containers': containers, 'selection': selection['saved_selection'],
                'live_api_selection': selection['live_api_selection']}

    def psql(self, database):
        """No connection can select an ambient role, port, password or database."""
        if database != SOURCE:
            verification_name(database)
        return self.command('psql', '--no-psqlrc', '--quiet', '--no-align', '--tuples-only',
            '--no-password', '--set', 'ON_ERROR_STOP=1', '-h', '/var/run/postgresql',
            '-U', ROLE, '-d', database)

    @contextmanager
    def snapshot(self):
        """Hold an exported read-only source transaction; finite container timeout survives transport loss."""
        process = subprocess.Popen(self.psql(SOURCE), stdin=subprocess.PIPE,
            stdout=subprocess.PIPE, stderr=subprocess.PIPE, env=environment(), start_new_session=True)
        try:
            process.stdin.write(b'BEGIN ISOLATION LEVEL REPEATABLE READ READ ONLY;\nSELECT pg_export_snapshot();\n')
            process.stdin.flush()
            value = bytearray()
            with selectors.DefaultSelector() as ready:
                ready.register(process.stdout, selectors.EVENT_READ)
                ready.register(process.stderr, selectors.EVENT_READ)
                while b'\n' not in value:
                    events = ready.select(min(5, self.remaining()))
                    inventory.require(events, 'snapshot_start_deadline_exceeded')
                    for key, _ in events:
                        block = os.read(key.fileobj.fileno(), 1024)
                        inventory.require(key.fileobj is not process.stderr and block, 'snapshot_start_failed')
                        value.extend(block)
                        inventory.require(len(value) <= 128, 'snapshot_response_refused')
            snapshot = snapshot_name(value.decode().strip())
            yield snapshot
            process.stdin.write(b'ROLLBACK;\n\\q\n')
            process.stdin.flush()
            inventory.require(process.wait(timeout=min(3, self.remaining())) == 0, 'snapshot_holder_failed')
        finally:
            self.stop(process)

    def metadata(self, database, label, snapshot=None):
        """Compare one snapshot's native+extension schema, counts and full logical row bytes."""
        start = 'BEGIN ISOLATION LEVEL REPEATABLE READ READ ONLY;\n'
        if snapshot:
            start += "SET TRANSACTION SNAPSHOT '" + snapshot_name(snapshot) + "';\n"
        start += ("DO $$BEGIN IF current_user<>'ortak_honcho' OR (SELECT count(*) FROM pg_class c "
            "JOIN pg_namespace n ON n.oid=c.relnamespace WHERE n.nspname='public' AND c.relkind IN ('r','p'))>2048 "
            "THEN RAISE EXCEPTION 'scope refused'; END IF; END$$;\n")
        raw = self.run(label, self.psql(database), sql=start + SCOPES_SQL + '\n' + inventory.HONCHO_METADATA + '\nROLLBACK;\n')
        lines = raw.decode().splitlines()
        inventory.require(len(lines) == 2 and json.loads(lines[0]) == ['public'], 'honcho_schema_scope_refused')
        data = json.loads(lines[1])
        inventory.require(data['database'] == database and data['role'] == ROLE
            and data['owners'] == [ROLE] and 'vector' in data['extensions']
            and re.fullmatch(r'[0-9a-f]{64}', data['schema_sha256'])
            and 0 < len(data['tables']) <= 2048
            and all(type(n) is int and n >= 0 for n in data['tables'].values())
            and sum(data['tables'].values()) <= MAX_ROWS
            and all('public.' + name in data['tables'] for name in
                ['ortak_resource_receipts', 'ortak_session_ownership', 'ortak_write_receipts']),
            'honcho_database_metadata_refused')
        # A restored target has no writers. Source content comparisons must
        # import the same snapshot as the earlier count/schema observation.
        inventory.require(database != SOURCE or snapshot is not None, 'source_snapshot_required')
        content = json.loads(self.run(label + '-content', self.psql(database),
            sql=start + CONTENT_SQL + '\nROLLBACK;\n'))
        inventory.require(set(content) == set(data['tables']) and all(isinstance(h, str)
            and re.fullmatch(r'[0-9a-f]{64}', h) for h in content.values()), 'honcho_content_digest_refused')
        del data['database']  # Identity was checked above; destination is intentionally a fresh name.
        data['logical_rows_sha256'] = content
        return data

    def create_target(self, target):
        """createdb fails on any occupied generated destination; no reuse or overwrite path exists."""
        target = verification_name(target)
        self.run('create-verification', self.command('createdb', '--no-password', '-h', '/var/run/postgresql',
            '-U', ROLE, '--maintenance-db=' + SOURCE, '--template=template0', '--owner=' + ROLE, target))
        self.restore_authority = target

    def restore(self, target, archive, *, source_checks=None):
        """Only an exact generated target receives the new local archive; warnings/errors refuse."""
        target = verification_name(target)
        inventory.require(self.restore_authority == target, 'fresh_restore_authority_required')
        self.restore_authority = None  # An uncertain/partial restore is retained, never blindly replayed.
        self.run('restore', self.command('pg_restore', '--no-password', '--exit-on-error', '--single-transaction',
            '-h', '/var/run/postgresql', '-U', ROLE, '-d', target), archive=archive)
        from private_restore_honcho_checks import repair_checks
        self.honcho_check_restore_authority = target
        return repair_checks(self, target, source_checks)


def backup(root, commands_type=HonchoCommands):
    """Retain a new backup and verification database, without any source or API mutation."""
    inventory.require(root == inventory.STATE, 'state_scope_refused')
    selected_root(root)
    parent = private_directory(root / 'honcho-backups')
    identifier = datetime.now(timezone.utc).strftime('%Y%m%dT%H%M%SZ') + '_' + uuid4().hex
    destination = private_directory(parent / identifier, fresh=True)
    target = verification_name(PREFIX + uuid4().hex)
    manifest = {'format': FORMAT, 'operation_id': identifier, 'status': 'started',
        'source_database': SOURCE, 'verification_database': target, 'database_only': True,
        'same_source_container': True, 'cross_store_snapshot': False, 'source_mutations': False,
        'application_endpoint_changed': False, 'executor_started': False, 'provider_requests': False,
        'limitations': ['not_an_independent_host_restore', 'no_global_roles_backup',
                        'no_sequence_current_value_comparison', 'no_cross_store_quiescence']}
    save(destination / 'intent.json', manifest)
    command = commands_type(destination)
    try:
        manifest['source'] = command.inspect()
        size = int(command.run('database-size', command.psql(SOURCE),
                               sql='SELECT pg_database_size(current_database());\n', ceiling=128))
        inventory.require(0 < size <= MAX_DUMP and shutil.disk_usage(destination).free >= MAX_DUMP + 2 * size,
                          'honcho_backup_capacity_refused')
        manifest['source_database_bytes'] = size
        with command.snapshot() as snapshot:
            manifest['snapshot'] = snapshot
            manifest['expected'] = command.metadata(SOURCE, 'source', snapshot)
            from private_restore_honcho_checks import source_checks
            manifest['source_checks'] = source_checks(command, SOURCE, snapshot)
            archive = destination / 'honcho.dump'
            command.run('dump', command.command('pg_dump', '--format=custom', '--no-password',
                '--lock-wait-timeout=2s', '--snapshot=' + snapshot_name(snapshot), '-h', '/var/run/postgresql',
                '-U', ROLE, '-d', SOURCE), output=archive, ceiling=MAX_DUMP)
        manifest['archive_bytes'] = archive.stat().st_size
        manifest['archive_sha256'] = digest(archive)
        # Journal exact destination intent before creation; any partial target remains retained.
        save(destination / 'restore-intent.json', {'verification_database': target,
            'container_id': command.container, 'source_database_restore_forbidden': True,
            'archive_sha256': manifest['archive_sha256'], 'archive_bytes': manifest['archive_bytes']})
        command.create_target(target)
        manifest['verification_created'] = True
        manifest['restore_check_compatibility'] = command.restore(target, archive, source_checks=manifest['source_checks'])
        manifest['restored'] = command.metadata(target, 'restored')
        if manifest['expected'] != manifest['restored']:
            manifest['different_fields'] = sorted(k for k in manifest['expected'].keys() | manifest['restored'].keys()
                                                  if manifest['expected'].get(k) != manifest['restored'].get(k))
            raise Refused('honcho_restored_metadata_mismatch')
        manifest['status'] = 'verified'
    except (Refused, OSError, ValueError, KeyError, TypeError, subprocess.SubprocessError) as error:
        manifest['status'] = 'failed'
        manifest['error_code'] = str(error) if isinstance(error, Refused) else 'honcho_backup_failed'
        save(destination / 'manifest.json', manifest)
        raise Refused('honcho_backup_failed_private_evidence_retained', receipt_path=destination / 'manifest.json') from None
    save(destination / 'manifest.json', manifest)
    return destination


def main():
    """No alternate target, overwrite, cleanup, credential or service action is accepted."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--state-dir', type=Path, required=True)
    args = parser.parse_args()
    destination = backup(selected_root(args.state_dir))
    print(json.dumps({'status': 'verified', 'manifest': str(destination / 'manifest.json'),
                      'database_only': True, 'source_mutations': False, 'restore_retained': True}))


if __name__ == '__main__':
    try:
        main()
    except (Refused, OSError, ValueError, KeyError, TypeError):
        raise SystemExit('Honcho backup refused; private artifacts and any fresh verification database remain retained. Source was not replaced.') from None
