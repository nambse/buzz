#!/usr/bin/env python3
"""Explicit retained-database/read-only-volume fixtures; no source application or credential access."""

import argparse
import json
from pathlib import Path
import tarfile
from unittest.mock import patch
from uuid import uuid4

from backup_private_database import Commands, Refused, digest, private_directory, verification_name
from backup_private_honcho import HonchoCommands, verification_name as honcho_verification_name
from prepare_private_recovery import save
from private_recovery_database_metadata import selected_content, selected_extras
import private_recovery_inventory as inventory
from private_recovery_payloads import VOLUME_READER
import private_recovery_schema_lease as schema

MAIN_RECEIPT = inventory.STATE / 'backups/20260905T161716Z_f2d4e29921d248dda62d7d0081b246ce/manifest.json'
HONCHO_RECEIPT = inventory.STATE / 'honcho-backups/20260905T172845Z_14551996b8ad42dcbe65faf47b00a8f4/manifest.json'
LOCK = '7094711454081051697'


def retained_target(path, archive, validate):
    """Only the already-verified retained fixture target is read; source and arbitrary targets refuse."""
    receipt, _ = inventory.public_json(path.parent, path.name)
    inventory.require(receipt['status'] == 'verified' and receipt['verification_created'] is True,
                      'retained_database_not_verified')
    target = validate(receipt['verification_database'])
    inventory.require(digest(path.parent / archive) == receipt['archive_sha256'], 'retained_archive_changed')
    return target, receipt['restored']


def volume_args(command, output, name, image, limit):
    """A new fixture-only Linux volume is the sole read-only mount; no socket or network."""
    return command.docker('run', '--pull', 'never', '--name', name,
        '--label', 'org.ortak.recovery_fixture=' + output.name, '--network', 'none',
        '--read-only', '--user', '0:0', '--cap-drop', 'ALL', '--cap-add', 'DAC_OVERRIDE',
        '--security-opt', 'no-new-privileges', '--pids-limit', '16', '--memory', '64m',
        '--mount', 'type=volume,source=ortak_recovery_fixture_' + output.name + ',target=/capture-source,readonly,volume-nocopy',
        '--entrypoint', '/usr/local/bin/python', image, '-u', '-c', VOLUME_READER, str(limit))


def execute():
    """Retain SQL/lease/volume fixture evidence without writes to any existing database or volume."""
    inventory.directory(inventory.STATE)
    output = private_directory(private_directory(inventory.STATE / 'recovery-foundation-fixtures') / uuid4().hex, fresh=True)
    command = Commands(output)
    manifest = {'format': 'ortak-private-recovery-foundation-fixture/1', 'status': 'started',
        'source_database_writes': False, 'provider_requests': False, 'credential_reads': False,
        'production_volume_mounts': False, 'network': 'none', 'docker_socket': False}
    save(output / 'intent.json', manifest)
    try:
        target, expected = retained_target(MAIN_RECEIPT, 'database.dump', verification_name)
        honcho_target, honcho_expected = retained_target(HONCHO_RECEIPT, 'honcho.dump', honcho_verification_name)
        command.inspect()
        inventory.require(command.metadata(target, 'main-metadata') == expected, 'retained_main_database_changed')
        extras = selected_extras(command, target, 'main-extras')
        content = selected_content(command, target, 'main-content', expected['tables'])
        manifest['main'] = {'database': target, 'tables': len(content),
                            'sequences': len(extras['sequences'] or {}), 'settings_query_passed': True}

        # The production lease is exercised against the exact retained test
        # database. PostgreSQL advisory locks are database scoped: this cannot
        # block the live source migration fence.
        class RetainedSchemaCommands(schema.SchemaCommands):
            def psql(self, database):
                inventory.require(database == 'ortak', 'fixture_schema_call_changed')
                return super().psql(target)

        contender_sql = 'BEGIN READ ONLY; SELECT pg_try_advisory_xact_lock(' + LOCK + '); ROLLBACK;'
        with patch.object(schema, 'SchemaCommands', RetainedSchemaCommands):
            with schema.held_schema(private_directory(output / 'schema', fresh=True)) as witness:
                held = command.run('schema-contender', command.psql(target), sql=contender_sql, ceiling=128)
                inventory.require(held.strip() == b'f', 'retained_schema_fence_not_held')
                manifest['schema_lease'] = {'retained_database': target, 'contention_blocked': True,
                                            'backend_pid': witness['backend_pid']}
        released = command.run('schema-released', command.psql(target), sql=contender_sql, ceiling=128)
        inventory.require(released.strip() == b't', 'retained_schema_fence_not_released')
        manifest['schema_lease']['release_reacquired'] = True

        honcho = HonchoCommands(private_directory(output / 'honcho', fresh=True))
        # Exact previously selected database container/mounts, with no live API
        # call or setting read needed for an already frozen retained target.
        inspector = inventory.Inventory(honcho.root)
        inspector.commands = honcho
        honcho.container = inspector.container('honcho_postgres')['id']
        inventory.require(honcho.metadata(honcho_target, 'metadata') == honcho_expected,
                          'retained_honcho_database_changed')
        extras = selected_extras(honcho, honcho_target, 'extras')
        manifest['honcho'] = {'database': honcho_target, 'tables': len(honcho_expected['tables']),
                              'sequences': len(extras['sequences'] or {}), 'settings_query_passed': True}

        image = inventory.SERVICES['controller'][2]
        actual = command.run('image', command.docker('image', 'inspect', '--format', '{{.Id}}', image), ceiling=128)
        inventory.require(actual.decode().strip() == image, 'fixture_image_changed')
        prefix = 'ortak-recovery-foundation-' + output.name
        volume = 'ortak_recovery_fixture_' + output.name
        # The real cold stores are Linux named volumes. Docker Desktop host
        # bind mounts do not support listxattr and are intentionally refused by
        # the production reader; they cannot prove complete volume semantics.
        occupied = command.run('existing-volume', command.docker('volume', 'ls', '--filter', 'name=^' + volume + '$',
                               '--format', '{{.Name}}'), ceiling=128)
        inventory.require(not occupied.strip(), 'fixture_volume_already_exists')
        command.run('create-volume', command.docker('volume', 'create', '--label',
                    'org.ortak.recovery_fixture=' + output.name, volume), ceiling=128)
        manifest['fixture_volume_retained'] = volume
        def write_fixture(suffix, script):
            args = volume_args(command, output, prefix + '-' + suffix, image, 1024)
            args[args.index('--mount') + 1] = 'type=volume,source=' + volume + ',target=/capture-source,volume-nocopy'
            args[-2] = script
            command.run(suffix, args, ceiling=128)
        write_fixture('setup', "from pathlib import Path;r=Path('/capture-source');r.chmod(0o700);(r/'empty').mkdir(mode=0o700);(r/'data').write_bytes(b'recovery-fixture-only');(r/'data').chmod(0o600)")
        archive = output / 'fixture.tar'
        command.run('cold-reader', volume_args(command, output, prefix + '-reader', image, 1024),
                    output=archive, ceiling=65536)
        with tarfile.open(archive) as reader:
            inventory.require(set(reader.getnames()) == {'.', 'empty', 'data'}
                and reader.getmember('empty').isdir() and reader.getmember('data').mode == 0o600
                and reader.extractfile('data').read() == b'recovery-fixture-only', 'volume_fixture_archive_mismatch')
        save(output / 'volume.json', {'image': image, 'archive_bytes': archive.stat().st_size,
             'complete_tree': True, 'empty_directory_preserved': True, 'file_mode_preserved': True})
        for suffix in ['limit', 'link']:
            if suffix == 'link':
                write_fixture('make-link', "from pathlib import Path;Path('/capture-source/link').symlink_to('/capture-source/data')")
            try:
                command.run(suffix, volume_args(command, output, prefix + '-' + suffix, image,
                            1 if suffix == 'limit' else 1024), ceiling=65536)
            except Refused:
                manifest['reader_' + suffix + '_refused'] = True
            else:
                raise Refused('volume_fixture_unsafe_acceptance')
        manifest['reader_containers_retained'] = [prefix + '-' + suffix for suffix in ['setup', 'reader', 'limit', 'make-link', 'link']]
        manifest['status'] = 'verified'
    except Exception as error:
        manifest.update(status='failed', error_type=type(error).__name__,
                        error_code=str(error) if isinstance(error, Refused) else 'fixture_failed')
        save(output / 'manifest.json', manifest)
        raise Refused('foundation_fixture_failed_retained') from None
    save(output / 'manifest.json', manifest)
    return output


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--execute-retained-fixtures', action='store_true', required=True)
    parser.parse_args()
    try:
        output = execute()
        print(json.dumps({'status': 'verified', 'manifest': str(output / 'manifest.json'), 'source_mutations': False}))
    except (Refused, OSError, ValueError, KeyError, TypeError):
        raise SystemExit('Recovery foundation fixture refused; private evidence retained. Existing databases unchanged.') from None
