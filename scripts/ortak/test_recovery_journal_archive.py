"""Physical temp-file seams only; SQLite, Docker and provider calls are not used."""

import hashlib
import io
import os
from pathlib import Path
import tarfile
import tempfile
import unittest
from unittest.mock import patch

import recovery_journal_archive as subject


class CallbackStream(io.BytesIO):
    def __init__(self, callback):
        super().__init__()
        self.callback = callback

    def flush(self):
        super().flush()
        self.callback()


class PartialStream(io.BytesIO):
    def read(self, size=-1):
        return super().read(min(size, 37))

    def write(self, value):
        return super().write(value[:41])


class JournalArchiveTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix='ortak-journal-unit-')
        self.addCleanup(self.temporary.cleanup)
        self.base = Path(self.temporary.name).resolve()
        self.root = self.base / 'cold'
        self.root.mkdir(mode=0o700)
        self.uid = os.getuid()
        # Source transfer runs inside Linux. macOS CPython has no xattr API;
        # simulate only that probe while retaining real descriptors and bytes.
        if not hasattr(os, 'listxattr'):
            probe = patch.object(os, 'listxattr', return_value=[], create=True)
            probe.start()
            self.addCleanup(probe.stop)
        self.make('executor.lock', b'')
        # Deliberately not a SQLite database: transfer must preserve raw bytes.
        self.make('journal.sqlite', b'opaque cold bytes\x00\xff\n' * 4000)

    def make(self, name, raw):
        path = self.root / name
        path.write_bytes(raw)
        path.chmod(0o600)
        os.utime(path, ns=(1_783_210_123_123456789, 1_783_210_123_123456789))
        return path

    def encode(self, stream=None):
        stream = stream if stream is not None else io.BytesIO()
        metadata = subject.write(self.root, stream, uid=self.uid)
        return stream.getvalue(), metadata

    def decode(self, raw, name='restored'):
        return subject.extract(io.BytesIO(raw), self.base / name, expected_uid=self.uid)

    def test_physical_roundtrip_preserves_each_companion_presence_and_exact_metadata(self):
        for index, companions in enumerate(((), ('journal.sqlite-wal',), ('journal.sqlite-shm',),
                                            ('journal.sqlite-wal', 'journal.sqlite-shm'))):
            with self.subTest(companions=companions):
                for name in subject.OPTIONAL:
                    (self.root / name).unlink(missing_ok=True)
                for name in companions:
                    self.make(name, (name + '\n').encode() * 53)
                os.utime(self.root, ns=(1_783_210_999_876543210, 1_783_210_999_876543210))
                raw, expected = self.encode()
                target = self.base / f'restored-{index}'
                actual = subject.extract(io.BytesIO(raw), target, expected_uid=self.uid)
                self.assertEqual(actual, expected)
                self.assertEqual(actual['absent'], [n for n in subject.OPTIONAL if n not in companions])
                self.assertEqual(target.stat().st_uid, self.uid)
                for row in expected['files']:
                    original, copy = self.root / row['name'], target / row['name']
                    self.assertEqual(copy.read_bytes(), original.read_bytes())
                    self.assertEqual(copy.stat().st_mtime_ns, original.stat().st_mtime_ns)
                    self.assertEqual(copy.stat().st_mode & 0o7777, 0o600)
                    self.assertEqual(copy.stat().st_nlink, 1)
                with tarfile.open(fileobj=io.BytesIO(raw)) as archive:
                    self.assertEqual(archive.getnames(), ['.'] + [r['name'] for r in expected['files']])

    def test_partial_stream_reads_and_writes_are_not_truncated(self):
        raw, expected = self.encode(PartialStream())
        self.assertEqual(subject.extract(PartialStream(raw), self.base / 'restored', self.uid), expected)

    def test_original_linux_ownership_is_reported_without_host_chown(self):
        _, expected = self.encode()
        expected['root'].update(uid=10001, gid=10002)
        stream = io.BytesIO()
        output = subject.Budget(stream)
        subject.header(output, '.', expected['root'], 0, root=True)
        for row in expected['files']:
            row.update(uid=10001, gid=10002)
            subject.header(output, row['name'], row, row['bytes'])
            output.write((self.root / row['name']).read_bytes())
            output.write(bytes(-row['bytes'] % 512))
        output.write(subject.ZERO * 2)
        target = self.base / 'linux-copy'
        actual = subject.extract(io.BytesIO(stream.getvalue()), target)
        self.assertEqual(actual, expected)
        self.assertTrue(all((target / r['name']).stat().st_uid == self.uid for r in actual['files']))
        self.assertTrue(all(r['uid'] == 10001 and r['gid'] == 10002 for r in actual['files']))

    def test_source_refuses_unknown_symlink_hardlink_mode_owner_and_nonempty_lock(self):
        cases = ('unknown', 'symlink', 'hardlink', 'mode', 'owner', 'lock', 'directory', 'ancestor')
        for case in cases:
            with self.subTest(case=case), tempfile.TemporaryDirectory(dir=self.base) as temporary:
                root = Path(temporary)
                for name in subject.REQUIRED:
                    (root / name).write_bytes(b'')
                    (root / name).chmod(0o600)
                uid = self.uid
                if case == 'unknown': (root / 'journal.sqlite-journal').write_bytes(b'')
                if case == 'symlink':
                    (root / 'journal.sqlite').unlink()
                    (root / 'journal.sqlite').symlink_to(self.root / 'journal.sqlite')
                if case == 'hardlink': os.link(root / 'journal.sqlite', self.base / 'hardlink')
                if case == 'mode': (root / 'journal.sqlite').chmod(0o644)
                if case == 'owner': uid += 1
                if case == 'lock': (root / 'executor.lock').write_bytes(b'occupied')
                if case == 'directory':
                    (root / 'journal.sqlite').unlink()
                    (root / 'journal.sqlite').mkdir()
                if case == 'ancestor':
                    alias = self.base / 'ancestor-alias'
                    alias.symlink_to(root, target_is_directory=True)
                    root = alias
                with self.assertRaises((subject.Refused, OSError)):
                    subject.write(root, io.BytesIO(), uid)

    @unittest.skipUnless(hasattr(os, 'setxattr'), 'native Linux xattrs unavailable on this host')
    def test_actual_xattrs_are_refused_without_removing_them(self):
        path = self.root / 'journal.sqlite'
        name = 'user.ortak_fixture'
        os.setxattr(path, name, b'opaque')
        with self.assertRaises(subject.Refused): self.encode()
        self.assertEqual(os.getxattr(path, name), b'opaque')

    def test_missing_linux_probe_refuses_write_but_not_fresh_inert_host_extraction(self):
        raw, expected = self.encode()
        probe = os.listxattr
        del os.listxattr
        try:
            with self.assertRaises(subject.Refused): self.encode()
            self.assertEqual(self.decode(raw), expected)
        finally:
            os.listxattr = probe

    def test_stream_flush_mutations_never_return_a_successful_source_witness(self):
        for kind in ('bytes', 'replacement', 'companion', 'hardlink', 'mode', 'parent'):
            with self.subTest(kind=kind), tempfile.TemporaryDirectory(dir=self.base) as temporary:
                original = self.root
                self.root = Path(temporary) / 'source'
                self.root.mkdir(mode=0o700)
                self.make('executor.lock', b'')
                path = self.make('journal.sqlite', b'original')
                def change():
                    if kind == 'bytes': path.write_bytes(b'mutated!')
                    if kind == 'replacement':
                        path.unlink(); self.make('journal.sqlite', b'original')
                    if kind == 'companion': self.make('journal.sqlite-wal', b'late')
                    if kind == 'hardlink': os.link(path, Path(temporary) / 'other-link')
                    if kind == 'mode': path.chmod(0o644)
                    if kind == 'parent':
                        self.root.rename(Path(temporary) / 'moved')
                        self.root.mkdir(mode=0o700)
                try:
                    with self.assertRaises((subject.Refused, OSError)):
                        self.encode(CallbackStream(change))
                finally:
                    self.root = original

    def test_decoder_refuses_missing_duplicate_and_noncanonical_members(self):
        def archive(names):
            output = io.BytesIO()
            budget = subject.Budget(output)
            row = dict(mode=0o700, uid=self.uid, gid=os.getgid(), mtime_ns=0)
            subject.header(budget, '.', row, 0, root=True)
            row['mode'] = 0o600
            for name, kind, mode in names:
                if kind == tarfile.REGTYPE:
                    subject.header(budget, name, dict(row, mode=mode), 0)
                else:
                    extended = subject.pax(0)
                    budget.write(subject.raw_header('@PaxHeader', tarfile.XHDTYPE, len(extended)))
                    budget.write(extended + bytes(-len(extended) % 512))
                    budget.write(subject.raw_header(name, kind, 0, mode, self.uid, os.getgid()))
            budget.write(subject.ZERO * 2)
            return output.getvalue()
        normal = [('executor.lock', tarfile.REGTYPE, 0o600), ('journal.sqlite', tarfile.REGTYPE, 0o600)]
        variants = [normal[:1], normal + [normal[-1]], normal[::-1]]
        for name in ('../outside', '/absolute', 'sub/file', 'journal.sqlite-journal'):
            variants.append(normal + [(name, tarfile.REGTYPE, 0o600)])
        variants += [[normal[0], ('journal.sqlite', kind, mode)] for kind, mode in
                     ((tarfile.SYMTYPE, 0o600), (tarfile.LNKTYPE, 0o600),
                      (tarfile.DIRTYPE, 0o700), (tarfile.REGTYPE, 0o644))]
        for index, names in enumerate(variants):
            with self.subTest(index=index), self.assertRaises((subject.Refused, OSError)):
                self.decode(archive(names), f'bad-{index}')

    def test_truncation_trailing_data_metadata_and_owner_corruption_refuse(self):
        raw, _ = self.encode()
        for index, changed in enumerate((raw[:-1], raw + b'not-zero', raw[:600],
                                         raw.replace(b'mtime=', b'ctime=', 1),
                                         b'x' + raw[1:])):
            with self.subTest(index=index), self.assertRaises(subject.Refused):
                self.decode(changed, f'bad-{index}')
        with self.assertRaises(subject.Refused):
            subject.extract(io.BytesIO(raw), self.base / 'wrong-owner', self.uid + 1)

    def test_existing_target_and_symlink_ancestor_are_never_used(self):
        raw, _ = self.encode()
        occupied = self.base / 'occupied'
        occupied.mkdir(mode=0o700)
        (occupied / 'keep').write_bytes(b'retained')
        with self.assertRaises(FileExistsError): self.decode(raw, 'occupied')
        self.assertEqual((occupied / 'keep').read_bytes(), b'retained')
        alias = self.base / 'alias'
        alias.symlink_to(occupied, target_is_directory=True)
        with self.assertRaises(OSError):
            subject.extract(io.BytesIO(raw), alias / 'new', self.uid)
        self.assertFalse((occupied / 'new').exists())

    def test_destination_byte_replacement_during_archive_read_is_detected(self):
        raw, _ = self.encode()
        target = self.base / 'restored'
        class ChangeAtEof(io.BytesIO):
            def read(stream, size=-1):
                value = super().read(size)
                if not value and (target / 'journal.sqlite').exists():
                    (target / 'journal.sqlite').write_bytes(b'changed')
                return value
        with self.assertRaises(subject.Refused):
            subject.extract(ChangeAtEof(raw), target, self.uid)
        self.assertTrue(target.exists())  # Partial evidence is retained, not erased.

    def test_limits_and_deadline_bind_actual_streaming_paths(self):
        with patch.object(subject, 'MAX_FILE_BYTES', 4):
            with self.assertRaises(subject.Refused): self.encode()
        raw, _ = self.encode()
        with patch.object(subject, 'MAX_CONTENT_BYTES', 4):
            with self.assertRaises(subject.Refused): self.decode(raw)
        with patch.object(subject, 'MAX_ARCHIVE_BYTES', 100):
            with self.assertRaises(subject.Refused): self.encode()
        with patch.object(subject, 'SECONDS', 0):
            with self.assertRaises(subject.Refused): self.encode()
            with self.assertRaises(subject.Refused): self.decode(raw, 'deadline')

    def test_payload_digest_is_observed_and_requires_callers_external_pin(self):
        raw, expected = self.encode()
        changed = raw.replace(b'opaque cold bytes', b'changedcold bytes', 1)
        self.assertNotEqual(raw, changed)
        actual = self.decode(changed)
        self.assertNotEqual(actual, expected)
        row = next(r for r in actual['files'] if r['name'] == 'journal.sqlite')
        self.assertEqual(row['sha256'], hashlib.sha256((self.base / 'restored' / row['name']).read_bytes()).hexdigest())


if __name__ == '__main__':
    unittest.main()
