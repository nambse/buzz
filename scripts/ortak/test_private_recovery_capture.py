"""Capture state-machine and real payload regressions using private disposable fixtures only."""

from contextlib import contextmanager
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
from cryptography.hazmat.primitives.ciphers.aead import AESGCM

import capture_private_recovery as subject
import private_recovery_payloads as payload
import private_recovery_database_metadata as metadata
import prepare_private_recovery as preparation
import restore_private_recovery as restore
from prepare_private_recovery import canonical, sha
from private_recovery_schema_lease import SchemaCommands


class PayloadTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name).resolve()
        self.root.chmod(0o700)

    def write(self, name, data, mode=0o600):
        path = self.root / name
        path.parent.mkdir(parents=True, exist_ok=True, mode=0o700)
        for parent in path.parents:
            if parent == self.root: break
            parent.chmod(0o700)
        path.write_bytes(data); path.chmod(mode)
        return path

    def test_failure_detail_reports_only_typed_codes_and_scoped_child_receipt(self):
        path=self.write('honcho-backups/20260905T224142Z_'+'a'*32+'/manifest.json',json.dumps({
            'status':'failed','error_code':'honcho_restored_metadata_mismatch','different_fields':['schema_sha256']}).encode())
        error=subject.main_backup.Refused('honcho_backup_failed_private_evidence_retained',receipt_path=path)
        with patch.object(subject.inventory,'STATE',self.root):
            result=subject.failure_detail(error)
        self.assertEqual(result['cause_code'],'honcho_backup_failed_private_evidence_retained')
        self.assertEqual(result['child_cause_code'],'honcho_restored_metadata_mismatch')
        self.assertEqual(result['different_fields'],['schema_sha256'])
        self.assertEqual(result['child_receipt']['path'],str(path))
        for error in [RuntimeError('fixture-secret-value'),subject.main_backup.Refused('fixture-secret-value')]:
            self.assertEqual(subject.failure_detail(error),{'cause_code':'unclassified_capture_failure'})
        self.assertEqual(subject.failure_detail(FileExistsError('fixture-secret-path')),
            {'cause_code':'capture_output_already_exists'})
        self.assertTrue(subject.failure_detail(subject.main_backup.Refused('backup_failed_private_manifest_retained',
            receipt_path=Path('/unrelated/secret')))['child_receipt_refused'])

    def test_file_archive_preserves_empty_directories_and_refuses_links_or_escape(self):
        tree = self.root / 'tree'; tree.mkdir(mode=0o700)
        (tree / 'empty').mkdir(mode=0o700)
        self.write('tree/data', b'private-fixture')
        entries = payload.tree_entries(tree, 'tree', 1024)
        target = self.root / 'tree.tar'
        payload.archive_files(target, entries, 1024)
        with tarfile.open(target) as archive:
            self.assertEqual(set(archive.getnames()), {'tree', 'tree/empty', 'tree/data'})
            self.assertEqual(archive.extractfile('tree/data').read(), b'private-fixture')
        for name in ['../outside', '/outside', 'x\\outside']:
            with self.assertRaises(payload.Refused): payload.safe_name(name)
        (tree / 'link').symlink_to(tree / 'data')
        with self.assertRaises(payload.Refused): payload.tree_entries(tree, 'tree', 1024)

    def test_volume_reader_failure_keeps_only_fixed_codes_and_no_path_or_value(self):
        path = self.write('reader.stderr', b'ORTAK_VOLUME_READER:unreviewed_xattr:xattr_names\n')
        expected = {'kind': 'minio', 'code': 'unreviewed_xattr', 'phase': 'xattr_names'}
        self.assertEqual(payload.volume_reader_failure(path, 'minio'), expected)
        error = subject.main_backup.Refused('cold_volume_reader_failed'); error.reader_failure = expected
        self.assertEqual(subject.failure_detail(error), {'cause_code': 'cold_volume_reader_failed', 'reader_failure': expected})
        for raw in (b'fixture-secret-path', b'ORTAK_VOLUME_READER:fixture-secret:xattr_names',
            b'ORTAK_VOLUME_READER:unreviewed_xattr:fixture-secret', b'x' * 257):
            path.write_bytes(raw)
            self.assertIsNone(payload.volume_reader_failure(path, 'minio'))
        error.reader_failure = {**expected, 'path': 'fixture-secret'}
        self.assertEqual(subject.failure_detail(error), {'cause_code': 'cold_volume_reader_failed'})

    def test_database_receipt_bytes_cannot_pass_with_equal_counts_but_changed_content(self):
        class Queries:
            def __init__(self, rows): self.rows, self.calls = iter(rows), []
            def psql(self, database): return database
            def run(self, label, database, *, sql, ceiling):
                self.calls.append((database, sql, ceiling))
                return json.dumps(next(self.rows)).encode()
        receipt = {'expected': {'tables': {'public.retained_receipt': 1}},
                   'restored': {'tables': {'public.retained_receipt': 1}}}
        original = {'public.retained_receipt': 'a' * 64}
        changed = {'public.retained_receipt': 'b' * 64}
        command = Queries([original, changed])
        with self.assertRaisesRegex(payload.Refused, 'database_logical_rows_restore_mismatch'):
            metadata.verified_content(command, 'ortak', 'ortak_verify_fixture', receipt)
        self.assertEqual([row[0] for row in command.calls], ['ortak', 'ortak_verify_fixture'])
        self.assertTrue(all('READ ONLY' in row[1] and 'to_jsonb(t)' in row[1] for row in command.calls))
        self.assertEqual(metadata.verified_content(Queries([original, original]), 'ortak', 'ortak_verify_fixture', receipt), original)
        command = Queries([])
        with self.assertRaises(payload.Refused):
            metadata.selected_content(command, 'ortak', 'oversize', {'public.receipt': 200001})
        self.assertEqual(command.calls, [])

    def test_file_bound_and_occupied_destination_are_closed(self):
        path = self.write('data', b'12345')
        with self.assertRaises(payload.Refused): payload.copy_file(path, self.root / 'copy', 4)
        payload.copy_file(path, self.root / 'copy', 5)
        with self.assertRaises(FileExistsError): payload.copy_file(path, self.root / 'copy', 5)
        with self.assertRaises(payload.Refused): payload.archive_files(self.root / 'too-small.tar', [(path, 'data')], 4)

    def test_sqlite_backup_captures_committed_wal_and_new_diagnostics_table(self):
        path = self.root / 'journal.sqlite'
        writer = sqlite3.connect(path)
        self.addCleanup(writer.close)
        writer.execute('PRAGMA journal_mode=WAL')
        writer.executescript("CREATE TABLE runs (id TEXT);CREATE TABLE failure_diagnostics(run_id TEXT, diagnostic TEXT);INSERT INTO runs VALUES('fixture');INSERT INTO failure_diagnostics VALUES('fixture','closed_provider_error');")
        writer.commit(); path.chmod(0o600)
        self.assertTrue(Path(str(path) + '-wal').exists())
        target = self.root / 'restored.sqlite'
        result = payload.sqlite_backup(path, target)
        with sqlite3.connect(target) as reader:
            self.assertEqual(reader.execute('SELECT * FROM failure_diagnostics').fetchall(), [('fixture', 'closed_provider_error')])
            self.assertEqual(reader.execute('SELECT * FROM runs').fetchall(), [('fixture',)])
        self.assertEqual(result['integrity'], 'ok')
        self.assertEqual(target.stat().st_mode & 0o777, 0o600)

    def test_sqlite_backup_cold_wal_without_shm_allows_working_metadata_and_preserves_rows(self):
        source = self.root / 'writer.sqlite'
        writer = sqlite3.connect(source)
        self.addCleanup(writer.close)
        writer.execute('PRAGMA journal_mode=WAL')
        writer.executescript("CREATE TABLE failure_diagnostics(run_id TEXT,diagnostic TEXT);INSERT INTO failure_diagnostics VALUES('fixture','closed_provider_error');")
        writer.commit()
        cold = self.root / 'cold'; cold.mkdir(mode=0o700)
        path = cold / 'journal.sqlite'
        for old, new in [(source, path), (Path(str(source) + '-wal'), Path(str(path) + '-wal'))]:
            shutil.copyfile(old, new); new.chmod(0o600)
        before = {p.name: p.read_bytes() for p in cold.iterdir()}
        self.assertFalse(Path(str(path) + '-shm').exists())
        target = self.root / 'backup.sqlite'
        payload.sqlite_backup(path, target, cold=True)
        with sqlite3.connect(target) as reader:
            self.assertEqual(reader.execute('SELECT * FROM failure_diagnostics').fetchall(), [('fixture','closed_provider_error')])
        for name, data in before.items(): self.assertEqual((cold / name).read_bytes(), data)
        self.assertFalse(Path(str(path) + '-shm').exists())

    def test_secret_envelope_authenticates_all_components_and_writes_no_plaintext_archive(self):
        secret = self.write('selected/secret', b'fixture-access-and-refresh', 0o444)
        selected = secret.parent
        bundle = self.root / 'bundle'; bundle.mkdir(mode=0o700)
        keys = self.root / 'keys'; keys.mkdir(mode=0o700)
        record = payload.inventory.file_metadata(selected, 'secret', service_readable=True)
        aad = {'operation_id': 'fixture', 'components_sha256': sha({'database': 'frozen'})}
        with patch.object(payload.inventory, 'SECRET_FILES', {selected: ['secret']}):
            result = payload.secret_envelope(bundle / 'secrets.aesgcm', keys / 'key', [record], aad,
                                             {'database-settings.json': b'{"setting":"fixture"}'})
        raw = (bundle / 'secrets.aesgcm').read_bytes()
        self.assertNotIn(secret.read_bytes(), raw)
        self.assertEqual(list(bundle.iterdir()), [bundle / 'secrets.aesgcm'])
        self.assertFalse(result['offline_restore_executed'])
        plaintext = AESGCM((keys / 'key').read_bytes()).decrypt(raw[8:20], raw[20:], canonical(aad))
        with tarfile.open(fileobj=io.BytesIO(plaintext)) as archive:
            name = next(name for name in archive.getnames() if name.startswith('selected/'))
            self.assertEqual(archive.extractfile(name).read(), secret.read_bytes())
        for data, associated in [(raw[20:-1] + bytes([raw[-1] ^ 1]), aad), (raw[20:], {'operation_id': 'other'})]:
            with self.assertRaises(InvalidTag): AESGCM((keys / 'key').read_bytes()).decrypt(raw[8:20], data, canonical(associated))

    def test_secret_scope_generation_and_key_inside_bundle_refused(self):
        selected = self.root / 'selected'; selected.mkdir(mode=0o700)
        self.write('selected/secret', b'fixture')
        record = payload.inventory.file_metadata(selected, 'secret', service_readable=True)
        bundle = self.root / 'bundle'; bundle.mkdir(mode=0o700)
        keys = self.root / 'keys'; keys.mkdir(mode=0o700)
        nested = bundle / 'keys'; nested.mkdir(mode=0o700)
        with patch.object(payload.inventory, 'SECRET_FILES', {selected: ['secret']}):
            with self.assertRaises(payload.Refused): payload.secret_envelope(bundle / 'bad', nested / 'key', [record], {})
            self.write('selected/secret', b'new-generation')
            with self.assertRaises(payload.Refused): payload.secret_envelope(bundle / 'changed', keys / 'key', [record], {})
        self.assertFalse((keys / 'key').exists())

    def test_schema_fence_idle_timeout_spans_whole_capture_without_disabling_bounds(self):
        command = SchemaCommands(self.root); command.container = 'selected'
        args = command.psql('ortak')
        options = next(value for value in args if value.startswith('PGOPTIONS='))
        self.assertIn('idle_in_transaction_session_timeout=900000', options)
        self.assertIn('statement_timeout=60000', options)
        self.assertIn('lock_timeout=2000', options)
        self.assertIn('--no-password', args)
        self.assertEqual(args[-1], 'ortak')


class StateMachineTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(); self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name).resolve(); self.root.chmod(0o700)
        self.calls = []; self.held = False; self.failure_stage = None

    def invoke(self, workspace=False):
        test = self
        class Backend:
            def __init__(self, output, registry): self.output = output; self.observation = {'workspace_selection':{'fixture':True}} if workspace else {}
            def step(self, name):
                test.assertTrue(test.held)
                test.calls.append(name)
                if test.failure_stage == 'database_file_collision' and name == 'databases':
                    raise FileExistsError('fixture-private-path-not-logged')
                if test.failure_stage == name: raise RuntimeError('fixture-private-value-not-logged')
                return {'fixture': name}
            def cold_stores(self): self.step('cold_stores')
            def databases(self):
                self.step('databases')
                return {'main': {'recovery_obligations': {'evidence': {'fixture': test.failure_stage != 'obligation_generation'}}}}
            def volumes(self): return self.step('volumes')
            def journal(self): return self.step('journal')
            def workspace_files(self,witness):
                test.assertTrue(witness['fixture_lease']); return self.step('workspace_files')
            def public_artifacts(self): return self.step('public_artifacts')
            def images(self): return self.step('images')
            def secrets(self, components):
                test.assertEqual(list(components), ['databases', 'volumes', 'journal'] +
                    (['workspace_files'] if workspace else []) + ['public_artifacts', 'images'])
                return self.step('secrets')
            def current(self): self.step('current')
        @contextmanager
        def barrier(output, registry, **kwargs):
            if test.failure_stage == 'admission': raise subject.main_backup.Refused('live_writer')
            test.held = True
            try: yield {'fixture_lease': True, 'databases': {'recovery_obligations': {'fixture': True}}}
            finally: test.held = False
            if test.failure_stage == 'release': raise subject.main_backup.Refused('lease_lost')
        with patch.object(subject.inventory, 'STATE', self.root), \
             patch.object(subject, 'load_registry', return_value={'registry_sha256': 'a' * 64}), \
             patch.object(subject, 'root_pause_receipt', return_value={}), \
             patch.object(subject.shutil, 'disk_usage', return_value=type('Usage', (), {'free': 100 * 1024**3})()):
            return subject.capture(self.root / 'owners', self.root / 'pause', backend_type=Backend, barrier=barrier)

    def test_selected_workspace_phase_runs_inside_held_barrier_and_failure_cannot_seal(self):
        output=self.invoke(workspace=True)
        value=json.loads((output/'manifest.json').read_text())
        self.assertEqual(list(value['components']),['databases','volumes','journal','workspace_files','public_artifacts','images'])
        self.assertLess(self.calls.index('journal'),self.calls.index('workspace_files'))
        self.assertLess(self.calls.index('workspace_files'),self.calls.index('secrets'))
        self.failure_stage='workspace_files'
        with self.assertRaises(subject.main_backup.Refused):self.invoke(workspace=True)
        failed=[json.loads(p.read_text()) for p in self.root.glob('recovery-bundles/*/manifest.json')
                if json.loads(p.read_text())['status']=='failed']
        self.assertEqual(len(failed),1)
        self.assertEqual(failed[0]['failed_phase'],'workspace_files')

    def test_every_component_and_secret_read_remains_inside_barrier_then_seal(self):
        output = self.invoke()
        manifest = json.loads((output / 'manifest.json').read_text())
        self.assertEqual(manifest['status'], 'captured')
        self.assertFalse(manifest['full_restore_executed'])
        self.assertTrue(manifest['source_resume_required_from_root'])
        self.assertFalse(manifest['source_service_actions'])
        self.assertFalse(self.held)
        self.assertEqual(self.calls, ['cold_stores', 'databases', 'volumes', 'journal', 'public_artifacts', 'images', 'secrets', 'current'])

    def test_live_owner_prevents_all_component_reads(self):
        self.failure_stage = 'admission'
        with self.assertRaises(subject.main_backup.Refused): self.invoke()
        self.assertEqual(self.calls, [])

    def test_exclusive_output_collision_retains_typed_cause_and_component_phase(self):
        self.failure_stage = 'database_file_collision'
        with self.assertRaises(subject.main_backup.Refused): self.invoke()
        path = next((self.root / 'recovery-bundles').glob('*/manifest.json'))
        raw = path.read_text(); manifest = json.loads(raw)
        self.assertEqual(manifest['failed_phase'], 'databases')
        self.assertEqual(manifest['cause_code'], 'capture_output_already_exists')
        self.assertNotIn('fixture-private-path', raw)
        self.assertEqual(manifest['components'], {})
        self.assertEqual(self.calls, ['cold_stores', 'databases'])
        self.assertFalse(self.held)

    def test_seal_fsync_failure_preserves_manifest_and_blocks_offline_restore(self):
        original_save = subject.save
        for failure_at in ('file', 'directory'):
            def failing_save(path, value):
                if path.name == 'manifest.json' and value.get('status') == 'captured':
                    effects = [OSError('fixture-private-fsync-error')] if failure_at == 'file' else [None, OSError('fixture-private-fsync-error')]
                    with patch.object(preparation.os, 'fsync', side_effect=effects):
                        original_save(path, value)
                else:
                    original_save(path, value)
            with self.subTest(failure_at=failure_at), patch.object(subject, 'save', side_effect=failing_save):
                with self.assertRaises(subject.main_backup.Refused): self.invoke()
        for path in (self.root / 'recovery-bundles').glob('*/manifest.json'):
            # Original seal bytes remain retained, including the valid hash.
            value = json.loads(path.read_text()); expected = value.pop('manifest_sha256')
            self.assertEqual(sha(value), expected)
            self.assertEqual(value['status'], 'captured')
            failure = json.loads((path.parent / 'failure.json').read_text())
            self.assertEqual(failure['status'], 'failed')
            self.assertEqual(failure['failed_phase'], 'seal')
            self.assertNotIn('manifest_sha256', failure)
            self.assertNotIn('fixture-private-fsync-error', json.dumps(failure))
            with patch.object(restore.inventory, 'STATE', self.root):
                with self.assertRaisesRegex(subject.main_backup.Refused, 'offline_capture_failed'):
                    restore.load_bundle(path)

    def test_partial_payload_or_release_failure_never_seals_or_loses_retry_evidence(self):
        for phase in ['databases', 'obligation_generation', 'volumes', 'journal', 'public_artifacts', 'images', 'secrets', 'release']:
            self.failure_stage = phase; self.calls = []
            with self.subTest(phase=phase), self.assertRaises(subject.main_backup.Refused): self.invoke()
        records = list((self.root / 'recovery-bundles').glob('*/manifest.json'))
        self.assertEqual(len(records), 8)
        for path in records:
            raw = path.read_text(); manifest = json.loads(raw)
            self.assertEqual(manifest['status'], 'failed')
            self.assertNotIn('manifest_sha256', manifest)
            self.assertNotIn('fixture-private-value-not-logged', raw)
            self.assertTrue(manifest['source_resume_required_from_root'])


if __name__ == '__main__':
    unittest.main()
