"""Three bounded physical-source seams; no SQLite, Docker, key or runtime use."""

import copy
import hashlib
import io
import os
from pathlib import Path
import tarfile
import tempfile
import unittest

import recovery_native_confidential as subject


class CallbackStream(io.BytesIO):
    def __init__(self, callback):
        super().__init__()
        self.callback = callback

    def flush(self):
        super().flush()
        self.callback()


class NativeConfidentialRecoveryTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix='ortak-native-ciphertext-unit-')
        self.addCleanup(self.temporary.cleanup)
        self.base = Path(self.temporary.name).resolve()
        self.owner = 'a3' * 32

    def app(self, name, present=True):
        app = self.base / name
        app.mkdir(mode=0o700)
        # Ordinary cache must not be traversed or admitted to the archive.
        (app / 'ordinary-cache').mkdir(mode=0o700)
        (app / 'ordinary-cache' / 'never-selected').write_bytes(b'public unrelated canary')
        if present:
            root = app / subject.DIRECTORY
            root.mkdir(mode=0o700)
            # Deliberately not parseable SQLite: preservation must not invoke
            # recovery, decrypt drafts, decode frozen copies or regress ACK bits.
            self.file(root, subject.DATABASE,
                      b'opaque\x00draft-version:7;operation:fixed;1059-copy0\xff;1059-copy1;ack:1,0\n' * 500)
        return app

    def file(self, root, name, raw):
        path = root / name
        path.write_bytes(raw)
        path.chmod(0o600)
        os.utime(path, ns=(1783210123123456789, 1783210123123456789))
        return path

    def capture(self, app, stream=None, stopped=None):
        stream = stream if stream is not None else io.BytesIO()
        result = subject.write(app, stream, stopped_native=stopped or (lambda: self.owner),
                               expected_owner_sha256=self.owner)
        raw = stream.getvalue()
        self.assertEqual(result['archive'], dict(bytes=len(raw), sha256=hashlib.sha256(raw).hexdigest()))
        return raw, result

    def test_exact_cold_presence_absence_and_delete_journal_roundtrip(self):
        for index, mode in enumerate(('absent', 'database', 'delete-journal')):
            with self.subTest(mode=mode):
                app = self.app(f'app-{index}', present=mode != 'absent')
                root = app / subject.DIRECTORY
                if mode == 'delete-journal':
                    self.file(root, subject.JOURNAL, b'opaque rollback preimage\x00\xfe' * 31)
                if root.exists():
                    os.utime(root, ns=(1783210555987654321, 1783210555987654321))
                checks = []
                raw, expected = self.capture(app, stopped=lambda: checks.append('stopped') or self.owner)
                self.assertEqual(checks, ['stopped', 'stopped'])
                target = self.base / f'copy-{index}'
                actual = subject.extract(io.BytesIO(raw), target, expected, app_data=app)
                self.assertEqual(actual, expected)
                self.assertEqual(target.stat().st_mode & 0o7777, 0o700)
                self.assertEqual(target.stat().st_uid, os.getuid())
                store = actual['store']
                self.assertEqual(store['state'], 'absent' if mode == 'absent' else 'cold_ciphertext')
                self.assertEqual(store['rollback_journal_present'], mode == 'delete-journal')
                self.assertNotIn(b'public unrelated canary', raw)
                with tarfile.open(fileobj=io.BytesIO(raw)) as archive:
                    self.assertEqual(archive.getnames(), [subject.MANIFEST] +
                                     ([] if mode == 'absent' else ['.'] + [r['name'] for r in store['files']]))
                self.assertEqual(sorted(p.name for p in target.iterdir()), [r['name'] for r in store['files']])
                for row in store['files']:
                    copied = target / row['name']
                    self.assertEqual(copied.read_bytes(), (root / row['name']).read_bytes())
                    meta = copied.stat()
                    self.assertEqual((meta.st_mode & 0o7777, meta.st_uid, meta.st_gid, meta.st_mtime_ns, meta.st_nlink),
                                     (0o600, row['uid'], row['gid'], row['mtime_ns'], 1))
                with self.assertRaises(subject.Refused):
                    subject.extract(io.BytesIO(raw), target, expected, app_data=app)
                with self.assertRaises(subject.Refused):
                    subject.extract(io.BytesIO(raw), app / 'in-place', expected, app_data=app)

    def test_unknown_sidecars_links_modes_bounds_and_uncontained_owner_refuse(self):
        variants = ('wal', 'shm', 'unknown', 'file-link', 'directory-link', 'hardlink',
                    'mode', 'root-mode', 'oversized', 'aggregate-bound', 'missing', 'owner', 'alias')
        for index, mode in enumerate(variants):
            with self.subTest(mode=mode):
                app = self.app(f'refuse-{index}')
                root = app / subject.DIRECTORY
                path = root / subject.DATABASE
                if mode in ('wal', 'shm', 'unknown'):
                    self.file(root, subject.DATABASE + '-' + mode, b'unknown retained bytes')
                if mode == 'file-link':
                    path.unlink(); path.symlink_to(app / 'ordinary-cache' / 'never-selected')
                if mode == 'directory-link':
                    moved = app / 'moved'; root.rename(moved); root.symlink_to(moved, target_is_directory=True)
                if mode == 'hardlink': os.link(path, app / 'additional-link')
                if mode == 'mode': path.chmod(0o644)
                if mode == 'root-mode': root.chmod(0o755)
                if mode in ('oversized', 'aggregate-bound'):
                    with path.open('r+b') as out:
                        out.truncate(subject.MAX_CONTENT_BYTES + (mode == 'oversized'))
                    if mode == 'aggregate-bound': self.file(root, subject.JOURNAL, b'one-extra-byte')
                if mode == 'missing': path.unlink()
                if mode == 'alias':
                    alias = self.base / 'symlink-app'; alias.symlink_to(app, target_is_directory=True); app = alias
                stream = io.BytesIO()
                with self.assertRaises(subject.Refused):
                    self.capture(app, stream, stopped=(lambda: 'b4' * 32) if mode == 'owner' else None)
                # Refusal retains every original unknown/unsafe object and does
                # not chmod, unlink, truncate or repair the selected source.
                if mode in ('wal', 'shm', 'unknown'):
                    self.assertEqual((root / (subject.DATABASE + '-' + mode)).read_bytes(), b'unknown retained bytes')
                if mode == 'hardlink': self.assertEqual(path.stat().st_nlink, 2)
                if mode == 'mode': self.assertEqual(path.stat().st_mode & 0o777, 0o644)
                if mode == 'owner': self.assertEqual(stream.getvalue(), b'')

    def test_mutation_owner_loss_and_unpinned_archive_never_return_success(self):
        for index, mode in enumerate(('bytes', 'replacement', 'journal', 'absence-became-present', 'owner-loss')):
            with self.subTest(mode=mode):
                app = self.app(f'race-{index}', present=mode != 'absence-became-present')
                root = app / subject.DIRECTORY
                path = root / subject.DATABASE
                observed = [self.owner]
                def mutate():
                    if mode == 'bytes': path.write_bytes(b'changed ciphertext')
                    if mode == 'replacement':
                        raw = path.read_bytes(); path.unlink(); self.file(root, subject.DATABASE, raw)
                    if mode == 'journal': self.file(root, subject.JOURNAL, b'late rollback journal')
                    if mode == 'absence-became-present':
                        root.mkdir(mode=0o700); self.file(root, subject.DATABASE, b'late store')
                    if mode == 'owner-loss': observed[0] = 'c5' * 32
                with self.assertRaises(subject.Refused):
                    self.capture(app, CallbackStream(mutate), stopped=lambda: observed[0])
        app = self.app('archive-corruption')
        raw, expected = self.capture(app)
        with tarfile.open(fileobj=io.BytesIO(raw)) as archive:
            offset = archive.getmember(subject.DATABASE).offset_data
        changed = bytearray(raw); changed[offset] ^= 1
        altered_expected = copy.deepcopy(expected)
        altered_expected['archive']['sha256'] = '00' * 32
        for index, (data, pinned) in enumerate(((bytes(changed), expected), (raw[:-512], expected),
                                                (raw, altered_expected), (raw + b'not-tar', expected))):
            target = self.base / f'bad-copy-{index}'
            with self.subTest(index=index), self.assertRaises(subject.Refused):
                subject.extract(io.BytesIO(data), target, pinned, app_data=app)
            self.assertTrue(target.exists(), 'partial evidence must be retained, never rolled back silently')


if __name__ == '__main__':
    unittest.main()
