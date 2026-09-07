"""Real local descriptor/flock/archive seams, with fresh synthetic files only."""

import copy
import fcntl
import json
import os
from pathlib import Path
import tarfile
import tempfile
import unittest
from unittest.mock import patch
from uuid import UUID

import private_recovery_workspace_files as subject
import recovery_workspace_io as files
from recovery_workspace_layout import canonical, digest
from private_recovery_workspaces import TABLE_KEYS


def identifier(n): return str(UUID(int=n))


class Fixture:
    def __init__(self, root):
        self.root = root
        self.company, self.revision, self.run, self.file_id = map(identifier, (1, 2, 3, 4))
        self.input, self.runs, self.output = (root / name for name in ('inputs', 'runs', 'bundle'))
        for path in (self.input, self.runs, self.output): path.mkdir(mode=0o700)
        self.binary = root / 'reader'
        self.write(self.binary, b'controlled reader fixture\n', 0o500)
        self.selected = {'company_id': self.company, 'input_root': str(self.input),
            'run_root': str(self.runs), 'reader_binary': str(self.binary),
            'reader_sha256': digest(self.binary.read_bytes()), 'reader_uid': os.getuid()}
        for path, name in ((self.input, '.ortak-workspace-inputs-v1'), (self.runs, '.ortak-workspace-runs-v1')):
            self.write(path / name, f'ortak-workspace/v1:{self.company}\n'.encode())
        self.content = 'synthetic C2 input: Iğdır\n'.encode()
        self.grant = {'format': 'ortak-workspace-read/v1', 'company_id': self.company,
            'project_id': identifier(5), 'employee_id': 'fixture-employee', 'workspace_ref': 'fixture:read',
            'revision': self.revision, 'files': [{'file_id': self.file_id, 'name': 'notes/answer.txt',
                'media_type': 'text/plain', 'bytes': len(self.content), 'sha256': digest(self.content)}]}
        self.grant['manifest_hash'] = digest(canonical(self.grant))
        (self.input / self.revision).mkdir(mode=0o700)
        self.input_file = self.input / self.revision / self.file_id
        self.write(self.input_file, self.content)
        self.company_root = self.runs / self.company
        self.company_root.mkdir(mode=0o700)
        self.copy = self.company_root / self.run
        self.copy.mkdir(mode=0o700)
        self.write(self.copy / self.file_id, self.content)
        self.write(self.copy / 'manifest.json', canonical(self.grant))
        self.copy.chmod(0o500)
        self.lock = self.company_root / (self.run + '.lock')
        self.write(self.lock, b'', 0o600)
        self.layout = {'company_id': self.company,
            'bindings': [{'revision': self.revision, 'grant_bytes': canonical(self.grant).decode()}],
            'runs': [{'run_id': self.run, 'revision': self.revision,
                'manifest_hash': self.grant['manifest_hash'],
                'store_ref': f'workspace-run:{self.company}:{self.run}', 'status': 'completed'}],
            'readers': [self.reader(self.run, identifier(6))]}

    def reader(self, run, token):
        return {'id': token, 'run_id': run, 'revision': self.revision,
            'executable': str(self.binary), 'executable_hash': self.selected['reader_sha256'],
            'operating_uid': os.getuid(), 'state': 'stopped', 'stop_proof': 'reaped',
            'created_at': '2026-09-01T00:00:00+00:00', 'owner_deadline': '2026-09-01T00:00:08+00:00',
            'stopped_at': '2026-09-01T00:00:02+00:00'}

    @staticmethod
    def write(path, raw, mode=0o400):
        if path.exists(): path.chmod(0o600)
        path.write_bytes(raw); path.chmod(mode)

    def observe(self):
        tables = {name: [] for name in TABLE_KEYS}
        def add(name, *key): tables[name].append({'key': [self.company, *key], 'row_sha256': 'a' * 64})
        for binding in self.layout['bindings']:
            add('workspace_bindings', binding['revision'])
            for file in json.loads(binding['grant_bytes'])['files']:
                add('workspace_files', binding['revision'], file['file_id'])
        for row in self.layout['runs']:
            if row['store_ref']: add('run_workspace_uses', row['run_id'])
        for row in self.layout['readers']: add('workspace_reader_executions', row['id'])
        evidence = {'schema_version': 74, 'company_id': self.company, 'tables': tables}
        return copy.deepcopy({'database_evidence': evidence, 'workspace_layout': self.layout,
            'closure_evidence': {'format': 'ortak-workspace-files-closure/v1', 'barrier_id': identifier(7),
                'selection_sha256': digest(canonical(self.selected)), 'database_evidence_sha256': digest(canonical(evidence)),
                'journal_sha256': 'b' * 64, 'process_observation_sha256': 'c' * 64,
                'workspace_journal_pending': 0, 'live_reader_count': 0, 'live_writer_count': 0}})

    def capture(self, observe=None):
        return subject.capture(self.selected, self.output, observe or self.observe)


class WorkspaceFileTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary.name).resolve()
        self.root.chmod(0o700)
        self.x = Fixture(self.root)

    def tearDown(self):
        # Synthetic sealed directories are explicitly made removable only by
        # the fixture cleanup, never by the production capture helper.
        for directory, _, _ in os.walk(self.root): os.chmod(directory, 0o700)
        self.temporary.cleanup()

    def test_exact_bytes_markers_binary_lock_and_run_manifest_survive(self):
        x = self.x
        before = {str(path): (path.read_bytes(), files.stamp(path.stat()))
                  for path in (x.input_file, x.copy / x.file_id, x.copy / 'manifest.json', x.lock, x.binary)}
        receipt = x.capture()
        proof = subject.verify(x.output, receipt['manifest_sha256'])
        self.assertFalse(proof['automatic_activation']); self.assertFalse(proof['physical_erasure'])
        with tarfile.open(x.output / subject.ARCHIVE) as archive:
            self.assertEqual(archive.extractfile(f'inputs/{x.revision}/{x.file_id}').read(), x.content)
            self.assertEqual(archive.extractfile(f'runs/{x.company}/{x.run}/{x.file_id}').read(), x.content)
            self.assertEqual(archive.extractfile(f'runs/{x.company}/{x.run}.lock').read(), b'')
            self.assertEqual(archive.extractfile('reader').read(), x.binary.read_bytes())
        for path, expected in before.items():
            self.assertEqual((Path(path).read_bytes(), files.stamp(Path(path).stat())), expected)
        # Two captures never overwrite immutable output or produce a failure
        # marker over a previously accepted archive.
        with self.assertRaisesRegex(subject.Refused, 'output_occupied'): x.capture()
        subject.verify(x.output, receipt['manifest_sha256'])

    def test_unreviewed_or_malformed_database_version_refuses_before_capture(self):
        x = self.x
        for version in (73, 79, True, 74.0, '75', None):
            value = x.observe()
            value['database_evidence']['schema_version'] = version
            value['closure_evidence']['database_evidence_sha256'] = digest(canonical(value['database_evidence']))
            with self.subTest(version=version), self.assertRaises(subject.Refused):
                x.capture(lambda: value)
            self.assertEqual(list(x.output.iterdir()), [])

    def test_failed_prepare_prefixes_are_retained_without_claiming_a_sealed_copy(self):
        x = self.x; run = identifier(20)
        x.layout['runs'].append({'run_id': run, 'revision': x.revision,
            'manifest_hash': x.grant['manifest_hash'], 'store_ref': None, 'status': 'cancelled'})
        x.layout['readers'].append(x.reader(run, identifier(21)))
        stage = x.company_root / (run + '.preparing'); stage.mkdir(mode=0o700)
        x.write(x.company_root / (run + '.lock'), b'', 0o600)
        x.write(stage / (x.file_id + '.partial'), x.content[:5], 0o600)
        receipt = x.capture(); subject.verify(x.output, receipt['manifest_sha256'])
        with tarfile.open(x.output / subject.ARCHIVE) as archive:
            self.assertEqual(archive.extractfile(f'runs/{x.company}/{run}.preparing/{x.file_id}.partial').read(), x.content[:5])
        self.assertTrue(stage.exists())
        self.assertFalse((x.company_root / run).exists())

    def test_bad_marker_wrong_binary_digest_and_unsafe_modes_refuse(self):
        x = self.x
        for mode in (0o444, 0o600, 0o404):
            x.input_file.chmod(mode)
            with self.subTest(mode=mode), self.assertRaises(subject.Refused): x.capture()
            self.clear_failed()
        x.input_file.chmod(0o400)
        x.write(x.input / '.ortak-workspace-inputs-v1', f'ortak-workspace/v1:{identifier(99)}\n'.encode())
        with self.assertRaisesRegex(subject.Refused, 'marker_differs'): x.capture()
        self.clear_failed()
        x.selected['reader_sha256'] = 'f' * 64
        with self.assertRaisesRegex(subject.Refused, 'reader_identity'): x.capture()

    def clear_failed(self):
        for path in self.x.output.iterdir(): path.unlink()

    def test_symlink_at_root_parent_file_or_run_copy_never_follows(self):
        x = self.x
        alias = self.root / 'alias'; alias.symlink_to(x.input, target_is_directory=True)
        original = x.selected['input_root']; x.selected['input_root'] = str(alias)
        with self.assertRaises(subject.Refused): x.capture()
        x.selected['input_root'] = original; self.clear_failed()
        saved = self.root / 'saved'; x.input_file.rename(saved); x.input_file.symlink_to(saved)
        with self.assertRaises(subject.Refused): x.capture()
        self.clear_failed(); x.input_file.unlink(); saved.rename(x.input_file)
        x.copy.chmod(0o700)
        copied = x.copy / x.file_id; copied.unlink(); copied.symlink_to(x.input_file)
        x.copy.chmod(0o500)
        with self.assertRaises(subject.Refused): x.capture()

    def test_hardlink_fifo_unknown_file_and_traversal_refuse_before_read(self):
        x = self.x
        os.link(x.input_file, self.root / 'second-link')
        with self.assertRaises(subject.Refused): x.capture()
        (self.root / 'second-link').unlink(); self.clear_failed()
        x.input_file.unlink(); os.mkfifo(x.input_file, 0o400)
        with self.assertRaises(subject.Refused): x.capture()
        x.input_file.unlink(); self.clear_failed(); x.write(x.input_file, x.content)
        x.write(x.input / x.revision / 'unselected-private-file', b'never selected')
        with self.assertRaisesRegex(subject.Refused, 'inventory_differs'): x.capture()
        self.clear_failed()
        x.selected['input_root'] += '/../inputs'
        with self.assertRaises(subject.Refused): x.capture()

    def test_live_lease_timer_and_cold_journal_are_not_stopped_proof(self):
        x = self.x
        for key in ('live_reader_count', 'live_writer_count', 'workspace_journal_pending'):
            value = x.observe(); value['closure_evidence'][key] = 1
            with self.subTest(key=key), self.assertRaisesRegex(subject.Refused, 'closure_required'):
                x.capture(lambda: value)
        for state, proof, stopped in [('running', 'reaped', '2026-09-02T00:00:00Z'),
                                      ('stopped', 'confirmed_absence', '2026-09-01T00:00:02Z')]:
            value = x.observe(); row = value['workspace_layout']['readers'][0]
            row.update(state=state, stop_proof=proof, stopped_at=stopped)
            with self.assertRaisesRegex(subject.Refused, 'readers_not_contained'): x.capture(lambda: value)
        value = x.observe(); value['workspace_layout']['runs'][0]['status'] = 'running'
        with self.assertRaisesRegex(subject.Refused, 'runs_not_terminal'): x.capture(lambda: value)

    def test_actual_lock_contention_refuses_and_capture_keeps_lock_until_final_observation(self):
        x = self.x
        with x.lock.open('rb') as held:
            fcntl.flock(held, fcntl.LOCK_EX | fcntl.LOCK_NB)
            with self.assertRaises(subject.Refused): x.capture()
        self.clear_failed()
        count = 0
        def observe():
            nonlocal count
            count += 1
            if count == 2:
                with x.lock.open('rb') as competitor:
                    with self.assertRaises(BlockingIOError):
                        fcntl.flock(competitor, fcntl.LOCK_EX | fcntl.LOCK_NB)
            return x.observe()
        x.capture(observe)
        self.assertEqual(count, 2)
        with x.lock.open('rb') as after: fcntl.flock(after, fcntl.LOCK_EX | fcntl.LOCK_NB)

    def test_changed_closure_same_size_input_and_exchanged_parent_refuse(self):
        x = self.x
        for action in ('closure', 'bytes', 'parent'):
            calls = 0
            def observe():
                nonlocal calls
                calls += 1
                result = x.observe()
                if calls == 2:
                    if action == 'closure': result['closure_evidence']['journal_sha256'] = 'd' * 64
                    elif action == 'bytes':
                        x.write(x.input_file, b'x' * len(x.content))
                    else:
                        old = self.root / 'old-input'; x.input.rename(old)
                        x.input.mkdir(mode=0o700)
                return result
            with self.subTest(action=action), self.assertRaisesRegex(subject.Refused, 'changed'):
                x.capture(observe)
            self.assertTrue((x.output / subject.FAILURE).exists())
            self.assertFalse((x.output / subject.MANIFEST).exists())
            self.clear_failed()
            if action == 'bytes': x.write(x.input_file, x.content)
            if action == 'parent': x.input.rmdir(); (self.root / 'old-input').rename(x.input)

    def test_missing_projection_or_cross_company_or_reader_binary_refuses(self):
        x = self.x
        for mutate in (lambda value: value['workspace_layout']['bindings'].clear(),
                       lambda value: value['database_evidence']['tables']['workspace_reader_executions'].clear(),
                       lambda value: value['workspace_layout']['readers'][0].update(operating_uid=os.getuid()+1),
                       lambda value: value['workspace_layout'].update(company_id=identifier(99))):
            value = x.observe(); mutate(value)
            # Keep cryptographic equality, so semantic guards must refuse.
            value['closure_evidence']['database_evidence_sha256'] = digest(canonical(value['database_evidence']))
            with self.assertRaises(subject.Refused): x.capture(lambda: value)

    def test_retained_grant_bytes_order_hash_and_bounds_are_not_reconstructed(self):
        x = self.x
        for raw in (json.dumps(x.grant), canonical(x.grant).decode().replace('fixture:read', 'changed:read'),
                    ' ' * 16385):
            value = x.observe(); value['workspace_layout']['bindings'][0]['grant_bytes'] = raw
            with self.subTest(size=len(raw)), self.assertRaises(subject.Refused): x.capture(lambda: value)
        value = x.observe(); value['workspace_layout']['runs'].append(copy.deepcopy(value['workspace_layout']['runs'][0]))
        with self.assertRaisesRegex(subject.Refused, 'ambiguous_run_revision'): x.capture(lambda: value)
        value = x.observe(); value['workspace_layout']['readers'] *= 129
        with self.assertRaisesRegex(subject.Refused, 'layout_bound'): x.capture(lambda: value)

    def test_copies_manifests_and_preparation_residue_cannot_hide_extra_bytes(self):
        x = self.x
        x.write(x.copy / 'manifest.json', canonical(x.grant) + b' ')
        with self.assertRaises(subject.Refused): x.capture()
        self.clear_failed(); x.write(x.copy / 'manifest.json', canonical(x.grant))
        x.write(x.copy / x.file_id, b'x' * len(x.content))
        with self.assertRaisesRegex(subject.Refused, 'run_copy_changed'): x.capture()
        self.clear_failed(); x.write(x.copy / x.file_id, x.content)
        x.copy.chmod(0o700)
        with self.assertRaises(subject.Refused): x.capture()
        self.clear_failed(); x.copy.chmod(0o500)
        x.layout['runs'][0]['store_ref'] = None
        x.copy.chmod(0o700)
        x.copy.rename(x.company_root / (x.run + '.preparing'))
        x.copy = x.company_root / (x.run + '.preparing')
        x.write(x.copy / (x.file_id + '.partial'), b'not an original prefix', 0o600)
        with self.assertRaisesRegex(subject.Refused, 'run_copy_changed'): x.capture()

    def test_pinned_reader_permissions_nlink_and_file_size_ceiling(self):
        x = self.x
        x.binary.chmod(0o777)
        with self.assertRaises(subject.Refused): x.capture()
        self.clear_failed(); x.binary.chmod(0o500)
        os.link(x.binary, self.root / 'reader-link')
        with self.assertRaises(subject.Refused): x.capture()
        self.clear_failed(); (self.root / 'reader-link').unlink()
        x.write(x.input_file, b'x' * 16385)
        with self.assertRaises(subject.Refused): x.capture()

    def test_file_and_revision_owner_checks_bind_actual_descriptor_stat_seam(self):
        x = self.x
        real_stat, real_fstat = os.stat, os.fstat
        for path in (x.input_file, x.input_file.parent):
            inode = real_stat(path).st_ino
            def changed(row):
                if row.st_ino != inode: return row
                fields = list(row); fields[4] = os.getuid() + 1
                return os.stat_result(fields)
            # A non-root test cannot chown. Both descriptor and path metadata
            # expose the same synthetic foreign owner, so only the production
            # owner predicate (not a mocked inconsistency) rejects this case.
            with patch.object(files.os, 'stat', side_effect=lambda *a, **k: changed(real_stat(*a, **k))), \
                    patch.object(files.os, 'fstat', side_effect=lambda fd: changed(real_fstat(fd))), \
                    self.assertRaises(subject.Refused):
                x.capture()
            self.clear_failed()

    def test_writable_nonsticky_ancestor_is_not_a_private_root(self):
        x = self.x
        parent = self.root / 'unsafe-parent'; parent.mkdir(mode=0o777); parent.chmod(0o777)
        x.input.rename(parent / 'inputs'); x.selected['input_root'] = str(parent / 'inputs')
        with self.assertRaises(subject.Refused): x.capture()

    def test_output_cannot_be_inside_selected_roots_or_reachable_through_alias(self):
        x = self.x
        nested = x.input / 'capture'; nested.mkdir(mode=0o700)
        with self.assertRaisesRegex(subject.Refused, 'output_scope'):
            subject.capture(x.selected, nested, x.observe)
        alias = self.root / 'bundle-alias'; alias.symlink_to(x.output, target_is_directory=True)
        with self.assertRaises(subject.Refused): subject.capture(x.selected, alias, x.observe)

    def test_final_callback_exception_propagates_and_leaves_failure_evidence(self):
        x = self.x; calls = 0
        def observe():
            nonlocal calls
            calls += 1
            if calls == 2: raise subject.Refused('synthetic_closure_unavailable')
            return x.observe()
        with self.assertRaisesRegex(subject.Refused, 'synthetic_closure_unavailable'): x.capture(observe)
        self.assertTrue((x.output / subject.FAILURE).exists())
        self.assertFalse((x.output / subject.MANIFEST).exists())

    def test_seal_directory_fsync_failure_is_retained_and_never_verifies(self):
        x = self.x; real = os.fsync; failed = False
        def fsync(fd):
            nonlocal failed
            if not failed and os.fstat(fd).st_ino == x.output.stat().st_ino:
                failed = True; raise OSError('synthetic')
            return real(fd)
        with patch.object(subject.os, 'fsync', side_effect=fsync), self.assertRaises(subject.Refused): x.capture()
        self.assertTrue((x.output / subject.MANIFEST).exists())
        self.assertTrue((x.output / subject.FAILURE).exists())
        with self.assertRaisesRegex(subject.Refused, 'bundle_incomplete'):
            subject.verify(x.output, digest((x.output / subject.MANIFEST).read_bytes()))

    def test_archive_bytes_manifest_pin_and_member_link_are_checked(self):
        x = self.x; receipt = x.capture()
        with self.assertRaisesRegex(subject.Refused, 'manifest_changed'): subject.verify(x.output, 'f' * 64)
        archive_path = x.output / subject.ARCHIVE
        raw = bytearray(archive_path.read_bytes()); raw[520] ^= 1; archive_path.write_bytes(raw)
        with self.assertRaisesRegex(subject.Refused, 'archive_changed'):
            subject.verify(x.output, receipt['manifest_sha256'])
        # Even an externally repinned archive may not introduce link members.
        with tarfile.open(archive_path, 'w') as archive:
            member = tarfile.TarInfo('inputs'); member.type = tarfile.SYMTYPE; member.linkname = '/'
            archive.addfile(member)
        manifest_path = x.output / subject.MANIFEST
        value = json.loads(manifest_path.read_bytes())
        value.update(archive_bytes=archive_path.stat().st_size, archive_sha256=digest(archive_path.read_bytes()))
        manifest_path.write_bytes(canonical(value))
        with self.assertRaisesRegex(subject.Refused, 'archive_inventory'):
            subject.verify(x.output, digest(canonical(value)))

    def test_capture_never_seals_a_new_baseline_for_tampered_created_archive(self):
        x = self.x
        for mutation in ('bytes', 'replace', 'link', 'mode'):
            calls = 0
            def observe():
                nonlocal calls
                calls += 1
                if calls == 2:
                    archive = x.output / subject.ARCHIVE
                    if mutation == 'bytes':
                        with archive.open('r+b') as stream:
                            stream.seek(520); stream.write(b'x')
                    elif mutation == 'replace':
                        raw = archive.read_bytes(); archive.unlink(); archive.write_bytes(raw); archive.chmod(0o600)
                    elif mutation == 'link': os.link(archive, self.root / 'archive-link')
                    else: archive.chmod(0o644)
                return x.observe()
            with self.subTest(mutation=mutation), self.assertRaisesRegex(subject.Refused, 'changed'):
                x.capture(observe)
            self.assertTrue((x.output / subject.FAILURE).exists())
            self.assertFalse((x.output / subject.MANIFEST).exists())
            self.clear_failed()
            if mutation == 'link': (self.root / 'archive-link').unlink()


if __name__ == '__main__': unittest.main()
