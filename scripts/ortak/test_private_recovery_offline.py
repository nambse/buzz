"""Falsifiable offline archive, secret-binding, journal and storage-owner regressions."""

import base64
import io
import json
from pathlib import Path
import shutil
import sqlite3
import tarfile
import tempfile
import unittest
from unittest.mock import patch

from cryptography.exceptions import InvalidTag

from backup_private_database import Refused
from prepare_private_recovery import sha
import private_recovery_payloads as payload
import private_recovery_offline_stores as stores
import recovery_archive_io as archive_io
import restore_private_recovery as subject


class OfflineTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name).resolve(); self.root.chmod(0o700)

    def tar(self, members):
        stream = io.BytesIO()
        with tarfile.open(fileobj=stream, mode='w') as archive:
            for name, kind, data in members:
                row = tarfile.TarInfo(name); row.mode = 0o700 if kind == tarfile.DIRTYPE else 0o600
                row.type = kind; row.size = len(data) if kind == tarfile.REGTYPE else 0
                if kind == tarfile.SYMTYPE: row.linkname = '../outside'
                archive.addfile(row, io.BytesIO(data) if kind == tarfile.REGTYPE else None)
        return stream.getvalue()

    def test_preflight_binds_workspace_evidence_for_every_reviewed_schema_only(self):
        evidence={'schema_version':76,'retained':'exact-fixture'}
        manifest={'secrets':{},'components':{
            'databases':{'main':{'receipt':{},'recovery_obligations':{'evidence':evidence}},
                'honcho':{'receipt':{}}},'volumes':{'redis':{},'minio':{}},'journal':{},
            'public_artifacts':{'configuration':{},'native_and_repositories':{}},
            'workspace_files':{'path':'workspace-files','manifest_sha256':'a'*64}}}
        def proof(*args):
            return {'database_evidence_sha256':subject.workspace_capture.digest(
                subject.workspace_capture.canonical(evidence))}
        with patch.object(subject,'artifact',side_effect=lambda bundle,row,name,limit:bundle/name), \
                patch.object(subject,'payload_identity',return_value=('unchanged',)), \
                patch.object(subject.workspace_capture,'bounded_action',side_effect=proof) as child:
            for version in (74,75,76,77,78):
                evidence['schema_version']=version
                subject.preflight(self.root,manifest,True,object())
            for version in (73,79,True,'76'):
                evidence['schema_version']=version
                with self.assertRaisesRegex(Refused,'offline_workspace_database_binding'):
                    subject.preflight(self.root,manifest,True,object())
            evidence['schema_version']=76
            child.side_effect=lambda *args:{'database_evidence_sha256':'0'*64}
            with self.assertRaisesRegex(Refused,'offline_workspace_database_binding'):
                subject.preflight(self.root,manifest,True,object())

    def test_safe_extraction_and_metadata_digest_include_empty_directories_and_all_bytes(self):
        raw = self.tar([('.', tarfile.DIRTYPE, b''), ('empty', tarfile.DIRTYPE, b''), ('data', tarfile.REGTYPE, b'fixture')])
        expected = archive_io.archive(io.BytesIO(raw), 1024)
        target = self.root / 'target'; target.mkdir(mode=0o700)
        actual = archive_io.archive(io.BytesIO(raw), 1024, target)
        self.assertEqual(actual, expected)
        self.assertEqual((target / 'data').read_bytes(), b'fixture')
        self.assertTrue((target / 'empty').is_dir())
        self.assertEqual(actual['directories'], 2)
        self.assertEqual(actual['bytes'], 7)
        changed = self.tar([('.', tarfile.DIRTYPE, b''), ('empty', tarfile.DIRTYPE, b''), ('data', tarfile.REGTYPE, b'different')])
        self.assertNotEqual(archive_io.archive(io.BytesIO(changed), 1024)['tree_sha256'], expected['tree_sha256'])
        with self.assertRaises(ValueError): archive_io.archive(io.BytesIO(raw), 1024, target)

    def test_escape_link_duplicate_special_oversize_and_unapproved_members_are_refused(self):
        for members in [
            [('../escape', tarfile.REGTYPE, b'x')], [('/escape', tarfile.REGTYPE, b'x')],
            [('a//b', tarfile.REGTYPE, b'x')], [('link', tarfile.SYMTYPE, b'')],
            [('dev', tarfile.CHRTYPE, b'')], [('same', tarfile.REGTYPE, b'x'), ('same', tarfile.REGTYPE, b'x')],
        ]:
            with self.subTest(members=members), self.assertRaises(ValueError):
                archive_io.archive(io.BytesIO(self.tar(members)), 1024)
        raw = self.tar([('data', tarfile.REGTYPE, b'12345')])
        with self.assertRaises(ValueError): archive_io.archive(io.BytesIO(raw), 4)
        with self.assertRaises(ValueError): archive_io.archive(io.BytesIO(raw), 10, expected_names={'other'})

    def test_minio_xattr_is_bounded_and_part_of_the_exact_tree_digest(self):
        def archive(value, key=archive_io.PAX_XATTR):
            output = io.BytesIO()
            with tarfile.open(fileobj=output, mode='w') as stream:
                row = tarfile.TarInfo('format.json'); row.size = 2; row.mode = 0o600
                row.pax_headers = {key: value}; stream.addfile(row, io.BytesIO(b'{}'))
            return output.getvalue()
        first = archive_io.archive(io.BytesIO(archive(base64.b64encode(b'12345678').decode())), 1024)
        second = archive_io.archive(io.BytesIO(archive(base64.b64encode(b'12345679').decode())), 1024)
        self.assertNotEqual(first['tree_sha256'], second['tree_sha256'])
        for value, key in [('invalid base64', archive_io.PAX_XATTR),
            *[(base64.b64encode(b'x' * size).decode(), key) for key in archive_io.PAX_XATTRS for size in (0,7,9,257)],
            ('MTIz', 'ORTAK.xattr.security.capability')]:
            with self.subTest(key=key), self.assertRaises(ValueError):
                archive_io.archive(io.BytesIO(archive(value, key)), 1024)
        path = self.root / 'attribute'; path.write_bytes(b'{}'); path.chmod(0o600)
        row = {'mode': 0o600, 'mtime_ns': 1000000000, 'xattrs': {archive_io.XATTR: base64.b64encode(b'12345678').decode()}}
        with patch.object(archive_io.os, 'listxattr', return_value=[], create=True), \
            patch.object(archive_io.os, 'setxattr', create=True) as setting, patch.object(archive_io.os, 'fsync') as sync:
            archive_io.attributes(path, row, False)
            setting.assert_called_once_with(path, 'user.total_writes', b'12345678', follow_symlinks=False)
            sync.assert_called_once()
        with patch.object(archive_io.os, 'listxattr', return_value=['user.unreviewed'], create=True):
            with self.assertRaises(ValueError): archive_io.xattrs(path)
        headers = {header: base64.b64encode(bytes([index]) * 8).decode()
            for index, header in enumerate(archive_io.PAX_XATTRS, 1)}
        attributes = archive_io.archive_xattrs(headers)
        self.assertEqual(set(attributes), {'user.total_writes', 'user.total_deletes'})
        changed = {**headers, 'ORTAK.xattr.user.total_deletes': base64.b64encode(b'3' * 8).decode()}
        self.assertNotEqual(archive_io.summary([{'name': 'format.json', 'type': 'file', 'bytes': 0, 'xattrs': attributes}]),
            archive_io.summary([{'name': 'format.json', 'type': 'file', 'bytes': 0, 'xattrs': archive_io.archive_xattrs(changed)}]))

    def test_fixture_secret_envelope_uses_exact_separate_key_and_never_mounts_oauth(self):
        bundle = self.root / 'recovery-fixture-bundles' / ('a' * 32)
        bundle.mkdir(mode=0o700, parents=True); bundle.parent.chmod(0o700)
        secret = bundle / 'fixture-secrets'; secret.mkdir(mode=0o700)
        for key in ['main-password', 'honcho-password', 'oauth-test']:
            (secret / key).write_bytes(b'fixture-never-valid'); (secret / key).chmod(0o600)
        keys = self.root / 'recovery-fixture-keys'; keys.mkdir(mode=0o700)
        selected = {secret: ['main-password', 'honcho-password', 'oauth-test']}
        metadata = [payload.inventory.file_metadata(secret, key) for key in selected[secret]]
        prepared = {'observation': {'files': {'secret_metadata_only': metadata}}}
        manifest = {'components': {'fixture': 'bound'}}
        aad = {'format': subject.BUNDLE_FORMAT, 'operation_id': bundle.name,
               'components_sha256': sha(manifest['components']), 'secret_metadata_sha256': sha(metadata)}
        with patch.object(payload.inventory, 'SECRET_FILES', selected):
            manifest['secrets'] = payload.secret_envelope(bundle / 'secrets.aesgcm', keys / (bundle.name + '.key'),
                metadata, aad, {kind + '-database-settings.json': b'{}' for kind in ['main', 'honcho']})
        target = self.root / 'restored'; target.mkdir(mode=0o700)
        with patch.object(subject.inventory, 'STATE', self.root):
            _, passwords, report = subject.decrypt(bundle, manifest, prepared, target, True)
            self.assertEqual(set(passwords), {'main', 'honcho'})
            self.assertTrue(all(path.read_bytes() == b'fixture-never-valid' for path in passwords.values()))
            self.assertFalse(report['runtime_mounted'])
            self.assertNotIn('fixture-never-valid', json.dumps(report))
            self.assertEqual((keys / (bundle.name + '.key')).stat().st_mode & 0o777, 0o600)
            manifest['components']['fixture'] = 'changed'
            other = self.root / 'other'; other.mkdir(mode=0o700)
            with self.assertRaises(Refused): subject.decrypt(bundle, manifest, prepared, other, True)
            self.assertEqual(list(other.iterdir()), [])
            manifest['components']['fixture'] = 'bound'
            key = keys / (bundle.name + '.key'); raw = key.read_bytes(); key.write_bytes(b'0' * 32)
            with self.assertRaises(InvalidTag): subject.decrypt(bundle, manifest, prepared, other, True)
            self.assertEqual(list(other.iterdir()), [])
            key.write_bytes(raw)
            manifest['secrets']['key_reference'] = str(bundle / 'key')
            with self.assertRaises(Refused): subject.decrypt(bundle, manifest, prepared, other, True)

    def test_artifact_cannot_follow_link_escape_hash_or_nonprivate_mode(self):
        path = self.root / 'main.dump'; path.write_bytes(b'fixture'); path.chmod(0o600)
        row = {'path': path.name, 'bytes': 7, 'sha256': payload.digest(path)}
        self.assertEqual(subject.artifact(self.root, row, 'main.dump', 1024), path)
        for altered in [{**row, 'path': '../main.dump'}, {**row, 'bytes': 8}, {**row, 'sha256': 'a' * 64}]:
            with self.assertRaises(Refused): subject.artifact(self.root, altered, 'main.dump', 1024)
        path.chmod(0o644)
        with self.assertRaises(Refused): subject.artifact(self.root, row, 'main.dump', 1024)

    def test_journal_binds_real_private_diagnostics_tombstones_and_dense_cursors(self):
        path = self.root / 'journal.sqlite'
        with sqlite3.connect(path) as db:
            db.executescript("CREATE TABLE runs(start_key TEXT PRIMARY KEY,status TEXT,sequence INTEGER);CREATE TABLE events(start_key TEXT REFERENCES runs(start_key),sequence INTEGER);CREATE TABLE private_failure_diagnostics(start_key TEXT PRIMARY KEY REFERENCES runs(start_key),recorded_at TEXT,diagnostic TEXT);CREATE TABLE fixture_tombstones(start_key TEXT PRIMARY KEY);INSERT INTO runs VALUES('fixture','failed',1);INSERT INTO events VALUES('fixture',1);INSERT INTO private_failure_diagnostics VALUES('fixture','today','closed');INSERT INTO fixture_tombstones VALUES('retired');")
        path.chmod(0o600)
        result = subject.journal(path)
        self.assertEqual(result['private_failure_diagnostics'], 1)
        self.assertEqual(result['tables']['fixture_tombstones'], 1)
        with sqlite3.connect(path) as db: db.execute("UPDATE private_failure_diagnostics SET diagnostic='changed'")
        self.assertNotEqual(subject.journal(path)['logical_rows_sha256'], result['logical_rows_sha256'])
        with sqlite3.connect(path) as db: db.execute("UPDATE private_failure_diagnostics SET diagnostic=?", ('x' * 2049,))
        with self.assertRaises(Refused): subject.journal(path)

    def test_wal_mode_backup_without_any_sidecar_is_inspected_without_changing_artifact(self):
        original = self.root / 'writer.sqlite'
        writer = sqlite3.connect(original)
        self.addCleanup(writer.close)
        writer.execute('PRAGMA journal_mode=WAL')
        writer.executescript("CREATE TABLE runs(start_key TEXT PRIMARY KEY,status TEXT,sequence INTEGER);CREATE TABLE events(start_key TEXT REFERENCES runs(start_key),sequence INTEGER);CREATE TABLE private_failure_diagnostics(start_key TEXT PRIMARY KEY REFERENCES runs(start_key),recorded_at TEXT,diagnostic TEXT);INSERT INTO runs VALUES('fixture','failed',1);INSERT INTO events VALUES('fixture',1);INSERT INTO private_failure_diagnostics VALUES('fixture','fixture-time','closed');")
        writer.commit(); original.chmod(0o600)
        backup = self.root / 'coherent.sqlite'
        payload.sqlite_backup(original, backup)
        checkpoint = sqlite3.connect(backup)
        try: checkpoint.execute('PRAGMA wal_checkpoint(TRUNCATE)')
        finally: checkpoint.close()
        cold = self.root / 'cold.sqlite'
        shutil.copyfile(backup, cold); cold.chmod(0o600)
        self.assertFalse(Path(str(cold) + '-wal').exists())
        self.assertFalse(Path(str(cold) + '-shm').exists())
        before = cold.read_bytes()
        result = subject.journal(cold)
        self.assertEqual(result['private_failure_diagnostics'], 1)
        self.assertEqual(cold.read_bytes(), before)
        self.assertFalse(Path(str(cold) + '-wal').exists())
        self.assertFalse(Path(str(cold) + '-shm').exists())

    def test_database_creation_uses_supported_flags_and_refuses_limits_before_mutation(self):
        store = stores.Postgres(self.root, 'a' * 32, 'main', 'sha256:' + 'b' * 64, self.root / 'password')
        store.container = 'c' * 64
        settings = {'database': {'owner': 'ortak', 'tablespace': 'pg_default', 'locale_provider': 'c',
                                'encoding': 'UTF8', 'collation': 'C', 'ctype': 'C', 'connection_limit': -1}}
        with patch.object(store, 'inspect', return_value={}), patch.object(store, 'run') as execute:
            store.create_database(settings)
            self.assertTrue(store.restore_allowed)
            self.assertFalse(any(arg.startswith('--connection-limit') for arg in execute.call_args.args[1]))
            execute.reset_mock(); settings['database']['connection_limit'] = 10001
            with self.assertRaises(Refused): store.create_database(settings)
            execute.assert_not_called()

    def test_existing_volume_cannot_gain_authority_or_create_container(self):
        class Occupied:
            root = self.root
            def docker(self, *args): return args
            def run(self, *args, **kwargs): return b'existing\n'
        with patch.object(stores, 'save') as record:
            with self.assertRaises(Refused): stores.fresh_volume(Occupied(), 'a' * 32, 'main')
            record.assert_not_called()

    def test_pg_restore_requires_once_created_destination_and_exact_owner(self):
        password = self.root / 'password'; password.write_bytes(b'fixture'); password.chmod(0o600)
        store = stores.Postgres(self.root, 'a' * 32, 'main', 'sha256:' + 'b' * 64, password)
        with patch.object(store, 'inspect', return_value={}), patch.object(store, 'run') as execute:
            with self.assertRaises(Refused): store.restore(self.root / 'database.dump')
            execute.assert_not_called()
            store.container = 'c' * 64; store.restore_allowed = True
            with patch('private_restore_credential_functions.restore_sections') as restore:
                store.restore(self.root / 'database.dump')
                restore.assert_called_once_with(store, 'ortak', self.root / 'database.dump')
            self.assertFalse(store.restore_allowed)
            with self.assertRaises(Refused): store.restore(self.root / 'database.dump')
        with self.assertRaises(Refused): store.psql('unselected')

    def test_stop_retains_only_exact_new_owner_and_uses_current_nonwarning_cli_flag(self):
        store = stores.Postgres(self.root, 'a' * 32, 'main', 'sha256:' + 'b' * 64, self.root / 'password')
        store.container = 'c' * 64
        with patch.object(store, 'inspect', side_effect=[{}, {'running': False}]) as inspect, \
                patch.object(store, 'run', return_value=(store.container + '\n').encode()) as execute, \
                patch.object(stores, 'save'):
            self.assertFalse(store.stop_retained()['running'])
            self.assertEqual(execute.call_args.args[1][-4:], ['stop', '--timeout', '30', store.container])
            self.assertEqual(inspect.call_args.kwargs, {'running': False})
        with patch.object(store, 'inspect', side_effect=Refused('offline_owner_changed')), patch.object(store, 'run') as execute:
            with self.assertRaises(Refused): store.stop_retained()
            execute.assert_not_called()


if __name__ == '__main__': unittest.main()
