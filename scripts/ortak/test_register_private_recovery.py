"""Selected launcher closure tests; no real processes, services or credentials touched."""

import ast
import hashlib
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import register_private_recovery as subject
from test_prepare_private_recovery import observation


class OwnerTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name).resolve()
        self.root.chmod(0o700)

    def test_rebase_changes_exact_one_literal_and_no_executable_logic(self):
        raw = ('import sys\nsys.path.insert(0,' + repr(str(subject.REPOSITORY_HELPERS)) + ')\n'
               'from private_native_services import environment\nresult = environment(root)\n').encode()
        changed = subject.rebase_source(raw, self.root / 'new')
        self.assertIn(repr(str(self.root / 'new')).encode(), changed)
        self.assertNotIn(str(subject.REPOSITORY_HELPERS).encode(), changed)
        self.assertEqual(ast.dump(ast.parse(raw.replace(str(subject.REPOSITORY_HELPERS).encode(), str(self.root / 'new').encode()))),
                         ast.dump(ast.parse(changed)))
        for bad in [raw + ('other=' + repr(str(subject.REPOSITORY_HELPERS))).encode(), b'import sys\n']:
            with self.assertRaises(subject.Refused): subject.rebase_source(bad, self.root / 'new')

    def test_source_is_bounded_owned_regular_and_frozen_mode500(self):
        path = self.root / 'public.py'
        path.write_text('value = 1\n'); path.chmod(0o644)
        raw = subject.source(path)
        frozen = subject.frozen_source(self.root / 'frozen.py', raw)
        self.assertEqual(frozen['sha256'], hashlib.sha256(raw).hexdigest())
        self.assertEqual((self.root / 'frozen.py').stat().st_mode & 0o777, 0o500)
        with self.assertRaises(FileExistsError): subject.frozen_source(self.root / 'frozen.py', raw)
        path.unlink(); path.symlink_to(self.root / 'frozen.py')
        with self.assertRaises(subject.Refused): subject.source(path)

    def test_normalized_session_receipts_bind_pid_start_binary_launcher_and_import_root(self):
        current = observation()
        selection = tuple(subject.inventory.STATE / ('fixture-' + name) for name in ['launcher', 'receipt', 'helpers'])
        receipts = {**subject.RECEIPTS, 'ortak-management': selection[1]}
        current['native_processes'] = {name: {'pid': n, 'uid': 501, 'started_at': 'Sat Sep 5 22:57:05 2026',
            'executable': str(subject.inventory.BACKEND_ARTIFACTS / name), 'sha256': 'b' * 64}
            for n, name in enumerate(receipts, 100)}
        records = {}
        for name, path in receipts.items():
            pid = current['native_processes'][name]['pid']
            process = current['native_processes'][name]
            value = {'pid': pid, 'session': pid + 1, 'status': 'resumed_verified',
                'binary': process['executable'], 'sha256': process['sha256'],
                'launcher': str(selection[0] if name == 'ortak-management' else subject.LAUNCHERS[name]),
                'helper_import_root': str(selection[2] if name == 'ortak-management' else subject.LAUNCH_HELPERS),
                'identity': str(pid) + ' 501 ' + process['started_at']}
            records[name] = (value, {'path': str(path), 'sha256': 'a' * 64})
        def read(name): return records[name]
        with patch.object(subject.inventory, 'native_launch_record', side_effect=read), \
            patch.object(subject, 'MANAGEMENT_SELECTION', selection):
            result = subject.sessions(current)
            self.assertEqual(result['ortak-server']['session_id'], 102)
            self.assertEqual(result['ortak-management']['session_id'], 104)
            records['ortak-worker'][0]['pid'] = 999
            with self.assertRaises(subject.Refused): subject.sessions(current)

    def test_management_selection_and_exact_four_owner_set_are_required(self):
        with patch.object(subject, 'MANAGEMENT_SELECTION', None), self.assertRaisesRegex(subject.Refused, 'selection_required'):
            subject.selected_process_sources()
        for names in [subject.inventory.NATIVE_WRITERS[:-1], (*subject.inventory.NATIVE_WRITERS, 'unknown-writer')]:
            with self.assertRaisesRegex(subject.Refused, 'inventory_incomplete'): subject.inventory.native_writer_set(names)
        subject.inventory.native_writer_set(subject.inventory.NATIVE_WRITERS)

    def test_seven_binary_receipt_binds_management_and_worker_without_legacy_hash_fallback(self):
        record = {'binaries': {name: {'sha256': 'a' * 64} for name in subject.inventory.NATIVE_WRITERS}}
        for name in subject.inventory.NATIVE_WRITERS:
            self.assertEqual(subject.inventory.native_artifact_hash(record, name), 'a' * 64)
        self.assertEqual(subject.inventory.native_artifact_hash({'binary_sha256': 'b' * 64}, 'ortak-worker'), 'b' * 64)
        with self.assertRaises(subject.Refused): subject.inventory.native_artifact_hash({'binary_sha256': 'b' * 64}, 'ortak-management')
        del record['binaries']['ortak-worker']; record['binary_sha256'] = 'b' * 64
        with self.assertRaises(subject.Refused): subject.inventory.native_artifact_hash(record, 'ortak-worker')

    def test_registration_preserves_failure_and_does_not_bless_changed_preparation(self):
        old = observation()
        changed = observation(); changed['containers']['fixture']['id'] = 'changed'
        with patch.object(subject.inventory, 'STATE', self.root), \
             patch.object(subject, 'load_preparation', return_value={'observation': old}):
            with self.assertRaises(subject.Refused):
                subject.register(self.root / 'unused', observer=lambda output: changed)
        record = json.loads(next((self.root / 'recovery-operations').glob('*/failure.json')).read_text())
        self.assertFalse(record['source_mutations'])
        self.assertFalse(record['service_paused'])

    def test_each_new_launcher_requires_recorded_source_hash_and_exact_helper_root(self):
        raw = b'import os\n'
        for name in subject.inventory.NATIVE_WRITERS:
            record = {'launcher': str(subject.LAUNCHERS[name]), 'launcher_sha256': hashlib.sha256(raw).hexdigest(),
                'helper_import_root': str(subject.LAUNCH_HELPERS)}
            with self.subTest(name=name), patch.object(subject.inventory, 'native_launch_record', return_value=(record, {})):
                subject.approved_launcher(name, raw)
                with self.assertRaises(subject.Refused): subject.approved_launcher(name, raw + b'print(1)\n')
                record['helper_import_root'] = 'different'
                with self.assertRaises(subject.Refused): subject.approved_launcher(name, raw)

    def test_operator_closure_includes_deferred_restore_dependency_with_exact_frozen_bytes(self):
        records = subject.freeze_operator(self.root)
        self.assertEqual(set(records), set(subject.OPERATOR_FILES))
        self.assertIn('private_restore_credential_functions.py', records)
        self.assertIn('private_restore_honcho_checks.py', records)
        for row in records.values():
            target = Path(row['frozen']['path'])
            self.assertEqual(target.stat().st_mode & 0o777, 0o500)
            self.assertEqual(hashlib.sha256(target.read_bytes()).hexdigest(), row['original_sha256'])
            self.assertEqual(target.parent, self.root / 'operator-code')


if __name__ == '__main__':
    unittest.main()
