"""Exact cold journal transfer, with no SQLite access or runtime activation.

The caller must hold the Linux journal/executor leases and contain all writers.
It also owns a process watchdog: the 30-second checks here cannot interrupt a
blocked caller-supplied stream or filesystem syscall. Extraction creates a new
private directory; failures retain its partial bytes and never claim success.
Only transferable metadata is returned. Source inode/ctime and parent links are
checked, not mistaken for identities that survive physical restoration.
"""

from contextlib import ExitStack
import hashlib
import os
from pathlib import Path
import re
import stat
import tarfile
import time

MAX_FILE_BYTES = 64 * 1024**2
MAX_CONTENT_BYTES = 192 * 1024**2
MAX_ARCHIVE_BYTES = MAX_CONTENT_BYTES + 64 * 1024
SECONDS = 30
CHUNK = 65536
NAMES = ('executor.lock', 'journal.sqlite', 'journal.sqlite-shm', 'journal.sqlite-wal')
REQUIRED = {'executor.lock', 'journal.sqlite'}
OPTIONAL = ('journal.sqlite-shm', 'journal.sqlite-wal')
ZERO = bytes(512)


class Refused(ValueError):
    """Closed failure without source names, paths or journal content."""


def require(ok):
    if not ok:
        raise Refused('cold_journal_archive_refused')


class Budget:
    def __init__(self, stream):
        self.stream, self.count = stream, 0
        self.deadline = time.monotonic() + SECONDS

    def check(self):
        require(time.monotonic() < self.deadline)

    def read(self, size):
        self.check()
        require(0 < size <= CHUNK)
        raw = self.stream.read(size)
        require(isinstance(raw, bytes) and len(raw) <= size)
        self.count += len(raw)
        require(self.count <= MAX_ARCHIVE_BYTES)
        self.check()
        return raw

    def exact(self, size):
        require(0 <= size <= CHUNK)
        result = bytearray()
        while len(result) < size:
            block = self.read(size - len(result))
            require(block)
            result.extend(block)
        return bytes(result)

    def write(self, raw):
        require(self.count + len(raw) <= MAX_ARCHIVE_BYTES)
        view = memoryview(raw)
        while view:
            self.check()
            count = self.stream.write(view[:CHUNK])
            require(type(count) is int and 0 < count <= min(CHUNK, len(view)))
            self.count += count
            view = view[count:]
            self.check()


def identity(info):
    return (info.st_dev, info.st_ino, info.st_mode, info.st_uid, info.st_gid)


def stamp(info):
    return identity(info) + (info.st_nlink, info.st_size, info.st_mtime_ns, info.st_ctime_ns)


def opened(stack, path, flags, *, dir_fd=None, mode=0o600):
    fd = os.open(path, flags | os.O_NOFOLLOW | os.O_NONBLOCK, mode, dir_fd=dir_fd)
    stack.callback(os.close, fd)
    return fd


def directory(stack, path):
    """Keep every ancestor descriptor/link; never follow a symlink component."""
    value = os.fspath(path)
    require(isinstance(value, str) and value.startswith('/')
            and str(Path(value)) == value and all(p not in ('.', '..') for p in value.split('/')[1:]))
    fd = opened(stack, '/', os.O_RDONLY | os.O_DIRECTORY)
    links = []
    for name in Path(value).parts[1:]:
        child = opened(stack, name, os.O_RDONLY | os.O_DIRECTORY, dir_fd=fd)
        links.append((fd, name, child, identity(os.fstat(child))))
        fd = child
    return fd, links


def check_links(links):
    for parent, name, child, expected in links:
        require(identity(os.fstat(child)) == expected
                and identity(os.stat(name, dir_fd=parent, follow_symlinks=False)) == expected)


def no_xattrs(fd, *, required=True):
    # The actual writer is Linux and must prove this property. Some host Python
    # builds lack xattr APIs; fresh inert extraction never installs archive
    # xattrs (the wire admits only mtime), matching the host recovery contract.
    if hasattr(os, 'listxattr'):
        require(not os.listxattr(fd))
    else:
        require(not required)


def metadata(info, uid, *, root=False):
    require(type(uid) is int and 0 <= uid < 2**21)
    require(info.st_uid == uid and 0 <= info.st_gid < 2**21
            and -(2**63) < info.st_mtime_ns < 2**63
            and stat.S_IMODE(info.st_mode) == (0o700 if root else 0o600))
    require(stat.S_ISDIR(info.st_mode) if root else
            stat.S_ISREG(info.st_mode) and info.st_nlink == 1 and 0 <= info.st_size <= MAX_FILE_BYTES)
    return dict(mode=stat.S_IMODE(info.st_mode), uid=info.st_uid,
                gid=info.st_gid, mtime_ns=info.st_mtime_ns)


def mtime(ns):
    seconds, fraction = divmod(abs(ns), 1_000_000_000)
    return ('-' if ns < 0 else '') + f'{seconds}.{fraction:09d}'


def pax(ns):
    body = ('mtime=' + mtime(ns) + '\n').encode('ascii')
    length = len(body) + 2
    while len(str(length)) + 1 + len(body) != length:
        length = len(str(length)) + 1 + len(body)
    return str(length).encode('ascii') + b' ' + body


def raw_header(name, kind, size, mode=0, uid=0, gid=0):
    value = tarfile.TarInfo(name)
    value.type, value.size, value.mode = kind, size, mode
    value.uid, value.gid, value.mtime = uid, gid, 0
    return value.tobuf(tarfile.USTAR_FORMAT, encoding='utf-8', errors='strict')


def header(output, name, row, size, *, root=False):
    extended = pax(row['mtime_ns'])
    output.write(raw_header('@PaxHeader', tarfile.XHDTYPE, len(extended)))
    output.write(extended + bytes(-len(extended) % 512))
    output.write(raw_header(name, tarfile.DIRTYPE if root else tarfile.REGTYPE,
                            size, row['mode'], row['uid'], row['gid']))


def result(root, files):
    names = {row['name'] for row in files}
    require(REQUIRED <= names <= set(NAMES))
    return dict(format='ortak-cold-journal-archive/v1', root=root, files=files,
                absent=[name for name in OPTIONAL if name not in names],
                content_bytes=sum(row['bytes'] for row in files))


def write(root, stream, uid=10001):
    """Stream exact cold bytes under the caller's held leases; never open SQLite.

    Source ownership/mode, all file identities, directory membership, xattrs and
    parent links must remain unchanged through the final stream flush.
    """
    budget = Budget(stream)
    with ExitStack() as stack:
        fd, links = directory(stack, root)
        original = os.fstat(fd)
        root_row = metadata(original, uid, root=True)
        no_xattrs(fd)
        names = sorted(os.listdir(fd))
        require(REQUIRED <= set(names) <= set(NAMES))
        entries, total = [], 0
        for name in names:
            budget.check()
            child = opened(stack, name, os.O_RDONLY, dir_fd=fd)
            info = os.fstat(child)
            row = metadata(info, uid)
            no_xattrs(child)
            require(name != 'executor.lock' or info.st_size == 0)
            total += info.st_size
            require(total <= MAX_CONTENT_BYTES)
            entries.append((name, child, info, row))
        header(budget, '.', root_row, 0, root=True)
        files = []
        for name, child, info, row in entries:
            header(budget, name, row, info.st_size)
            remaining, digest = info.st_size, hashlib.sha256()
            while remaining:
                budget.check()
                block = os.read(child, min(CHUNK, remaining))
                require(block)
                digest.update(block)
                budget.write(block)
                remaining -= len(block)
            require(os.read(child, 1) == b'')
            budget.write(bytes(-info.st_size % 512))
            files.append(dict(row, name=name, bytes=info.st_size, sha256=digest.hexdigest()))
        budget.write(ZERO + ZERO)
        stream.flush()
        budget.check()
        require(sorted(os.listdir(fd)) == names and stamp(os.fstat(fd)) == stamp(original))
        no_xattrs(fd)
        for name, child, info, _ in entries:
            require(stamp(os.fstat(child)) == stamp(info)
                    and stamp(os.stat(name, dir_fd=fd, follow_symlinks=False)) == stamp(info))
            no_xattrs(child)
        check_links(links)
        budget.check()
        return result(root_row, files)


def member(source):
    """Read only our bounded canonical USTAR + mtime-PAX shape, not arbitrary tar."""
    block = source.exact(512)
    if block == ZERO:
        require(source.exact(512) == ZERO)
        while tail := source.read(CHUNK):
            require(not any(tail))
        return None
    try:
        extension = tarfile.TarInfo.frombuf(block, 'utf-8', 'strict')
        require(extension.type == tarfile.XHDTYPE and 0 < extension.size <= 128
                and block == raw_header('@PaxHeader', tarfile.XHDTYPE, extension.size))
        data = source.exact(extension.size)
        require(not any(source.exact(-extension.size % 512)))
        match = re.fullmatch(rb'[0-9]+ mtime=(-?)([0-9]+)\.([0-9]{9})\n', data)
        require(match is not None)
        ns = (int(match[2]) * 1_000_000_000 + int(match[3])) * (-1 if match[1] else 1)
        require(-(2**63) < ns < 2**63 and pax(ns) == data)
        block = source.exact(512)
        value = tarfile.TarInfo.frombuf(block, 'utf-8', 'strict')
        require(value.type in (tarfile.DIRTYPE, tarfile.REGTYPE)
                and 0 <= value.uid < 2**21 and 0 <= value.gid < 2**21
                and block == raw_header(value.name, value.type, value.size, value.mode, value.uid, value.gid))
        return value, dict(mode=value.mode, uid=value.uid, gid=value.gid, mtime_ns=ns)
    except (tarfile.TarError, UnicodeError, OverflowError) as cause:
        raise Refused('cold_journal_archive_refused') from cause


def attributes(fd, row):
    os.fchmod(fd, row['mode'])
    os.utime(fd, ns=(row['mtime_ns'], row['mtime_ns']))
    os.fsync(fd)


def extract(stream, target, expected_uid=10001):
    """Create a fresh inert host tree and read back every byte before returning.

    Returned UID/GID are original archive metadata, never a claim of chown on
    the host. Actual destination files belong to the current UID. The caller
    must durably pin the returned metadata and archive hash outside this helper.
    """
    source = Budget(stream)
    require(type(expected_uid) is int and 0 <= expected_uid < 2**21)
    first = member(source)
    require(first is not None)
    top, root_row = first
    require(top.name == '.' and top.type == tarfile.DIRTYPE and top.size == 0
            and top.mode == 0o700 and top.uid == expected_uid)
    path = Path(target)
    require(path.name not in ('', '.', '..'))
    with ExitStack() as stack:
        parent, links = directory(stack, path.parent)
        os.mkdir(path.name, 0o700, dir_fd=parent)
        fd = opened(stack, path.name, os.O_RDONLY | os.O_DIRECTORY, dir_fd=parent)
        links.append((parent, path.name, fd, identity(os.fstat(fd))))
        require(os.fstat(fd).st_uid == os.getuid())
        no_xattrs(fd, required=False)
        os.fsync(parent)
        files, saved, total = [], [], 0
        while (item := member(source)) is not None:
            value, row = item
            name = value.name
            require(value.type == tarfile.REGTYPE and name in NAMES and value.mode == 0o600
                    and value.uid == expected_uid and 0 <= value.size <= MAX_FILE_BYTES
                    and (not files or files[-1]['name'] < name)
                    and (name != 'executor.lock' or value.size == 0))
            total += value.size
            require(total <= MAX_CONTENT_BYTES)
            child = opened(stack, name, os.O_RDWR | os.O_CREAT | os.O_EXCL, dir_fd=fd)
            digest, remaining = hashlib.sha256(), value.size
            while remaining:
                block = source.exact(min(CHUNK, remaining))
                digest.update(block)
                view = memoryview(block)
                while view:
                    source.check()
                    count = os.write(child, view)
                    require(count > 0)
                    view = view[count:]
                remaining -= len(block)
            require(not any(source.exact(-value.size % 512)))
            attributes(child, row)
            actual = os.fstat(child)
            metadata(actual, os.getuid())
            no_xattrs(child, required=False)
            require(actual.st_size == value.size and actual.st_mtime_ns == row['mtime_ns'])
            os.lseek(child, 0, os.SEEK_SET)
            verified, count = hashlib.sha256(), 0
            while block := os.read(child, CHUNK):
                source.check()
                count += len(block)
                require(count <= value.size)
                verified.update(block)
            require(count == value.size and verified.digest() == digest.digest()
                    and stamp(os.fstat(child)) == stamp(actual))
            files.append(dict(row, name=name, bytes=value.size, sha256=digest.hexdigest()))
            saved.append((name, child, actual))
        transferred = result(root_row, files)
        attributes(fd, root_row)
        root_info = os.fstat(fd)
        metadata(root_info, os.getuid(), root=True)
        require(root_info.st_mtime_ns == root_row['mtime_ns']
                and sorted(os.listdir(fd)) == [row['name'] for row in files])
        no_xattrs(fd, required=False)
        for name, child, info in saved:
            require(stamp(os.fstat(child)) == stamp(info)
                    and stamp(os.stat(name, dir_fd=fd, follow_symlinks=False)) == stamp(info))
            no_xattrs(child, required=False)
        check_links(links)
        source.check()
        return transferred
