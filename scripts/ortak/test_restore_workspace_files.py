"""Physical offline extraction/readback through production descriptors; synthetic roots only."""

import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import restore_workspace_files as subject
from recovery_workspace_layout import canonical, digest
from test_private_recovery_workspace_files import Fixture, identifier


class RestoreWorkspaceFileTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary.name).resolve(); self.root.chmod(0o700)
        self.x = Fixture(self.root)
        self.receipt = self.x.capture()
        self.target = self.root / 'offline'; self.target.mkdir(mode=0o700)

    def tearDown(self):
        for directory, _, _ in os.walk(self.root): os.chmod(directory, 0o700)
        self.temporary.cleanup()

    def extract(self):
        return subject.extract(self.x.output, self.receipt['manifest_sha256'], self.target)

    def test_restores_actual_reader_input_run_lock_bytes_and_modes_without_sources(self):
        x = self.x
        # Original absolute source paths are gone. Restoration must depend only
        # on the pinned archive; no source-path repair/read/activation is allowed.
        x.input.rename(self.root / 'retained-original-inputs')
        x.runs.rename(self.root / 'retained-original-runs')
        x.binary.rename(self.root / 'retained-original-reader')
        proof = self.extract()
        self.assertEqual(proof['status'], 'workspace_files_restored_offline')
        self.assertFalse(proof['automatic_activation']); self.assertFalse(proof['physical_erasure'])
        restored_input = self.target / 'inputs' / x.revision / x.file_id
        restored_run = self.target / 'runs' / x.company / x.run
        self.assertEqual(restored_input.read_bytes(), x.content)
        self.assertEqual((restored_run / x.file_id).read_bytes(), x.content)
        self.assertEqual((self.target / 'runs' / x.company / (x.run + '.lock')).read_bytes(), b'')
        self.assertEqual((self.target / 'reader').read_bytes(), b'controlled reader fixture\n')
        self.assertEqual(restored_input.stat().st_mode & 0o777, 0o400)
        self.assertEqual(restored_run.stat().st_mode & 0o777, 0o500)
        self.assertFalse(x.input.exists()); self.assertFalse(x.runs.exists()); self.assertFalse(x.binary.exists())

    def test_reviewed75_through78_keep_the_same_physical_capture_and_extraction_contract(self):
        for version in (75, 76, 77, 78):
            with self.subTest(version=version):
                location = self.root / str(version); location.mkdir(mode=0o700)
                x = Fixture(location)
                def observe():
                    value = x.observe()
                    value['database_evidence']['schema_version'] = version
                    value['closure_evidence']['database_evidence_sha256'] = digest(canonical(value['database_evidence']))
                    return value
                receipt = x.capture(observe)
                target = location / 'offline'; target.mkdir(mode=0o700)
                x.input.rename(location / 'retained-inputs')
                x.runs.rename(location / 'retained-runs')
                x.binary.rename(location / 'retained-reader')
                proof = subject.extract(x.output, receipt['manifest_sha256'], target)
                self.assertEqual(proof['status'], 'workspace_files_restored_offline')
                self.assertFalse(proof['automatic_activation'])
                self.assertEqual((target / 'inputs' / x.revision / x.file_id).read_bytes(), x.content)
                copied = target / 'runs' / x.company / x.run
                self.assertEqual((copied / x.file_id).read_bytes(), x.content)
                self.assertEqual((copied / 'manifest.json').read_bytes(), canonical(x.grant))
                self.assertEqual((target / 'runs' / x.company / (x.run + '.lock')).read_bytes(), b'')
                self.assertEqual((target / 'reader').read_bytes(), b'controlled reader fixture\n')
                self.assertEqual(copied.stat().st_mode & 0o777, 0o500)
                self.assertFalse(x.input.exists()); self.assertFalse(x.runs.exists()); self.assertFalse(x.binary.exists())

    def test_existing_destination_and_symlink_alias_never_overwrite(self):
        sentinel = self.target / 'sentinel'; sentinel.write_bytes(b'keep')
        with self.assertRaisesRegex(subject.Refused, 'destination_occupied'): self.extract()
        self.assertEqual(sentinel.read_bytes(), b'keep')
        self.assertFalse((self.target / subject.FAILURE).exists())
        sentinel.unlink()
        alias = self.root / 'alias'; alias.symlink_to(self.target, target_is_directory=True)
        with self.assertRaises(subject.Refused):
            subject.extract(self.x.output, self.receipt['manifest_sha256'], alias)
        self.assertEqual(list(self.target.iterdir()), [])

    def test_failed_preparation_prefix_and_lock_are_physically_restored_without_resuming(self):
        location = self.root / 'failed-prepare-fixture'; location.mkdir(mode=0o700)
        x = Fixture(location); run = identifier(30)
        x.layout['runs'].append({'run_id': run, 'revision': x.revision,
            'manifest_hash': x.grant['manifest_hash'], 'store_ref': None, 'status': 'cancelled'})
        x.layout['readers'].append(x.reader(run, identifier(31)))
        stage = x.company_root / (run + '.preparing'); stage.mkdir(mode=0o700)
        x.write(x.company_root / (run + '.lock'), b'', 0o600)
        x.write(stage / (x.file_id + '.partial'), x.content[:4], 0o600)
        receipt = x.capture()
        proof = subject.extract(x.output, receipt['manifest_sha256'], self.target)
        restored = self.target / 'runs' / x.company
        self.assertEqual((restored / (run + '.preparing') / (x.file_id + '.partial')).read_bytes(), x.content[:4])
        self.assertEqual((restored / (run + '.lock')).read_bytes(), b'')
        self.assertFalse((restored / run).exists())
        self.assertFalse(proof['automatic_activation'])

    def test_no_in_place_source_or_bundle_restore(self):
        for target in (self.x.input, self.x.runs, self.x.output, self.x.input / 'new'):
            with self.subTest(target=target.name), self.assertRaisesRegex(subject.Refused, 'destination_scope'):
                subject.extract(self.x.output, self.receipt['manifest_sha256'], target)

    def test_new_destination_entry_created_at_race_is_never_followed(self):
        real_mkdir = os.mkdir
        def race(name, *args, **kwargs):
            if name == 'inputs' and kwargs.get('dir_fd') is not None:
                os.symlink(str(self.x.input), name, dir_fd=kwargs['dir_fd'])
            return real_mkdir(name, *args, **kwargs)
        with patch.object(subject.os, 'mkdir', side_effect=race), self.assertRaises(subject.Refused): self.extract()
        self.assertTrue((self.target / subject.FAILURE).exists())
        self.assertEqual(self.x.input_file.read_bytes(), self.x.content)
        self.assertTrue((self.target / 'inputs').is_symlink())

    def test_real_readback_detects_changed_bytes_hardlink_and_mode(self):
        for mutation in ('bytes', 'link', 'mode'):
            with self.subTest(mutation=mutation):
                current = self.target / mutation; current.mkdir(mode=0o700)
                real_readback = subject.readback
                def changed(*args):
                    file = current / 'inputs' / self.x.revision / self.x.file_id
                    if mutation == 'bytes':
                        file.chmod(0o600); file.write_bytes(b'x' * len(self.x.content)); file.chmod(0o400)
                    elif mutation == 'link': os.link(file, self.root / 'extra-link')
                    else: file.chmod(0o600)
                    return real_readback(*args)
                with patch.object(subject, 'readback', side_effect=changed), self.assertRaises(subject.Refused):
                    subject.extract(self.x.output, self.receipt['manifest_sha256'], current)
                self.assertTrue((current / subject.FAILURE).exists())
                if mutation == 'link': (self.root / 'extra-link').unlink()

    def test_io_failure_retains_partial_tree_and_cannot_be_retried_in_place(self):
        with patch.object(subject.os, 'write', side_effect=OSError('controlled write failure')), \
                self.assertRaisesRegex(subject.Refused, 'workspace_restore_refused'):
            self.extract()
        self.assertTrue((self.target / subject.FAILURE).exists())
        with self.assertRaisesRegex(subject.Refused, 'destination_occupied'): self.extract()

    def test_parent_swap_after_copy_is_detected_by_original_descriptor_links(self):
        real_readback = subject.readback
        def exchanged(*args):
            (self.target / 'inputs').rename(self.target / 'old-inputs')
            (self.target / 'inputs').symlink_to(self.x.input, target_is_directory=True)
            return real_readback(*args)
        with patch.object(subject, 'readback', side_effect=exchanged), self.assertRaises(subject.Refused): self.extract()
        self.assertTrue((self.target / subject.FAILURE).exists())
        self.assertEqual(self.x.input_file.read_bytes(), self.x.content)


if __name__ == '__main__': unittest.main()
