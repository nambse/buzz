"""Native client identity, exact artifact scope and fresh-ingress absence tests; no real app access."""

import hashlib
import os
from pathlib import Path
import plistlib
import tempfile
import unittest
from unittest.mock import patch

from backup_private_database import Refused
import recovery_native_ingress as subject


class NativeTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(); self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name).resolve(); self.root.chmod(0o700)
        for name, kind in subject.ENTRIES.items():
            path = self.root / name
            if kind == 'directory': path.mkdir(exist_ok=True); path.chmod(0o700)
            else:
                path.write_bytes(plistlib.dumps({'CFBundleIdentifier': 'dev.ortak.private20260905'})
                    if name.endswith('Info.plist') else b'fixture')
                path.chmod(0o700 if name.endswith('buzz-desktop') else 0o600 if name.endswith('Info.plist') else 0o644)
        self.digest = hashlib.sha256(b'fixture').hexdigest()
        for name, value in [('BUNDLE', self.root), ('BINARY', self.root / 'Contents/MacOS/buzz-desktop'), ('EXPECTED_SHA', self.digest)]:
            context = patch.object(subject, name, value); context.start(); self.addCleanup(context.stop)
        self.build_receipt = {'native_sha256': self.digest, 'status': 'built_policy_verified'}
        self.resume_receipt = {**{name:{} for name in subject.inventory.NATIVE_WRITERS},
            'native': {'pid': subject.SELECTED_PID,'session_id':subject.SELECTED_SESSION, 'sha256': self.digest, 'executable': str(subject.BINARY),
            'cwd': str(subject.inventory.STATE), 'inode': subject.BINARY.stat().st_ino,
            'launcher': str(subject.LAUNCHER),
            'launcher_sha256': subject.LAUNCHER_SHA,
            'identity': f'{subject.SELECTED_PID} {os.getuid()} {subject.SELECTED_STARTED}'}}
        self.resume_metadata = {'path': str(subject.RESUME_RECEIPT), 'sha256': subject.inventory.CURRENT_OWNERS_SHA}
        def public_json(root, name):
            selected = root / name
            if selected == subject.RESUME_RECEIPT:
                return self.resume_receipt, self.resume_metadata
            self.assertEqual(selected, subject.BUILD_RECEIPT, 'only the selected build receipt may be read')
            return self.build_receipt, {'path': str(selected), 'fixture': True}
        context = patch.object(subject.inventory, 'public_json', side_effect=public_json)
        context.start(); self.addCleanup(context.stop)
        self.inspector = type('Inspector', (), {'run': staticmethod(lambda *args, **kwargs: b'')})()

    def test_exact_bundle_includes_binary_plist_icon_and_directories_with_original_metadata(self):
        result = subject.bundle(self.inspector)
        self.assertEqual(len(result['entries']), 7)
        self.assertEqual(result['binary_sha256'], self.digest)
        self.assertEqual(sum(row['kind'] == 'file' for row in result['entries']), 3)
        self.assertFalse(result['old_native_profile_access'])
        (self.root / 'unexpected').write_text('new')
        with self.assertRaisesRegex(Refused, 'inventory_changed'): subject.bundle(self.inspector)

    def test_modified_binary_or_unreviewed_xattr_cannot_become_capture_artifact(self):
        inspector = type('Inspector', (), {'run': staticmethod(lambda *args, **kwargs: b'com.apple.unreviewed')})()
        with self.assertRaisesRegex(Refused, 'extended_metadata'): subject.bundle(inspector)
        subject.BINARY.write_bytes(b'changed')
        with self.assertRaisesRegex(Refused, 'build_identity'): subject.bundle(self.inspector)

    def test_native_resume_and_compiled_policy_receipts_must_match_selected_generation(self):
        for field, changed in [('pid', 99999), ('inode', 99999), ('sha256', 'b' * 64),
            ('cwd', '/unrelated'), ('executable', '/unrelated/binary'), ('launcher', '/unrelated'),
            ('launcher_sha256', 'b' * 64), ('identity', 'old generation')]:
            original = self.resume_receipt['native'][field]
            self.resume_receipt['native'][field] = changed
            with self.subTest(field=field), self.assertRaisesRegex(Refused, 'resume_receipt'):
                subject.bundle(self.inspector)
            self.resume_receipt['native'][field] = original
        original = self.resume_receipt['native']['session_id']; self.resume_receipt['native']['session_id'] = 99999
        with self.assertRaisesRegex(Refused, 'resume_receipt'): subject.bundle(self.inspector)
        self.resume_receipt['native']['session_id'] = original
        self.build_receipt['status'] = 'built_without_policy'
        with self.assertRaisesRegex(Refused, 'build_identity'): subject.bundle(self.inspector)

    def test_reply_only_registry_hash_cannot_authorize_mentions_native_even_with_current_rows(self):
        self.resume_metadata['sha256'] = subject.inventory.WORKER_OWNERS_SHA
        with self.assertRaisesRegex(Refused, 'resume_receipt'):
            subject.bundle(self.inspector)

    def test_native_provenance_is_bounded_exact_inert_evidence_not_restored_xattrs(self):
        def run(args, **kwargs):
            return b'00112233\n' if '-x' in args else b'com.apple.provenance\n'
        inspector = type('Inspector', (), {'run': staticmethod(run)})()
        result = subject.bundle(inspector)
        self.assertEqual(result['os_metadata_restore'], 'never_reapply_trust_or_provenance')
        self.assertTrue(all(row['os_metadata_evidence_only'] == {'com.apple.provenance': 'ABEiMw=='} for row in result['entries']))
        def oversized(args, **kwargs): return b'00' * 257 if '-x' in args else b'com.apple.provenance\n'
        inspector.run = oversized
        with self.assertRaisesRegex(Refused, 'metadata_bound'): subject.bundle(inspector)

    def test_new_private_client_blocks_capture_even_when_previous_record_says_stopped(self):
        expected = {'artifact': subject.bundle(self.inspector), 'process': None, 'running': False}
        with patch.object(subject, 'candidates', return_value=['99999']):
            with self.assertRaisesRegex(Refused, 'still_running'): subject.capture_entries(self.inspector, expected)
        with patch.object(subject, 'candidates', return_value=[]):
            entries = subject.capture_entries(self.inspector, expected)
            self.assertEqual(len(entries), 7)
            self.assertTrue(all(path.is_relative_to(self.root) and name.startswith('native-client/Ortak Private.app') for path, name in entries))

    def test_processes_outside_exact_private_cwd_do_not_become_signal_or_capture_authority(self):
        def run(args, **kwargs):
            return b'["1", "2"]' if args[-1] == 'buzz-desktop' else \
                ('n' + str(subject.inventory.STATE)).encode() if args[3] == '2' else b'n/preserved-old-stack'
        inspector = type('Inspector', (), {'run': staticmethod(run)})()
        self.assertEqual(subject.candidates(inspector), ['2'])


if __name__ == '__main__': unittest.main()
