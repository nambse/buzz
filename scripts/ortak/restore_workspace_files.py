"""Restore a pinned C2 file archive into one fresh inert directory, never in place.

Run under the recovery caller's bounded process watchdog. No reader is executed,
no workspace is registered and no source absolute path is opened or rewritten.
"""

import json
import os
from pathlib import PurePosixPath
import stat
import tarfile

from backup_private_database import Refused
from private_recovery_workspace_files import ARCHIVE, MANIFEST, MAX_ARCHIVE, MAX_MANIFEST, save, verify
from recovery_workspace_io import Source, absolute, identity, require
from recovery_workspace_layout import canonical, digest

FAILURE = 'workspace-restore-failure.json'


def attributes(fd, row):
    """Only newly-created owned descriptors receive retained metadata."""
    current = os.fstat(fd)
    require(current.st_uid == row['uid'] == os.getuid())
    if current.st_gid != row['gid']: os.fchown(fd, -1, row['gid'])
    os.fchmod(fd, row['mode'])
    os.utime(fd, ns=(row['mtime_ns'], row['mtime_ns']))
    os.fsync(fd)


def readback(target, root, rows, directories):
    """Read all restored bytes and exact metadata through the owned parent chain."""
    names = {name: [] for name in directories}
    for row in rows:
        path = PurePosixPath(row['name'])
        names[str(path.parent)].append(path.name)
    for name, expected in names.items():
        require(target.names(directories[name]) == sorted(expected), 'workspace_restore_inventory_changed')
    for row in rows:
        path = PurePosixPath(row['name']); parent = directories[str(path.parent)]
        if row['type'] == 'directory':
            child = directories[row['name']]
            current = os.fstat(child)
            target.links.append((parent, path.name, child, identity(current)))
            actual = target.add(row['name'], child, current, 'directory')
        else:
            _, actual = target.file(parent, path.name, row['name'], row['bytes'], (row['mode'],))
        require(actual == row, 'workspace_restore_readback_changed')
    target.check()


def _extract(bundle, expected_manifest_sha256, destination):
    # This is an actual archive verification, not a receipt flag read. The
    # pinned bytes are opened and rechecked under descriptors again below.
    verify(bundle, expected_manifest_sha256)
    bundle, destination = absolute(str(bundle)), absolute(str(destination))
    require(bundle != destination and bundle not in destination.parents
            and destination not in bundle.parents, 'workspace_restore_destination_scope')
    with Source(maximum=MAX_ARCHIVE + MAX_MANIFEST) as source, Source() as target:
        source_root = source.root(str(bundle))
        require(source.names(source_root) == sorted([ARCHIVE, MANIFEST]))
        _, manifest_record = source.file(source_root, MANIFEST, 'manifest', MAX_MANIFEST, (0o600,))
        raw = b''.join(source.blocks(source.entries['manifest'][1], manifest_record['bytes']))
        require(digest(raw) == expected_manifest_sha256, 'workspace_files_manifest_changed')
        manifest = json.loads(raw)
        require(canonical(manifest) == raw)
        for key in ('input_root', 'run_root', 'reader_binary'):
            original = absolute(manifest['selection'][key])
            require(destination != original and destination not in original.parents
                    and original not in destination.parents, 'workspace_restore_destination_scope')
        root = target.root(str(destination))
        require(not target.names(root, remember=False), 'workspace_restore_destination_occupied')
        try:
            _, archived = source.file(source_root, ARCHIVE, 'archive', MAX_ARCHIVE, (0o600,))
            require(archived['sha256'] == manifest['archive_sha256']
                    and archived['bytes'] == manifest['archive_bytes'], 'workspace_files_archive_changed')
            directories, rows, index = {'.': root}, manifest['entries'], 0
            with os.fdopen(os.dup(source.entries['archive'][1]), 'rb') as stream:
                stream.seek(0)
                with tarfile.open(fileobj=stream, mode='r|') as archive:
                    for member in archive:
                        target.bound()
                        require(index < len(rows), 'workspace_restore_archive_changed')
                        row = rows[index]; index += 1
                        require(member.name == row['name'] and not member.linkname and not member.pax_headers
                                and member.type == (tarfile.DIRTYPE if row['type'] == 'directory' else tarfile.REGTYPE)
                                and member.size == row['bytes'] and member.mode == row['mode']
                                and member.uid == row['uid'] and member.gid == row['gid'] and member.mtime == 0,
                                'workspace_restore_archive_changed')
                        path = PurePosixPath(row['name'])
                        require(str(path.parent) in directories, 'workspace_restore_parent_missing')
                        parent = directories[str(path.parent)]
                        if row['type'] == 'directory':
                            os.mkdir(path.name, mode=0o700, dir_fd=parent)
                            child = target.fd(path.name, os.O_RDONLY | os.O_DIRECTORY, parent)
                            target.directory(child, (0o700,))
                            directories[row['name']] = child
                        else:
                            fd = os.open(path.name, os.O_WRONLY | os.O_CREAT | os.O_EXCL
                                         | os.O_NOFOLLOW | os.O_CLOEXEC, 0o600, dir_fd=parent)
                            target.stack.callback(os.close, fd)
                            current = os.fstat(fd)
                            require(stat.S_ISREG(current.st_mode) and current.st_nlink == 1
                                    and current.st_uid == os.getuid() and current.st_size == 0)
                            content = archive.extractfile(member); require(content is not None)
                            count = 0
                            while block := content.read(65536):
                                target.bound(); count += len(block); require(count <= row['bytes'])
                                view = memoryview(block)
                                while view:
                                    written = os.write(fd, view); require(written > 0); view = view[written:]
                            require(count == row['bytes'])
                            attributes(fd, row)
                        os.fsync(parent)
            require(index == len(rows), 'workspace_restore_archive_changed')
            for row in sorted((row for row in rows if row['type'] == 'directory'),
                              key=lambda row: len(PurePosixPath(row['name']).parts), reverse=True):
                attributes(directories[row['name']], row)
            os.fsync(root)
            readback(target, root, rows, directories)
            source.check(); target.check()
            return {'status': 'workspace_files_restored_offline', 'manifest_sha256': expected_manifest_sha256,
                    'archive_sha256': manifest['archive_sha256'], 'entries': len(rows),
                    'tree_sha256': digest(canonical(rows)), 'automatic_activation': False,
                    'physical_erasure': False}
        except BaseException:
            try: save(root, FAILURE, b'{"status":"failed"}\n')
            except OSError: pass
            raise


def extract(bundle, expected_manifest_sha256, destination):
    """Create and read back only an externally pinned fresh offline tree; never execute it.

    Destination must already be empty/current-UID/0700 and disjoint from both
    the bundle and every original selected path. Failed trees are retained and
    cannot be retried in place. The outer recovery caller must persist this
    returned proof before claiming a complete offline foundation.
    """
    try: return _extract(bundle, expected_manifest_sha256, destination)
    except Refused: raise
    except (OSError, ValueError, TypeError, KeyError, UnicodeError, OverflowError, tarfile.TarError):
        raise Refused('workspace_restore_refused') from None
