"""Explicit C2 file inventory/capture/verification; not selected by the live G73 gate.

The caller owns a held recovery barrier and a bounded helper process. It supplies
fresh same-transaction DB evidence/layout plus actual process and cold-journal
closure evidence. This module neither stops services nor treats elapsed leases,
hashes, restored rows or an operator JSON file as containment authority.
"""

import hashlib
import json
import os
from pathlib import PurePosixPath
import tarfile

from backup_private_database import Refused
from recovery_workspace_io import MAX_BINARY, MAX_DATA, MAX_ENTRIES, Source, absolute, require
from recovery_workspace_layout import build, canonical, digest, observation, selection, sha

FORMAT = 'ortak-workspace-files/v1'
MAX_MANIFEST = 2 * 1024**2
MAX_ARCHIVE = MAX_BINARY + MAX_DATA + MAX_ENTRIES * 2048
ARCHIVE = 'workspace-files.tar'
MANIFEST = 'workspace-files.json'
FAILURE = 'workspace-files-failure.json'


def private_file(parent, name):
    return os.fdopen(os.open(name, os.O_RDWR | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW
                            | os.O_CLOEXEC, 0o600, dir_fd=parent), 'w+b')


def save(parent, name, raw):
    with private_file(parent, name) as stream:
        stream.write(raw); stream.flush(); os.fsync(stream.fileno())
    os.fsync(parent)


class WrittenArchive:
    """Hash the actual write stream before any subsequent observation can mutate it."""

    def __init__(self, stream):
        self.stream, self.sha256, self.size = stream, hashlib.sha256(), 0

    def write(self, raw):
        written = self.stream.write(raw)
        require(written == len(raw), 'workspace_files_archive_write')
        self.sha256.update(raw); self.size += written
        require(self.size <= MAX_ARCHIVE, 'workspace_files_archive_bound')
        return written


def _capture(selected, output, observe):
    selected = json.loads(canonical(selection(selected)))
    destination = absolute(str(output))
    for key in ('input_root', 'run_root', 'reader_binary'):
        path = absolute(selected[key])
        require(path != destination and path not in destination.parents
                and destination not in path.parents, 'workspace_files_output_scope')
    # observe is executable authority supplied by the held barrier, never a
    # caller-authored saved "stopped":true document. Copy its exact bytes now.
    frozen = canonical(observe())
    first = json.loads(frozen)
    grants, runs = observation(first, selected)
    with Source() as source:
        output_fd = source.root(str(destination))
        with os.scandir(output_fd) as entries:
            require(next(entries, None) is None, 'workspace_files_output_occupied')
        try:
            build(source, selected, grants, runs)
            source.check()
            with private_file(output_fd, ARCHIVE) as stream:
                archive_fd = os.dup(stream.fileno())
                source.stack.callback(os.close, archive_fd)
                written = WrittenArchive(stream)
                with tarfile.open(fileobj=written, mode='w|', format=tarfile.USTAR_FORMAT) as archive:
                    for record in source.records():
                        source.bound()
                        member = tarfile.TarInfo(record['name'])
                        member.uid, member.gid, member.mode = record['uid'], record['gid'], record['mode']
                        member.mtime = 0  # Original nanoseconds are retained in the bound manifest.
                        member.type = tarfile.DIRTYPE if record['type'] == 'directory' else tarfile.REGTYPE
                        member.size = record['bytes']
                        if member.isdir():
                            archive.addfile(member)
                        else:
                            fd = source.entries[record['name']][1]
                            with os.fdopen(os.dup(fd), 'rb') as content:
                                content.seek(0)
                                archive.addfile(member, content)
                stream.flush(); os.fsync(stream.fileno())
                source.artifact(output_fd, ARCHIVE, archive_fd, written.size)
            # Check all paths after I/O, then current closure and paths again;
            # a callback-triggered swap cannot slip between these two checks.
            source.check()
            second = observe()
            observation(second, selected)
            require(canonical(second) == frozen, 'workspace_files_closure_changed')
            source.check()
            current_archive = hashlib.sha256()
            for block in source.blocks(archive_fd, written.size): current_archive.update(block)
            require(current_archive.hexdigest() == written.sha256.hexdigest(),
                    'workspace_files_archive_changed')
            manifest = {'format': FORMAT, 'selection': selected,
                'observation_sha256': digest(frozen),
                'database_evidence_sha256': digest(canonical(first['database_evidence'])),
                'workspace_layout_sha256': digest(canonical(first['workspace_layout'])),
                'closure_evidence_sha256': digest(canonical(first['closure_evidence'])),
                'entries': source.records(), 'archive_bytes': written.size,
                'archive_sha256': written.sha256.hexdigest(), 'automatic_activation': False,
                'physical_erasure': False}
            raw = canonical(manifest)
            require(len(raw) <= MAX_MANIFEST, 'workspace_files_manifest_bound')
            source.check()
            save(output_fd, MANIFEST, raw)
            return {'manifest_sha256': digest(raw), 'archive_sha256': manifest['archive_sha256'],
                    'files': len(manifest['entries']), 'automatic_activation': False,
                    'physical_erasure': False}
        except BaseException:
            # Preserve partial archive/manifest bytes for diagnosis, but make
            # even a complete-looking manifest unusable after a failed seal.
            try: save(output_fd, FAILURE, b'{"status":"failed"}\n')
            except OSError: pass
            raise


def capture(selected, output, observe):
    """Write once into a fresh private directory, with all retained locks held.

    ``observe()`` must itself reject open admissions, live/ambiguous writers or
    readers, nonterminal workspace parents/actions and pending cold-journal
    calls. Its raw layout must share the database evidence transaction. Execute
    this helper under the caller's bounded process watchdog (regular file I/O
    may block); an interrupted helper cannot produce an accepted seal.
    """
    try:
        return _capture(selected, output, observe)
    except Refused: raise
    except (OSError, ValueError, TypeError, KeyError, UnicodeError, OverflowError, tarfile.TarError):
        raise Refused('workspace_files_io_refused') from None


def _verify(output, expected_manifest_sha256):
    sha(expected_manifest_sha256)
    with Source(maximum=MAX_ARCHIVE + MAX_MANIFEST) as source:
        root = source.root(str(absolute(str(output))))
        require(source.names(root) == sorted([ARCHIVE, MANIFEST]), 'workspace_files_bundle_incomplete')
        raw, record = source.file(root, MANIFEST, 'manifest', MAX_MANIFEST, (0o600,))
        # Large JSON manifests are still bounded; Source intentionally does not
        # buffer binary-sized files, so read this small selected descriptor here.
        if not raw:
            raw = b''.join(source.blocks(source.entries['manifest'][1], record['bytes']))
        require(digest(raw) == expected_manifest_sha256, 'workspace_files_manifest_changed')
        manifest = json.loads(raw)
        require(canonical(manifest) == raw and isinstance(manifest, dict) and set(manifest) == {
            'format', 'selection', 'observation_sha256', 'database_evidence_sha256',
            'workspace_layout_sha256', 'closure_evidence_sha256', 'entries', 'archive_bytes',
            'archive_sha256', 'automatic_activation', 'physical_erasure'}
            and manifest['format'] == FORMAT and manifest['automatic_activation'] is False
            and manifest['physical_erasure'] is False)
        selection(manifest['selection'])
        for key in ('observation_sha256', 'database_evidence_sha256', 'workspace_layout_sha256',
                    'closure_evidence_sha256', 'archive_sha256'): sha(manifest[key])
        rows = manifest['entries']
        require(isinstance(rows, list) and 1 <= len(rows) <= MAX_ENTRIES
                and type(manifest['archive_bytes']) is int and 0 < manifest['archive_bytes'] <= MAX_ARCHIVE)
        prior = ''
        for row in rows:
            require(isinstance(row, dict) and row.get('type') in ('directory', 'file'))
            require(set(row) == {'name', 'type', 'mode', 'uid', 'gid', 'mtime_ns', 'bytes'}
                    | ({'sha256'} if row['type'] == 'file' else set()))
            name = row['name']
            require(isinstance(name, str) and 0 < len(name) <= 256)
            path = PurePosixPath(name)
            require(path.parts and name > prior and str(path) == name and not path.is_absolute()
                    and '..' not in path.parts and len(name) <= 256
                    and path.parts[0] in ('reader', 'inputs', 'runs')
                    and all(ord(c) >= 32 for c in name) and '\\' not in name)
            require(type(row['bytes']) is int and 0 <= row['bytes'] <= MAX_BINARY
                    and (row['type'] != 'directory' or row['bytes'] == 0)
                    and type(row['mode']) is int and row['mode'] in (0o400, 0o500, 0o600, 0o700, 0o555, 0o755)
                    and type(row['uid']) is int and row['uid'] == manifest['selection']['reader_uid']
                    and type(row['gid']) is int and 0 <= row['gid'] < 2**32
                    and type(row['mtime_ns']) is int)
            if row['type'] == 'file': sha(row['sha256'])
            prior = name
        _, record = source.file(root, ARCHIVE, 'archive', MAX_ARCHIVE, (0o600,))
        require(record['bytes'] == manifest['archive_bytes'] and record['sha256'] == manifest['archive_sha256'],
                'workspace_files_archive_changed')
        fd = source.entries['archive'][1]
        with os.fdopen(os.dup(fd), 'rb') as stream:
            stream.seek(0)
            with tarfile.open(fileobj=stream, mode='r|') as archive:
                count = 0
                for member in archive:
                    require(count < len(rows), 'workspace_files_archive_inventory')
                    row = rows[count]; count += 1
                    require(member.name == row['name'] and not member.pax_headers and not member.linkname
                            and member.type == (tarfile.DIRTYPE if row['type'] == 'directory' else tarfile.REGTYPE)
                            and member.size == row['bytes'] and member.mode == row['mode']
                            and member.uid == row['uid'] and member.gid == row['gid'] and member.mtime == 0,
                            'workspace_files_archive_inventory')
                    if row['type'] == 'file':
                        content = archive.extractfile(member)
                        require(content is not None)
                        value = hashlib.sha256(); size = 0
                        while block := content.read(65536):
                            source.bound(); size += len(block); require(size <= row['bytes'])
                            value.update(block)
                        require(size == row['bytes'] and value.hexdigest() == row['sha256'],
                                'workspace_files_archive_content')
                require(count == len(rows), 'workspace_files_archive_inventory')
        source.check()
        return {'status': 'workspace_files_verified_offline', 'manifest_sha256': expected_manifest_sha256,
                'entries': len(rows), 'automatic_activation': False, 'physical_erasure': False}


def verify(output, expected_manifest_sha256):
    """Check an exact externally pinned manifest and every archived byte; never extract or activate."""
    try: return _verify(output, expected_manifest_sha256)
    except Refused: raise
    except (OSError, ValueError, TypeError, KeyError, UnicodeError, OverflowError, tarfile.TarError):
        raise Refused('workspace_files_verify_refused') from None
