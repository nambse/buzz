#!/usr/bin/env python3
"""Build an explicitly synthetic bundle around retained verified dumps and rehearse offline storage."""

import argparse
import json
import os
from pathlib import Path
import secrets
import sqlite3
from unittest.mock import patch
from uuid import uuid4

from backup_private_database import Commands, Refused, digest, private_directory, verification_name
from backup_private_honcho import HonchoCommands, verification_name as honcho_verification_name
from capture_private_recovery import FORMAT as BUNDLE_FORMAT
from prepare_private_recovery import save, sha
from private_recovery_database_metadata import selected_content, selected_extras
import private_recovery_inventory as inventory
import private_recovery_payloads as payload
import restore_private_recovery
from test_private_recovery_foundation import MAIN_RECEIPT, HONCHO_RECEIPT, retained_target


def fixture_bundle():
    """Read only exact retained verify databases; OAuth and all secret/volume/native bytes are synthetic."""
    inventory.directory(inventory.STATE)
    bundle = private_directory(private_directory(inventory.STATE / 'recovery-fixture-bundles') / uuid4().hex, fresh=True)
    manifest = {'format': BUNDLE_FORMAT, 'operation_id': bundle.name, 'status': 'started', 'fixture_only': True,
        'automatic_activation': False, 'full_restore_executed': False, 'source_mutations': False,
        'source_credentials_read': False, 'production_volumes_mounted': False, 'components': {}}
    save(bundle / 'intent.json', manifest)
    try:
        settings = {}
        databases = {}
        for kind, path, archive_name, validate in [
            ('main', MAIN_RECEIPT, 'database.dump', verification_name),
            ('honcho', HONCHO_RECEIPT, 'honcho.dump', honcho_verification_name),
        ]:
            target, expected = retained_target(path, archive_name, validate)
            command = (Commands if kind == 'main' else HonchoCommands)(private_directory(bundle / (kind + '-source-check'), fresh=True))
            if kind == 'main':
                command.inspect()
            else:
                inspector = inventory.Inventory(command.root); inspector.commands = command
                command.container = inspector.container('honcho_postgres')['id']
            inventory.require(command.metadata(target, 'retained') == expected, 'fixture_retained_database_changed')
            settings[kind] = selected_extras(command, target, 'settings')
            content = selected_content(command, target, 'content', expected['tables'])
            archive = payload.copy_file(path.parent / archive_name, bundle / (kind + '.dump'), 256 * 1024**2)
            receipt = payload.copy_file(path, bundle / (kind + '-database.json'), 1024**2)
            databases[kind] = {**archive, 'receipt': receipt, 'logical_rows_sha256': content}
        manifest['components']['databases'] = databases
        seed = private_directory(bundle / 'fixture-secrets', fresh=True)
        for name in ['main-password', 'honcho-password']:
            with payload.private_binary(seed / name) as stream: stream.write(secrets.token_hex(24).encode())
        with payload.private_binary(seed / 'oauth-test') as stream:
            stream.write(b'{"access_token":"fixture-never-valid","refresh_token":"fixture-never-valid"}')
        names = ['main-password', 'honcho-password', 'oauth-test']
        metadata = [inventory.file_metadata(seed, name) for name in names]
        images = sorted({inventory.SERVICES[name][2] for name in ['postgres', 'honcho_postgres', 'controller']})
        prepared = {'observation': {'containers': {name: {'image': inventory.SERVICES[name][2]}
                         for name in ['postgres', 'honcho_postgres', 'controller']},
                         'files': {'secret_metadata_only': metadata}}, 'plan': {'images': images}, 'fixture_only': True}
        configuration = bundle / 'fixture-preparation.json'; save(configuration, prepared)
        config = payload.archive_files(bundle / 'configuration.tar', [(configuration, 'operation/preparation.json')], 1024**2)
        native = private_directory(bundle / 'fixture-native', fresh=True)
        with payload.private_binary(native / 'never-execute.py') as stream: stream.write(b'raise SystemExit("offline fixture only")\n')
        (native / 'never-execute.py').chmod(0o500)
        private_directory(native / 'empty-repository', fresh=True)
        native_archive = payload.archive_files(bundle / 'native-and-repositories.tar',
            payload.tree_entries(native, 'native-fixture', 1024**2), 1024**2)
        manifest['components']['public_artifacts'] = {'configuration': config, 'native_and_repositories': native_archive}
        volumes = {}
        for kind in ['redis', 'minio']:
            tree = private_directory(bundle / ('fixture-' + kind), fresh=True)
            private_directory(tree / ('appendonlydir' if kind == 'redis' else '.minio.sys'), fresh=True)
            private_directory(tree / 'empty', fresh=True)
            with payload.private_binary(tree / ('appendonlydir/fixture.aof' if kind == 'redis' else '.minio.sys/fixture.meta')) as stream:
                stream.write(b'owned offline fixture\0\xff')
            entries = payload.tree_entries(tree, 'volume', 1024**2)
            entries = [(path, '.' if name == 'volume' else name[len('volume/'):]) for path, name in entries]
            # The production tar builder intentionally requires ordinary names;
            # create this synthetic volume root with the standard library only.
            import tarfile
            with payload.private_binary(bundle / (kind + '.tar')) as outgoing, tarfile.open(fileobj=outgoing, mode='w') as archive:
                for path, name in entries: archive.add(path, arcname=name, recursive=False)
            archive = bundle / (kind + '.tar')
            volumes[kind] = {'path': archive.name, 'bytes': archive.stat().st_size, 'sha256': digest(archive)}
        manifest['components']['volumes'] = volumes
        writer_path = bundle / 'fixture-writer.sqlite'
        writer = sqlite3.connect(writer_path)
        try:
            writer.execute('PRAGMA journal_mode=WAL')
            writer.executescript("CREATE TABLE runs(start_key TEXT PRIMARY KEY,status TEXT,sequence INTEGER);CREATE TABLE events(start_key TEXT REFERENCES runs(start_key),sequence INTEGER);CREATE TABLE private_failure_diagnostics(start_key TEXT PRIMARY KEY REFERENCES runs(start_key),recorded_at TEXT,diagnostic TEXT CHECK(length(diagnostic)<=2048));CREATE TABLE fixture_tombstones(start_key TEXT PRIMARY KEY);INSERT INTO runs VALUES('offline-fixture','failed',1);INSERT INTO events VALUES('offline-fixture',1);INSERT INTO private_failure_diagnostics VALUES('offline-fixture','fixture-time','{\"stage\":\"response_normalized\",\"kind\":\"type\",\"boundary\":null,\"frames\":[]}');INSERT INTO fixture_tombstones VALUES('retired-fixture');")
            writer.commit(); writer_path.chmod(0o600)
            manifest['components']['journal'] = payload.sqlite_backup(writer_path, bundle / 'journal.sqlite')
        finally:
            writer.close()
        manifest['components']['images'] = {'images': images, 'fixture_existing_images_only': True}
        aad = {'format': BUNDLE_FORMAT, 'operation_id': bundle.name,
               'components_sha256': sha(manifest['components']), 'secret_metadata_sha256': sha(metadata)}
        keys = private_directory(inventory.STATE / 'recovery-fixture-keys')
        with patch.object(inventory, 'SECRET_FILES', {seed: names}):
            manifest['secrets'] = payload.secret_envelope(bundle / 'secrets.aesgcm', keys / (bundle.name + '.key'),
                metadata, aad, {kind + '-database-settings.json': json.dumps(value, sort_keys=True).encode()
                                for kind, value in settings.items()})
        manifest.update(status='captured', fixture_volume_semantics='synthetic bytes only', real_retained_database_archives=True)
        manifest['manifest_sha256'] = sha(manifest)
        save(bundle / 'manifest.json', manifest)
    except Exception as error:
        manifest.update(status='failed', error_type=type(error).__name__)
        save(bundle / 'manifest.json', manifest)
        raise Refused('offline_fixture_bundle_failed_retained') from None
    return bundle


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--execute-owned-fixture', action='store_true', required=True)
    parser.parse_args()
    try:
        bundle = fixture_bundle()
        output = restore_private_recovery.restore(bundle / 'manifest.json', fixture=True)
        print(json.dumps({'status': 'offline_foundation_verified', 'fixture_bundle': str(bundle / 'manifest.json'),
                          'manifest': str(output / 'manifest.json'), 'runtime_activation': False, 'source_credentials_read': False}))
    except Exception:
        raise SystemExit('Offline fixture rehearsal refused; fresh artifacts/volumes/databases retained. No source replaced or activated.') from None
