"""Descriptor-only bounded reads for the explicitly selected C2 recovery roots.

No source file is created, removed, chmodded or repaired. Open ancestor handles
remain anchored until the final check; retained run locks stay held throughout.
"""

from contextlib import ExitStack
import fcntl
import hashlib
import os
from pathlib import PurePosixPath
import stat
import time

from backup_private_database import Refused

MAX_BINARY = 256 * 1024**2
MAX_DATA = 32 * 1024**2
MAX_ENTRIES = 4096


def require(value, code='workspace_files_refused'):
    """Closed diagnostics never include a rejected path, text or OS exception."""
    if not value: raise Refused(code)


def absolute(value):
    require(isinstance(value, str) and 1 < len(value) <= 4096)
    path = PurePosixPath(value)
    require(path.is_absolute() and str(path) == value and len(path.parts) <= 64
            and '..' not in path.parts and all(ord(c) >= 32 for c in value)
            and '\\' not in value)
    return path


def stamp(row):
    return (row.st_dev, row.st_ino, row.st_uid, row.st_gid, row.st_mode,
            row.st_nlink, row.st_size, row.st_mtime_ns, row.st_ctime_ns)


def identity(row):
    return (row.st_dev, row.st_ino, row.st_uid, row.st_gid, row.st_mode)


class Source:
    """Retain exact descriptors, nonblocking locks and private file fingerprints."""

    def __init__(self, maximum=MAX_BINARY + MAX_DATA):
        self.stack = ExitStack()
        self.links, self.entries, self.listings, self.guards = [], {}, [], []
        self.total = 0
        self.maximum = maximum
        self.deadline = time.monotonic() + 30

    def __enter__(self): return self
    def __exit__(self, *args): return self.stack.__exit__(*args)

    def bound(self):
        require(time.monotonic() < self.deadline, 'workspace_files_deadline')

    def fd(self, name, flags, parent=None):
        self.bound()
        result = os.open(name, flags | os.O_NOFOLLOW | os.O_CLOEXEC | os.O_NONBLOCK,
                         dir_fd=parent)
        self.stack.callback(os.close, result)
        return result

    def root(self, path, private=True):
        """Reject aliases at every ancestor, including an exchanged parent path."""
        path = absolute(path)
        current = self.fd('/', os.O_RDONLY | os.O_DIRECTORY)
        for name in path.parts[1:]:
            child = self.fd(name, os.O_RDONLY | os.O_DIRECTORY, current)
            row = os.fstat(child)
            require(stat.S_ISDIR(row.st_mode) and row.st_uid in (0, os.getuid())
                    and (not row.st_mode & 0o022 or
                         row.st_uid == 0 and row.st_mode & stat.S_ISVTX))
            self.links.append((current, name, child, identity(row)))
            current = child
        if private:
            self.directory(current, (0o700,))
        return current

    def directory(self, fd, modes=(0o500, 0o700)):
        row = os.fstat(fd)
        require(stat.S_ISDIR(row.st_mode) and row.st_uid == os.getuid()
                and stat.S_IMODE(row.st_mode) in modes)
        return row

    def descend(self, parent, name, archive, modes=(0o500, 0o700)):
        child = self.fd(name, os.O_RDONLY | os.O_DIRECTORY, parent)
        row = self.directory(child, modes)
        self.links.append((parent, name, child, identity(row)))
        self.add(archive, child, row, 'directory')
        return child

    def names(self, fd, remember=True):
        """Cap enumeration before allocating an unbounded list."""
        names = []
        with os.scandir(fd) as stream:
            for entry in stream:
                require(len(names) < MAX_ENTRIES, 'workspace_files_count')
                names.append(entry.name)
        result = sorted(names)
        if remember: self.listings.append((fd, result))
        return result

    def add(self, name, fd, row, kind):
        require(name not in self.entries and len(self.entries) < MAX_ENTRIES,
                'workspace_files_count')
        record = {'name': name, 'type': kind, 'mode': stat.S_IMODE(row.st_mode),
                  'uid': row.st_uid, 'gid': row.st_gid, 'mtime_ns': row.st_mtime_ns,
                  'bytes': row.st_size if kind == 'file' else 0}
        self.entries[name] = (record, fd, stamp(row))
        return record

    def artifact(self, parent, name, fd, size):
        """Bind an already-created output to its original descriptor, never a later reopen."""
        row = os.fstat(fd)
        require(stat.S_ISREG(row.st_mode) and row.st_uid == os.getuid() and row.st_nlink == 1
                and stat.S_IMODE(row.st_mode) == 0o600 and row.st_size == size,
                'workspace_files_archive_changed')
        self.links.append((parent, name, fd, identity(row)))
        self.guards.append((fd, stamp(row)))

    def file(self, parent, name, archive, maximum, modes=(0o400,), lock=False):
        fd = self.fd(name, os.O_RDONLY, parent)
        row = os.fstat(fd)
        require(stat.S_ISREG(row.st_mode) and row.st_nlink == 1
                and row.st_uid == os.getuid() and 0 <= row.st_size <= maximum
                and stat.S_IMODE(row.st_mode) in modes)
        self.links.append((parent, name, fd, identity(row)))
        if lock:
            fcntl.flock(fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
        self.total += row.st_size
        require(self.total <= self.maximum, 'workspace_files_bytes')
        record = self.add(archive, fd, row, 'file')
        digest, data = hashlib.sha256(), bytearray()
        for block in self.blocks(fd, row.st_size):
            digest.update(block)
            if maximum <= 131072: data.extend(block)
        record['sha256'] = digest.hexdigest()
        require(stamp(os.fstat(fd)) == stamp(row), 'workspace_files_changed')
        return bytes(data), record

    def blocks(self, fd, size):
        os.lseek(fd, 0, os.SEEK_SET)
        count = 0
        while True:
            self.bound()
            block = os.read(fd, min(65536, size - count + 1))
            if not block: break
            count += len(block)
            require(count <= size, 'workspace_files_changed')
            yield block
        require(count == size, 'workspace_files_changed')

    def check(self):
        """Revalidate descriptors AND their original parent links, not just paths."""
        self.bound()
        for parent, name, child, expected in self.links:
            require(identity(os.stat(name, dir_fd=parent, follow_symlinks=False)) == expected
                    and identity(os.fstat(child)) == expected, 'workspace_files_changed')
        for _, fd, expected in self.entries.values():
            require(stamp(os.fstat(fd)) == expected, 'workspace_files_changed')
        for fd, expected in self.guards:
            require(stamp(os.fstat(fd)) == expected, 'workspace_files_archive_changed')
        for fd, expected in tuple(self.listings):
            require(self.names(fd, remember=False) == expected, 'workspace_files_changed')

    def records(self):
        return [self.entries[name][0] for name in sorted(self.entries)]
