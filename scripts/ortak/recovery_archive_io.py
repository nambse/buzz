"""Standalone bounded tar inspection/extraction for a fresh offline destination only."""

from decimal import Decimal
import base64
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import stat
import sys
import tarfile

MAX_FILES = 100000
XATTR = 'user.total_writes'
PAX_XATTR = 'ORTAK.xattr.' + XATTR
XATTRS = (XATTR, 'user.total_deletes')
PAX_XATTRS = {'ORTAK.xattr.' + key: key for key in XATTRS}


def xattrs(path):
    """Preserve MinIO's exact eight-byte write/delete counters; unknown metadata refuses."""
    names = os.listxattr(path, follow_symlinks=False)
    require(set(names) <= set(XATTRS))
    result = {}
    for key in sorted(names):
        value = os.getxattr(path, key, follow_symlinks=False)
        require(len(value) == 8)
        result[key] = base64.b64encode(value).decode()
    return result


def archive_xattrs(headers):
    """Only the two reviewed exact-width PAX attributes grant restored xattr bytes."""
    result = {}
    for header, key in PAX_XATTRS.items():
        if header not in headers: continue
        value = headers[header]
        require(isinstance(value, str) and len(value) == 12)
        raw = base64.b64decode(value, validate=True)
        require(len(raw) == 8 and base64.b64encode(raw).decode() == value)
        result[key] = value
    return result


def require(value):
    """Never include rejected archive names or private content in a diagnostic."""
    if not value: raise ValueError('offline_archive_refused')


def name(value, directory=False):
    """Reject traversal, aliases, control characters and links before filesystem access."""
    require(isinstance(value, str) and 0 < len(value) <= 4096)
    path = PurePosixPath(value)
    require(not path.is_absolute() and '..' not in path.parts and len(path.parts) <= 64
        and '\\' not in value and all(ord(c) >= 32 for c in value)
        and str(path) == value and (value != '.' or directory))
    return value


def summary(rows):
    """A deterministic digest includes all names, modes, owners, timestamps and file bytes."""
    encoded = json.dumps(sorted(rows, key=lambda row: row['name']), sort_keys=True, separators=(',', ':')).encode()
    return {'files': sum(row['type'] == 'file' for row in rows),
            'directories': sum(row['type'] == 'directory' for row in rows),
            'bytes': sum(row['bytes'] for row in rows),
            'tree_sha256': hashlib.sha256(encoded).hexdigest()}


def archive(stream, maximum, target=None, *, ownership=False, expected_names=None):
    """Inspect or extract into one already-created empty private root, never overwrite a member."""
    require(type(maximum) is int and 0 < maximum <= 8 * 1024**3)
    if target is not None:
        row = target.lstat()
        require(target.resolve() == target and stat.S_ISDIR(row.st_mode) and row.st_uid == os.getuid()
            and stat.S_IMODE(row.st_mode) == 0o700 and not any(target.iterdir()))
    rows, seen, directories, total = [], set(), [], 0
    with tarfile.open(fileobj=stream, mode='r|') as source:
        for member in source:
            require(len(rows) < MAX_FILES and (member.isdir() or member.type in (tarfile.REGTYPE, tarfile.AREGTYPE)))
            member_name = name(member.name, member.isdir())
            require(member_name not in seen and not member.mode & ~0o777 and member.size >= 0
                and 0 <= member.uid < 2**32 and 0 <= member.gid < 2**32
                and set(member.pax_headers) <= {'mtime', 'atime', 'ctime', 'path'} | set(PAX_XATTRS))
            seen.add(member_name)
            require(not member.isdir() or member.size == 0)
            total += member.size
            require(total <= maximum)
            mtime = int(Decimal(member.pax_headers.get('mtime', str(member.mtime))) * 1000000000)
            require(-(2**63) < mtime < 2**63)
            record = {'name': member_name, 'type': 'directory' if member.isdir() else 'file',
                      'bytes': member.size, 'mode': member.mode, 'uid': member.uid, 'gid': member.gid,
                      'mtime_ns': mtime}
            metadata = archive_xattrs(member.pax_headers)
            if metadata: record['xattrs'] = metadata
            path = target / member_name if target is not None else None
            if path is not None:
                for parent in reversed(path.parents):
                    if parent == target or target in parent.parents:
                        if not parent.exists(): parent.mkdir(mode=0o700)
                        require(stat.S_ISDIR(parent.lstat().st_mode) and not parent.is_symlink())
            if member.isdir():
                if path is not None:
                    if path != target: path.mkdir(mode=0o700, exist_ok=True)
                    require(stat.S_ISDIR(path.lstat().st_mode) and not path.is_symlink())
                    directories.append((path, record))
            else:
                content = source.extractfile(member)
                require(content is not None)
                descriptor = None
                try:
                    if path is not None:
                        descriptor = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600)
                    count, digest = 0, hashlib.sha256()
                    while block := content.read(65536):
                        count += len(block); require(count <= member.size)
                        digest.update(block)
                        if descriptor is not None:
                            view = memoryview(block)
                            while view:
                                written = os.write(descriptor, view); require(written > 0)
                                view = view[written:]
                    require(count == member.size)
                    record['sha256'] = digest.hexdigest()
                    if descriptor is not None: os.fsync(descriptor)
                finally:
                    if descriptor is not None: os.close(descriptor)
                if path is not None: attributes(path, record, ownership)
            rows.append(record)
    require(rows and (expected_names is None or seen == set(expected_names)))
    for path, record in sorted(directories, key=lambda pair: len(pair[0].parts), reverse=True):
        attributes(path, record, ownership)
    if target is not None:
        descriptor = os.open(target, os.O_RDONLY)
        try: os.fsync(descriptor)
        finally: os.close(descriptor)
    return summary(rows)


def attributes(path, row, ownership):
    """Apply and fsync metadata on each new file/directory before any restrictive mode change."""
    descriptor = os.open(path, os.O_RDONLY | os.O_NOFOLLOW)
    try:
        # Host-side inert/secret extraction may run on Python builds without
        # xattr APIs. Such a host cannot accept archived xattrs or Linux owner
        # restoration; the actual volume extractor always requires both.
        require(hasattr(os, 'listxattr') or (not ownership and not row.get('xattrs')))
        if hasattr(os, 'listxattr'): require(not os.listxattr(path, follow_symlinks=False))
        for key, value in row.get('xattrs', {}).items():
            raw = base64.b64decode(value, validate=True)
            require(key in XATTRS and len(raw) == 8)
            os.setxattr(path, key, raw, follow_symlinks=False)
        if ownership: os.chown(path, row['uid'], row['gid'], follow_symlinks=False)
        os.chmod(path, row['mode'], follow_symlinks=False)
        os.utime(path, ns=(row['mtime_ns'], row['mtime_ns']), follow_symlinks=False)
        os.fsync(descriptor)
    finally:
        os.close(descriptor)


def tree(root, maximum):
    """Re-read the complete newly restored Linux tree, including root and empty directories."""
    require(root.resolve() == root)
    pending, rows, total = [root], [], 0
    while pending:
        path = pending.pop()
        data = path.lstat()
        require(len(rows) < MAX_FILES
            and not stat.S_IMODE(data.st_mode) & ~0o777)
        directory = stat.S_ISDIR(data.st_mode)
        require(directory or (stat.S_ISREG(data.st_mode) and data.st_nlink == 1))
        record = {'name': str(path.relative_to(root)), 'type': 'directory' if directory else 'file',
                  'bytes': 0 if directory else data.st_size, 'mode': stat.S_IMODE(data.st_mode),
                  'uid': data.st_uid, 'gid': data.st_gid, 'mtime_ns': data.st_mtime_ns}
        metadata = xattrs(path)
        if metadata: record['xattrs'] = metadata
        if directory:
            pending.extend(sorted(path.iterdir(), reverse=True))
        else:
            total += data.st_size; require(total <= maximum)
            digest = hashlib.sha256()
            descriptor = os.open(path, os.O_RDONLY | os.O_NOFOLLOW)
            with os.fdopen(descriptor, 'rb') as stream:
                opened = os.fstat(stream.fileno())
                require((data.st_ino, data.st_size, data.st_mtime_ns) == (opened.st_ino, opened.st_size, opened.st_mtime_ns))
                count = 0
                while block := stream.read(65536):
                    count += len(block); require(count <= data.st_size); digest.update(block)
            require(count == data.st_size)
            record['sha256'] = digest.hexdigest()
        rows.append(record)
    return summary(rows)


if __name__ == '__main__':
    try:
        require(sys.platform == 'linux' and not Path('/var/run/docker.sock').exists())
        destination = Path('/restore-target')
        require(destination.resolve() == destination and destination.is_dir() and not any(destination.iterdir()))
        destination.chmod(0o700)
        result = archive(sys.stdin.buffer, int(sys.argv[1]), destination, ownership=True)
        require(tree(destination, int(sys.argv[1])) == result)
        print(json.dumps({'status': 'verified', **result}))
    except Exception:
        print('{"status":"refused"}')
        raise SystemExit(3) from None
