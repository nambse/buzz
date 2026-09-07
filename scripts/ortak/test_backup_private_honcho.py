"""Falsifiable fixed-scope Honcho snapshot/restore tests; default uses no real database."""

from contextlib import contextmanager
import copy
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import backup_private_honcho as subject
from backup_private_database import private_binary


def catalog(database):
    return {'database': database, 'role': subject.ROLE, 'owners': [subject.ROLE],
            'extensions': {'vector': '0.8.6'}, 'schema_sha256': 'a' * 64,
            'tables': {'public.' + name: 1 for name in
                ['messages', 'ortak_resource_receipts', 'ortak_session_ownership', 'ortak_write_receipts']}}


class FakeCommands:
    calls = []
    failure = None

    def __init__(self, root):
        self.root = root
        self.container = subject.inventory.SERVICES['honcho_postgres'][0]
        self.in_snapshot = False

    def inspect(self):
        self.calls.append(('inspect',))
        return {'container_id': self.container}

    def psql(self, database):
        return ['psql', database]

    def command(self, *args):
        return list(args)

    def run(self, label, args, **kwargs):
        self.calls.append((label, args, kwargs))
        if label == 'database-size': return b'1024'
        if label == 'honcho-check-source':
            assert self.in_snapshot
            return b'{"tables":[],"checks":[]}'
        if label == 'dump':
            assert self.in_snapshot
            with private_binary(kwargs['output']) as stream: stream.write(b'fixture-private-archive')
            if self.failure == 'dump': raise subject.Refused('fixture_dump_failed')
        return b''

    @contextmanager
    def snapshot(self):
        self.in_snapshot = True
        self.calls.append(('snapshot',))
        try: yield '00000001-00000002-1'
        finally:
            self.in_snapshot = False
            self.calls.append(('snapshot_released',))

    def metadata(self, database, label, snapshot=None):
        self.calls.append(('metadata', database, label, snapshot))
        if database == subject.SOURCE: assert self.in_snapshot and snapshot
        result = {'tables': {'native': 1, 'extension': 2}, 'logical_rows_sha256': {'native': 'a', 'extension': 'b'}}
        if self.failure == 'bytes' and label == 'restored': result['logical_rows_sha256']['extension'] = 'changed'
        return result

    def create_target(self, target):
        intent = json.loads((self.root / 'restore-intent.json').read_text())
        assert intent['verification_database'] == target
        subject.verification_name(target)
        assert not self.in_snapshot
        self.calls.append(('create_target', target))
        if self.failure == 'occupied': raise subject.Refused('command_failed')

    def restore(self, target, archive, *, source_checks=None):
        assert source_checks == {'tables':[],'checks':[]}
        self.calls.append(('restore', target, archive))
        if self.failure == 'restore': raise subject.Refused('command_failed')


class HonchoBackupTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name).resolve()
        self.root.chmod(0o700)
        FakeCommands.calls = []
        FakeCommands.failure = None

    def backup(self):
        with patch.object(subject.inventory, 'STATE', self.root), patch.object(subject, 'selected_root', return_value=self.root):
            return subject.backup(self.root, FakeCommands)

    def test_snapshot_feeds_both_native_extension_metadata_and_dump_then_fresh_restore(self):
        output = self.backup()
        result = json.loads((output / 'manifest.json').read_text())
        self.assertEqual(result['status'], 'verified')
        self.assertFalse(result['cross_store_snapshot'])
        self.assertFalse(result['application_endpoint_changed'])
        self.assertFalse(result['source_mutations'])
        self.assertEqual(result['expected'], result['restored'])
        dump = next(row for row in FakeCommands.calls if row[0] == 'dump')
        self.assertIn('--snapshot=' + result['snapshot'], dump[1])
        self.assertIn(subject.SOURCE, dump[1])
        self.assertEqual(next(row for row in FakeCommands.calls if row[0] == 'restore')[1], result['verification_database'])
        self.assertEqual(output.stat().st_mode & 0o777, 0o700)
        self.assertTrue(all(p.stat().st_mode & 0o777 == 0o600 for p in output.iterdir()))
        self.assertNotIn('fixture-private-archive', (output / 'manifest.json').read_text())

    def test_mismatched_receipt_bytes_fail_even_when_all_counts_match(self):
        FakeCommands.failure = 'bytes'
        with self.assertRaises(subject.Refused): self.backup()
        output = next((self.root / 'honcho-backups').iterdir())
        manifest = json.loads((output / 'manifest.json').read_text())
        self.assertEqual(manifest['status'], 'failed')
        self.assertEqual(manifest['different_fields'], ['logical_rows_sha256'])
        self.assertTrue(manifest['verification_created'])
        self.assertTrue((output / 'honcho.dump').exists())

    def test_failed_dump_or_occupied_target_or_partial_restore_retained_without_replay(self):
        for failure in ['dump', 'occupied', 'restore']:
            FakeCommands.failure = failure
            FakeCommands.calls = []
            with self.subTest(failure=failure), self.assertRaises(subject.Refused): self.backup()
            self.assertLessEqual(sum(row[0] == 'restore' for row in FakeCommands.calls), 1)
            if failure != 'restore': self.assertFalse(any(row[0] == 'restore' for row in FakeCommands.calls))
        self.assertEqual(len(list((self.root / 'honcho-backups').glob('*/manifest.json'))), 3)

    def test_restore_rejects_source_existing_unapproved_or_already_consumed_authority(self):
        command = subject.HonchoCommands(self.root)
        command.container = 'fixed'
        target = subject.PREFIX + 'a' * 32
        with patch.object(command, 'run', side_effect=lambda label, *args, **kwargs:
                          b'{"tables":[],"checks":[]}' if label.startswith('honcho-check-') else b'') as run:
            for name in [subject.SOURCE, 'ortak', 'postgres', 'arbitrary', target]:
                with self.subTest(name=name), self.assertRaises(subject.Refused): command.restore(name, self.root / 'unused')
            run.assert_not_called()
            command.create_target(target)
            command.restore(target, self.root / 'new.dump')
            with self.assertRaises(subject.Refused): command.restore(target, self.root / 'new.dump')
            args = next(call.args[1] for call in run.call_args_list if call.args[0]=='restore')
            self.assertIn('--single-transaction', args)
            self.assertIn('--exit-on-error', args)
            self.assertNotIn('--clean', args)
            self.assertNotIn('--create', args)
            self.assertEqual(args[-1], target)

    def test_failed_create_never_grants_restore_authority(self):
        command = subject.HonchoCommands(self.root)
        command.container = 'fixed'
        target = subject.PREFIX + 'a' * 32
        with patch.object(command, 'run', side_effect=subject.Refused('occupied')):
            with self.assertRaises(subject.Refused): command.create_target(target)
        self.assertIsNone(command.restore_authority)

    def test_metadata_imports_exact_snapshot_for_content_and_counts_and_rejects_secret_uris(self):
        command = subject.HonchoCommands(self.root)
        command.container = 'fixed'
        data = catalog(subject.SOURCE)
        hashes = {key: 'b' * 64 for key in data['tables']}
        rows = [b'["public"]\n' + json.dumps(data).encode(), json.dumps(hashes).encode()]
        with patch.object(command, 'run', side_effect=rows) as run:
            result = command.metadata(subject.SOURCE, 'source', '00000001-00000002-1')
        self.assertEqual(result['logical_rows_sha256'], hashes)
        for call in run.call_args_list:
            self.assertIn("SET TRANSACTION SNAPSHOT '00000001-00000002-1'", call.kwargs['sql'])
            self.assertIn('READ ONLY', call.kwargs['sql'])
            self.assertEqual(call.args[1][-1], subject.SOURCE)
            self.assertNotIn('PGPASSWORD', str(call))
        with patch.object(command, 'run') as run:
            for bad in [None, "0';DELETE", 'postgres://secret@host/db']:
                if bad is None: continue
                with self.assertRaises(subject.Refused): command.metadata(subject.SOURCE, 'source', bad)
            run.assert_not_called()

    def test_nonpublic_schema_crossscope_database_or_missing_extension_refuses_before_hash(self):
        command = subject.HonchoCommands(self.root)
        command.container = 'fixed'
        for mutation in ['schema', 'database', 'owner', 'extension', 'rows']:
            data = catalog(subject.SOURCE)
            schemas = ['public']
            if mutation == 'schema': schemas.append('unreviewed')
            elif mutation == 'database': data['database'] = 'other'
            elif mutation == 'owner': data['owners'] = ['other']
            elif mutation == 'extension': data['extensions'] = {}
            else: data['tables']['public.messages'] = subject.MAX_ROWS + 1
            with self.subTest(mutation=mutation), patch.object(command, 'run', return_value=json.dumps(schemas).encode() + b'\n' + json.dumps(data).encode()) as run:
                with self.assertRaises(subject.Refused): command.metadata(subject.SOURCE, 'source', '00000001-00000002-1')
                self.assertEqual(run.call_count, 1)


if __name__ == '__main__':
    unittest.main()
