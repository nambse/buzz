"""Cold preservation of the single explicitly selected native ciphertext store.

No SQLite, key, draft decoder, replay or ordinary cache is opened. A surviving
DELETE journal is preserved as opaque evidence, never declared recovered. The
caller owns a hard process watchdog, an exclusive private archive file, durable
failure receipts, and exact stopped-native identity verification. The callback
must recheck that selected owner and return its pinned receipt SHA-256. These
checks do not themselves discover or contain a process.

Returned metadata plus the archive hash must be pinned outside the archive.
Extraction is into a fresh inert directory outside the original app_data tree;
failures retain partial output. Successful bytes preserve encrypted versions,
operation IDs, both frozen1059 copies and ACK bits without interpreting them.
"""

from contextlib import ExitStack
from functools import wraps
import hashlib
import json
import os
from pathlib import Path
import re
import stat
import tarfile

import recovery_journal_archive as wire

DIRECTORY = 'ortak-encrypted-dm-v1'
DATABASE = 'ciphertext.sqlite'
JOURNAL = DATABASE + '-journal'
NAMES = (DATABASE, JOURNAL)
MAX_CONTENT_BYTES = 12 * 1024**2
MAX_ARCHIVE_BYTES = MAX_CONTENT_BYTES + 16384
MAX_MANIFEST_BYTES = 4096
MANIFEST = 'native-confidential.json'
FORMAT = 'ortak-native-confidential-recovery/1'


class Refused(ValueError):
    """Closed failure; the rejected path and ciphertext are not diagnostics."""


def require(value):
    if not value:
        raise Refused('native_confidential_recovery_refused')


def closed(function):
    @wraps(function)
    def call(*args, **kwargs):
        try:
            return function(*args, **kwargs)
        except (OSError, wire.Refused, KeyError, TypeError, UnicodeError):
            raise Refused('native_confidential_recovery_refused') from None
    return call


def canonical(value):
    return json.dumps(value, sort_keys=True, separators=(',', ':'), ensure_ascii=True).encode('ascii')


def absolute(value):
    value = os.fspath(value)
    require(isinstance(value, str) and 1 < len(value) <= 4096
            and str(Path(value)) == value and value.startswith('/')
            and len(Path(value).parts) <= 64 and '..' not in Path(value).parts
            and all(ord(c) >= 32 for c in value) and '\\' not in value)
    return Path(value)


def digest(value):
    return hashlib.sha256(value).hexdigest()


def sha(value):
    return isinstance(value, str) and re.fullmatch('[0-9a-f]{64}', value) is not None


class Budget(wire.Budget):
    def __init__(self, stream):
        super().__init__(stream)
        self.digest = hashlib.sha256()

    def check(self):
        super().check()
        require(self.count <= MAX_ARCHIVE_BYTES)

    def read(self, size):
        value = super().read(size)
        self.digest.update(value)
        return value

    def write(self, value):
        require(self.count + len(value) <= MAX_ARCHIVE_BYTES)
        super().write(value)
        self.digest.update(value)


def directory(stack, path):
    fd, links = wire.directory(stack, absolute(path))
    for _, _, child, _ in links:
        row = os.fstat(child)
        require(row.st_uid in (0, os.getuid())
                and (not row.st_mode & 0o022 or row.st_uid == 0 and row.st_mode & stat.S_ISVTX))
    return fd, links


def names(fd, *, required=True):
    values = []
    with os.scandir(fd) as entries:
        for entry in entries:
            require(len(values) < 2 and entry.name in NAMES)
            values.append(entry.name)
    require(not required or DATABASE in values)
    return sorted(values)


def absent(parent):
    try:
        os.stat(DIRECTORY, dir_fd=parent, follow_symlinks=False)
    except FileNotFoundError:
        return True
    return False


def file_digest(fd, size, budget, output=None):
    os.lseek(fd, 0, os.SEEK_SET)
    result, count = hashlib.sha256(), 0
    while True:
        budget.check()
        block = os.read(fd, min(wire.CHUNK, size - count + 1))
        if not block:
            break
        count += len(block)
        require(count <= size)
        result.update(block)
        if output is not None:
            output.write(block)
    require(count == size)
    return result.hexdigest()


def metadata(app_data, root, files):
    return dict(format=FORMAT, selection=DIRECTORY,
                app_data_sha256=digest(str(app_data).encode()), owner_uid=os.getuid(),
                state='absent' if root is None else 'cold_ciphertext', root=root,
                files=files, rollback_journal_present=any(r['name'] == JOURNAL for r in files),
                content_bytes=sum(r['bytes'] for r in files))


def validate_row(row, *, root=False):
    keys = {'mode', 'uid', 'gid', 'mtime_ns'}
    require(isinstance(row, dict) and set(row) == (keys if root else keys | {'name', 'bytes', 'sha256'})
            and all(type(row[k]) is int for k in keys)
            and row['mode'] == (0o700 if root else 0o600) and row['uid'] == os.getuid()
            and 0 <= row['gid'] < 2**21 and -(2**63) < row['mtime_ns'] < 2**63)
    if not root:
        require(row['name'] in NAMES and type(row['bytes']) is int
                and 0 <= row['bytes'] <= MAX_CONTENT_BYTES and sha(row['sha256']))


@closed
def write(app_data, stream, *, stopped_native, expected_owner_sha256):
    """Archive exact cold bytes or explicit absence, checking stopped ownership twice.

    The supplied callback is trusted orchestration, not an authentication token
    accepted from a client. Returning a different hash or a false result refuses.
    """
    require(sha(expected_owner_sha256) and callable(stopped_native))
    require(stopped_native() == expected_owner_sha256)
    app_data = absolute(app_data)
    budget = Budget(stream)
    with ExitStack() as stack:
        parent, links = directory(stack, app_data)
        require(os.fstat(parent).st_uid == os.getuid())
        missing = absent(parent)
        files, saved, root, original = [], [], None, None
        if not missing:
            fd = wire.opened(stack, DIRECTORY, os.O_RDONLY | os.O_DIRECTORY, dir_fd=parent)
            original = os.fstat(fd)
            root = wire.metadata(original, os.getuid(), root=True)
            wire.no_xattrs(fd, required=False)
            links.append((parent, DIRECTORY, fd, wire.identity(original)))
            selected, total = names(fd), 0
            for name in selected:
                child = wire.opened(stack, name, os.O_RDONLY, dir_fd=fd)
                info = os.fstat(child)
                row = wire.metadata(info, os.getuid())
                wire.no_xattrs(child, required=False)
                total += info.st_size
                require(total <= MAX_CONTENT_BYTES)
                files.append(dict(row, name=name, bytes=info.st_size,
                                  sha256=file_digest(child, info.st_size, budget)))
                saved.append((name, child, info))
        transferred = metadata(app_data, root, files)
        raw = canonical(transferred)
        require(len(raw) <= MAX_MANIFEST_BYTES)
        virtual = dict(mode=0o600, uid=os.getuid(), gid=os.getgid(), mtime_ns=0)
        wire.header(budget, MANIFEST, virtual, len(raw))
        budget.write(raw + bytes(-len(raw) % 512))
        if not missing:
            wire.header(budget, '.', root, 0, root=True)
            for (name, child, info), row in zip(saved, files, strict=True):
                wire.header(budget, name, row, info.st_size)
                require(file_digest(child, info.st_size, budget, budget) == row['sha256'])
                budget.write(bytes(-info.st_size % 512))
        budget.write(wire.ZERO * 2)
        stream.flush()
        budget.check()
        require(stopped_native() == expected_owner_sha256)
        require(absent(parent) == missing)
        if not missing:
            require(names(fd) == selected and wire.stamp(os.fstat(fd)) == wire.stamp(original))
            wire.no_xattrs(fd, required=False)
            for name, child, info in saved:
                require(wire.stamp(os.fstat(child)) == wire.stamp(info)
                        and wire.stamp(os.stat(name, dir_fd=fd, follow_symlinks=False)) == wire.stamp(info))
                wire.no_xattrs(child, required=False)
        wire.check_links(links)
        budget.check()
        return dict(store=transferred, archive=dict(bytes=budget.count, sha256=budget.digest.hexdigest()),
                    stopped_native_sha256=expected_owner_sha256)


def read_manifest(source, expected, app_data):
    require(isinstance(expected, dict) and set(expected) == {'store', 'archive', 'stopped_native_sha256'}
            and isinstance(expected['archive'], dict) and set(expected['archive']) == {'bytes', 'sha256'}
            and sha(expected['stopped_native_sha256']) and sha(expected['archive']['sha256'])
            and type(expected['archive']['bytes']) is int
            and 0 < expected['archive']['bytes'] <= MAX_ARCHIVE_BYTES)
    item = wire.member(source)
    require(item is not None)
    entry, row = item
    require(entry.name == MANIFEST and entry.type == tarfile.REGTYPE
            and entry.mode == 0o600 and entry.uid == os.getuid()
            and row['mtime_ns'] == 0 and 0 < entry.size <= MAX_MANIFEST_BYTES)
    store = expected['store']
    require(isinstance(store, dict) and set(store) == {'format', 'selection', 'app_data_sha256', 'owner_uid', 'state', 'root',
                           'files', 'rollback_journal_present', 'content_bytes'}
            and store['format'] == FORMAT and store['selection'] == DIRECTORY
            and store['app_data_sha256'] == digest(str(app_data).encode())
            and type(store['owner_uid']) is int and store['owner_uid'] == os.getuid()
            and isinstance(store['files'], list) and len(store['files']) <= 2
            and type(store['content_bytes']) is int and 0 <= store['content_bytes'] <= MAX_CONTENT_BYTES)
    if store['state'] == 'absent':
        require(store['root'] is None and store['files'] == []
                and store['rollback_journal_present'] is False and store['content_bytes'] == 0)
    else:
        require(store['state'] == 'cold_ciphertext' and isinstance(store['root'], dict))
        validate_row(store['root'], root=True)
        require(store['files'])
        for row in store['files']:
            validate_row(row)
        require([r['name'] for r in store['files']] in ([DATABASE], list(NAMES))
                and store['rollback_journal_present'] is (len(store['files']) == 2)
                and store['content_bytes'] == sum(r['bytes'] for r in store['files']))
    raw = source.exact(entry.size)
    require(not any(source.exact(-entry.size % 512)))
    expected_raw = canonical(store)
    require(len(expected_raw) <= MAX_MANIFEST_BYTES and raw == expected_raw)
    return store


@closed
def extract(stream, target, expected, *, app_data):
    """Verify externally pinned archive and read back a fresh same-UID inert copy.

    No original path restoration, SQLite open, decryption or send is performed.
    Absence creates an empty evidence directory, not a replacement native store.
    """
    app_data, target = absolute(app_data), absolute(target)
    require(target != app_data and target not in app_data.parents and app_data not in target.parents)
    source = Budget(stream)
    store = read_manifest(source, expected, app_data)
    with ExitStack() as stack:
        parent, links = directory(stack, target.parent)
        require(os.fstat(parent).st_uid == os.getuid())
        os.mkdir(target.name, 0o700, dir_fd=parent)
        fd = wire.opened(stack, target.name, os.O_RDONLY | os.O_DIRECTORY, dir_fd=parent)
        links.append((parent, target.name, fd, wire.identity(os.fstat(fd))))
        wire.metadata(os.fstat(fd), os.getuid(), root=True)
        wire.no_xattrs(fd, required=False)
        os.fsync(parent)
        actual, saved = [], []
        if store['state'] == 'cold_ciphertext':
            item = wire.member(source)
            require(item is not None)
            top, root = item
            require(top.name == '.' and top.type == tarfile.DIRTYPE and top.size == 0
                    and top.mode == 0o700 and top.uid == os.getuid() and root == store['root'])
            for expected_file in store['files']:
                item = wire.member(source)
                require(item is not None)
                entry, row = item
                name = entry.name
                require(name in NAMES and entry.type == tarfile.REGTYPE and entry.mode == 0o600
                        and entry.uid == os.getuid() and 0 <= entry.size <= MAX_CONTENT_BYTES
                        and (not actual or actual[-1]['name'] < name)
                        and sum(r['bytes'] for r in actual) + entry.size <= MAX_CONTENT_BYTES)
                child = wire.opened(stack, name, os.O_RDWR | os.O_CREAT | os.O_EXCL, dir_fd=fd)
                remaining, hashed = entry.size, hashlib.sha256()
                while remaining:
                    block = source.exact(min(wire.CHUNK, remaining))
                    hashed.update(block)
                    view = memoryview(block)
                    while view:
                        source.check()
                        count = os.write(child, view)
                        require(count > 0)
                        view = view[count:]
                    remaining -= len(block)
                require(not any(source.exact(-entry.size % 512)))
                transferred = dict(row, name=name, bytes=entry.size, sha256=hashed.hexdigest())
                require(transferred == expected_file)
                wire.attributes(child, row)
                info = os.fstat(child)
                wire.metadata(info, os.getuid())
                wire.no_xattrs(child, required=False)
                require(info.st_size == entry.size and info.st_mtime_ns == row['mtime_ns']
                        and info.st_gid == row['gid']
                        and file_digest(child, entry.size, source) == hashed.hexdigest()
                        and wire.stamp(os.fstat(child)) == wire.stamp(info))
                actual.append(transferred)
                saved.append((name, child, info))
            require(actual and actual[0]['name'] == DATABASE)
            require(store['rollback_journal_present'] is (len(actual) == 2)
                    and store['content_bytes'] == sum(r['bytes'] for r in actual))
            wire.attributes(fd, root)
        require(wire.member(source) is None and source.count == expected['archive']['bytes']
                and source.digest.hexdigest() == expected['archive']['sha256'])
        require(names(fd, required=bool(actual)) == [r['name'] for r in actual])
        # Parent traversal and each destination entry remain bound to the exact
        # descriptors created above; a matching replacement inode is refused.
        root_info = os.fstat(fd)
        wire.metadata(root_info, os.getuid(), root=True)
        if actual:
            require(root_info.st_gid == store['root']['gid']
                    and root_info.st_mtime_ns == store['root']['mtime_ns'])
        wire.no_xattrs(fd, required=False)
        for name, child, info in saved:
            require(wire.stamp(os.fstat(child)) == wire.stamp(info)
                    and wire.stamp(os.stat(name, dir_fd=fd, follow_symlinks=False)) == wire.stamp(info))
            wire.no_xattrs(child, required=False)
        wire.check_links(links)
        os.fsync(fd)
        os.fsync(parent)
        source.check()
        return expected
