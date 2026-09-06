"""Bounded recovery payload primitives; real secret reads require the caller's held barrier."""

import hashlib
import io
import json
import os
from pathlib import Path, PurePosixPath
import sqlite3
import stat
import tarfile
import time

from cryptography.hazmat.primitives.ciphers.aead import AESGCM

from backup_private_database import Refused, digest, private_binary, private_directory
from prepare_private_recovery import canonical, sha
import private_recovery_inventory as inventory

MAX_FILES = 100000
SECRET_LIMIT = 32 * 1024**2
MAGIC = b'ORTAKA01'
READER_CODES = frozenset(('unreviewed_xattr', 'xattr_bound', 'file_count_bound', 'deadline',
    'entry_type', 'file_link_count', 'byte_bound', 'opened_generation_changed',
    'final_generation_changed', 'xattr_generation_changed', 'os_error', 'unexpected_failure'))
READER_PHASES = frozenset(('initialization', 'xattr_names', 'xattr_value',
    'directory_entries', 'entry_metadata', 'file_read'))


def volume_reader_failure(path, kind):
    """Read only a fixed reader code/phase; rejected stderr text never enters a manifest."""
    inventory.require(kind in ('redis', 'minio'), 'volume_reader_kind_refused')
    try:
        raw, _ = read_regular(path, 256)
        marker, code, phase = raw.decode('ascii').strip().split(':')
        if marker == 'ORTAK_VOLUME_READER' and code in READER_CODES and phase in READER_PHASES:
            return {'kind': kind, 'code': code, 'phase': phase}
    except (OSError, ValueError, UnicodeError, Refused):
        pass
    return None


def safe_name(value):
    """Archive members must be relative ordinary paths with bounded components."""
    path = PurePosixPath(value)
    inventory.require(isinstance(value, str) and len(value) <= 4096 and not path.is_absolute()
        and path.parts and len(path.parts) <= 64 and '..' not in path.parts
        and '\\' not in value and '\0' not in value, 'archive_member_refused')
    return str(path)


def fingerprint(row):
    """Metadata used only to catch a source file changing while it is being read."""
    return row.st_dev, row.st_ino, row.st_size, row.st_mtime_ns, row.st_uid, row.st_mode


def read_regular(path, maximum, *, secret=False):
    """Never follow a link or accept a changing/oversized selected file."""
    before = path.lstat()
    inventory.require(stat.S_ISREG(before.st_mode) and before.st_nlink == 1
        and before.st_uid == os.getuid() and before.st_size <= maximum, 'capture_file_refused')
    if secret:
        inventory.require(stat.S_IMODE(before.st_mode) in (0o600, 0o444), 'secret_file_mode_refused')
    descriptor = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
    with os.fdopen(descriptor, 'rb') as stream:
        opened = os.fstat(stream.fileno())
        raw = stream.read(maximum + 1)
    inventory.require(len(raw) <= maximum and fingerprint(before) == fingerprint(opened)
        == fingerprint(path.lstat()), 'capture_file_changed')
    return raw, before


def copy_file(source, target, limit):
    """Stream a new private artifact; callers pass only exact selected source paths."""
    before = source.lstat()
    inventory.require(stat.S_ISREG(before.st_mode) and before.st_nlink == 1
        and before.st_uid == os.getuid() and before.st_size <= limit, 'copy_source_refused')
    descriptor = os.open(source, os.O_RDONLY | os.O_NOFOLLOW)
    size = 0
    with os.fdopen(descriptor, 'rb') as incoming, private_binary(target) as outgoing:
        inventory.require(fingerprint(os.fstat(incoming.fileno())) == fingerprint(before), 'copy_source_changed')
        while block := incoming.read(65536):
            size += len(block)
            inventory.require(size <= limit, 'copy_limit_exceeded')
            outgoing.write(block)
        outgoing.flush(); os.fsync(outgoing.fileno())
    inventory.require(fingerprint(source.lstat()) == fingerprint(before) and size == before.st_size,
                      'copy_source_changed')
    return {'path': target.name, 'bytes': size, 'sha256': digest(target)}


def archive_files(target, entries, limit):
    """Archive an already-expanded exact allowlist; no links, devices or implicit recursion."""
    inventory.require(len(entries) <= MAX_FILES, 'archive_file_count_refused')
    names = [safe_name(name) for _, name in entries]
    inventory.require(len(names) == len(set(names)), 'duplicate_archive_member')
    total = 0
    with private_binary(target) as outgoing, tarfile.open(fileobj=outgoing, mode='w|') as archive:
        for (path, _), name in zip(entries, names):
            before = path.lstat()
            directory = stat.S_ISDIR(before.st_mode)
            inventory.require((directory or (stat.S_ISREG(before.st_mode) and before.st_nlink == 1))
                and before.st_uid == os.getuid(), 'archive_source_refused')
            total += 0 if directory else before.st_size
            inventory.require(total <= limit, 'archive_bytes_refused')
            if directory:
                info = tarfile.TarInfo(name); info.type = tarfile.DIRTYPE
                info.mode, info.mtime = stat.S_IMODE(before.st_mode), int(before.st_mtime)
                info.uid, info.gid = before.st_uid, before.st_gid
                archive.addfile(info)
                continue
            descriptor = os.open(path, os.O_RDONLY | os.O_NOFOLLOW)
            with os.fdopen(descriptor, 'rb') as incoming:
                inventory.require(fingerprint(os.fstat(incoming.fileno())) == fingerprint(before), 'archive_source_changed')
                info = tarfile.TarInfo(name)
                info.size, info.mode, info.mtime = before.st_size, stat.S_IMODE(before.st_mode), int(before.st_mtime)
                info.uid, info.gid = before.st_uid, before.st_gid
                archive.addfile(info, incoming)
            inventory.require(fingerprint(path.lstat()) == fingerprint(before), 'archive_source_changed')
        # tarfile writes its trailer on close, before the outer fsync below.
    with target.open('rb') as stream: os.fsync(stream.fileno())
    inventory.require(target.stat().st_size <= limit + MAX_FILES * 2048, 'archive_overhead_refused')
    return {'path': target.name, 'bytes': target.stat().st_size, 'sha256': digest(target), 'files': len(entries)}


def tree_entries(root, prefix, limit):
    """Expand only one explicitly selected tree, refusing links and bounding traversal."""
    inventory.directory(root)
    entries, pending, total, visited = [(root, safe_name(prefix))], [root], 0, 0
    while pending:
        directory = pending.pop()
        for child in sorted(directory.iterdir()):
            visited += 1
            inventory.require(visited <= MAX_FILES, 'tree_count_refused')
            row = child.lstat()
            inventory.require(row.st_uid == os.getuid(), 'tree_owner_refused')
            if stat.S_ISDIR(row.st_mode):
                pending.append(child)
                entries.append((child, safe_name(prefix + '/' + str(child.relative_to(root)))))
            else:
                inventory.require(stat.S_ISREG(row.st_mode) and row.st_nlink == 1, 'tree_file_type_refused')
                total += row.st_size
                inventory.require(total <= limit, 'tree_bytes_refused')
                entries.append((child, safe_name(prefix + '/' + str(child.relative_to(root)))))
    return entries


def sqlite_backup(source, target, limit=64 * 1024**2, *, cold=False):
    """Use the backup API; a held cold capture stages working metadata without source writes."""
    if cold:
        from recovery_lock_holder import cold_journal_file
        with cold_journal_file(source) as working:
            return sqlite_backup_file(working, target, limit, working_metadata=True)
    return sqlite_backup_file(source, target, limit)


def sqlite_backup_file(source, target, limit, working_metadata=False):
    """Only a disposable cold working copy opens RW; every SQL operation remains query-only."""
    row = source.lstat()
    inventory.require(stat.S_ISREG(row.st_mode) and row.st_nlink == 1 and row.st_uid == os.getuid()
                      and stat.S_IMODE(row.st_mode) == 0o600 and row.st_size <= limit, 'sqlite_source_refused')
    with private_binary(target): pass
    source_db = sqlite3.connect(source.as_uri() + ('?mode=rw' if working_metadata else '?mode=ro'), uri=True, timeout=2)
    destination = sqlite3.connect(target, timeout=2)
    deadline = time.monotonic() + 20
    def progress(status, remaining, total):
        inventory.require(time.monotonic() < deadline and total * page_size <= limit, 'sqlite_backup_bound')
    try:
        source_db.execute('PRAGMA query_only=ON')
        page_size = source_db.execute('PRAGMA page_size').fetchone()[0]
        pages = source_db.execute('PRAGMA page_count').fetchone()[0]
        inventory.require(page_size * pages <= limit, 'sqlite_backup_size_refused')
        source_db.backup(destination, pages=64, progress=progress, sleep=0.01)
        inventory.require(destination.execute('PRAGMA integrity_check').fetchall() == [('ok',)], 'sqlite_integrity_failed')
    finally:
        source_db.close(); destination.close()
    with target.open('rb') as stream: os.fsync(stream.fileno())
    inventory.require(target.stat().st_size <= limit, 'sqlite_backup_size_refused')
    return {'path': target.name, 'bytes': target.stat().st_size, 'sha256': digest(target), 'integrity': 'ok'}


def secret_envelope(target, key_path, metadata, aad, extras=None):
    """Encrypt an exact bounded in-memory archive; no plaintext secret archive is written."""
    inventory.directory(target.parent)
    inventory.directory(key_path.parent)
    inventory.require(not key_path.is_relative_to(target.parent), 'recovery_key_must_be_separate')
    buffer = io.BytesIO()
    seen = set()
    with tarfile.open(fileobj=buffer, mode='w') as archive:
        for record in metadata:
            path = Path(record['path'])
            root = next((root for root, names in inventory.SECRET_FILES.items()
                         if path.is_relative_to(root) and str(path.relative_to(root)) in names), None)
            inventory.require(root is not None and inventory.file_metadata(root, str(path.relative_to(root)), service_readable=True) == record,
                              'secret_scope_or_generation_changed')
            name = safe_name('selected/' + str(path).lstrip('/'))
            inventory.require(name not in seen, 'duplicate_secret_member')
            seen.add(name)
            raw, row = read_regular(path, 1024**2, secret=True)
            info = tarfile.TarInfo(name)
            info.size, info.mode = len(raw), stat.S_IMODE(row.st_mode)
            archive.addfile(info, io.BytesIO(raw))
            inventory.require(buffer.tell() <= SECRET_LIMIT, 'secret_archive_bound')
            inventory.require(inventory.file_metadata(root, str(path.relative_to(root)), service_readable=True) == record,
                              'secret_generation_changed')
        for name, raw in (extras or {}).items():
            name = safe_name('metadata/' + name)
            inventory.require(name not in seen and len(raw) <= 1024**2, 'secret_extra_refused')
            seen.add(name)
            info = tarfile.TarInfo(name); info.size = len(raw); info.mode = 0o600
            archive.addfile(info, io.BytesIO(raw))
            inventory.require(buffer.tell() <= SECRET_LIMIT, 'secret_archive_bound')
    plaintext = buffer.getvalue()
    inventory.require(len(plaintext) <= SECRET_LIMIT, 'secret_archive_bound')
    key, nonce = AESGCM.generate_key(bit_length=256), os.urandom(12)
    with private_binary(key_path) as stream:
        stream.write(key); stream.flush(); os.fsync(stream.fileno())
    directory = os.open(key_path.parent, os.O_RDONLY)
    try:
        os.fsync(directory)
    finally:
        os.close(directory)
    ciphertext = AESGCM(key).encrypt(nonce, plaintext, canonical(aad))
    with private_binary(target) as stream:
        stream.write(MAGIC + nonce + ciphertext); stream.flush(); os.fsync(stream.fileno())
    # Authentication/decryption is checked in memory; material remains unusable
    # by any restored process. Actual offline destination restoration is separate.
    inventory.require(AESGCM(key).decrypt(nonce, ciphertext, canonical(aad)) == plaintext, 'secret_envelope_self_check_failed')
    return {'path': target.name, 'bytes': target.stat().st_size, 'sha256': digest(target),
            'format': 'AES-256-GCM/1', 'key_reference': str(key_path), 'aad': aad,
            'members': sorted(seen), 'authenticated_round_trip': True, 'offline_restore_executed': False}


# Executed only in an image-pinned, no-network, read-only cold volume reader.
# It emits tar bytes only. No model/controller/provider module is imported.
VOLUME_READER = r"""
import base64,os,stat,sys,tarfile,time
from pathlib import Path
phase='initialization'
class ReaderFailure(Exception): pass
def require(value,code):
 if not value: raise ReaderFailure(code)
try:
 root=Path('/capture-source'); limit=int(sys.argv[1]); deadline=time.monotonic()+120
 def metadata(path):
  global phase
  phase='xattr_names'
  names=os.listxattr(path,follow_symlinks=False)
  require(not set(names)-{'user.total_writes','user.total_deletes'},'unreviewed_xattr')
  result={}
  for key in sorted(names):
   phase='xattr_value'
   value=os.getxattr(path,key,follow_symlinks=False)
   require(len(value)==8,'xattr_bound')
   result['ORTAK.xattr.'+key]=base64.b64encode(value).decode()
  return result
 def info_for(archive,path,name):
  info=archive.gettarinfo(str(path),name);info.pax_headers.update(metadata(path));return info
 pending=[root]; count=0; size=0
 with tarfile.open(fileobj=sys.stdout.buffer,mode='w|') as archive:
  archive.addfile(info_for(archive,root,'.'))
  while pending:
   directory=pending.pop()
   phase='directory_entries'
   for child in sorted(directory.iterdir()):
    count+=1
    require(count<=100000,'file_count_bound')
    require(time.monotonic()<=deadline,'deadline')
    phase='entry_metadata'
    row=child.lstat()
    selected_metadata=metadata(child)
    if stat.S_ISDIR(row.st_mode):
     archive.addfile(info_for(archive,child,str(child.relative_to(root))))
     pending.append(child); continue
    require(stat.S_ISREG(row.st_mode),'entry_type')
    require(row.st_nlink==1,'file_link_count')
    size+=row.st_size
    require(size<=limit,'byte_bound')
    info=info_for(archive,child,str(child.relative_to(root)))
    phase='file_read'
    fd=os.open(child,os.O_RDONLY|os.O_NOFOLLOW)
    with os.fdopen(fd,'rb') as incoming:
     opened=os.fstat(incoming.fileno())
     require((row.st_ino,row.st_size,row.st_mtime_ns)==(opened.st_ino,opened.st_size,opened.st_mtime_ns),'opened_generation_changed')
     archive.addfile(info,incoming)
    after=child.lstat()
    require((row.st_ino,row.st_size,row.st_mtime_ns)==(after.st_ino,after.st_size,after.st_mtime_ns),'final_generation_changed')
    require(metadata(child)==selected_metadata,'xattr_generation_changed')
except Exception as error:
 code=str(error) if isinstance(error,ReaderFailure) else 'os_error' if isinstance(error,OSError) else 'unexpected_failure'
 sys.stderr.write('ORTAK_VOLUME_READER:'+code+':'+phase+'\n')
 sys.exit(3)
"""
