"""Exact schema78 native client artifact/ingress evidence; no app startup, stop or profile access."""

import base64
import hashlib
import json
import os
from pathlib import Path
import plistlib
import re
import stat
import sys

import private_recovery_inventory as inventory

BUNDLE = Path('/Users/nambse/.codex/worktrees/14b1/ortak.dev/desktop/src-tauri/target/ortak-private-native/debug/bundle/macos/Ortak Private.app')
BINARY = BUNDLE / 'Contents/MacOS/buzz-desktop'
EXPECTED_SHA = 'ca6ae5d8c723fb6a45199f514b98d7ad931d1865789349ff1a22caf1025c26d9'
BUILD_RECEIPT = inventory.NATIVE_ROLLOUT / 'receipt.json'
RESUME_RECEIPT = inventory.CURRENT_OWNERS
LAUNCHER = inventory.NATIVE_ROLLOUT / 'launch-native78.py'
LAUNCHER_SHA = 'e797840ae3b9b01e57aeab4ad14f9be8bf8cad7a0404296ff3609396597b0913'
SELECTED_SESSION = 62498
SELECTED_PID = 72325
SELECTED_STARTED = 'Mon Sep 7 01:32:19 2026'
ENTRIES = {'.': 'directory', 'Contents': 'directory', 'Contents/MacOS': 'directory',
    'Contents/Resources': 'directory', 'Contents/Info.plist': 'file',
    'Contents/MacOS/buzz-desktop': 'file', 'Contents/Resources/icon.icns': 'file'}
OS_METADATA = {'com.apple.provenance'}
PID_SCAN = r"""import json,subprocess,sys
try:
 p=subprocess.Popen(['/usr/bin/pgrep','-x',sys.argv[1]],stdout=subprocess.PIPE,stderr=subprocess.DEVNULL)
 try:
  raw=p.stdout.read(1025)
  if len(raw)>1024: raise ValueError()
  code=p.wait(timeout=2); ids=raw.decode().split()
  if code not in (0,1) or len(ids)>8 or any(not x.isdigit() for x in ids): raise ValueError()
  if code==1 and ids: raise ValueError()
  print(json.dumps(ids))
 finally:
  if p.poll() is None: p.kill()
  p.wait(timeout=2)
except Exception:
 sys.exit(3)
"""


def generation(row):
    """Bind every opened selected inode to its observed size, mode, ownership and modification time."""
    return (row.st_dev, row.st_ino, row.st_size, row.st_mtime_ns, row.st_mode, row.st_uid, row.st_nlink)


def operating_system_metadata(inspector, path):
    """Root-approved native provenance is inert evidence only, never restored as trusted OS metadata."""
    names = inspector.run(['/usr/bin/xattr', str(path)], limit=4096).decode().splitlines()
    inventory.require(set(names) <= OS_METADATA and len(names) == len(set(names)),
        'native_bundle_extended_metadata_unreviewed')
    result = {}
    for name in names:
        encoded = inspector.run(['/usr/bin/xattr', '-p', '-x', name, str(path)], limit=1024)
        inventory.require(re.fullmatch(rb'[0-9A-Fa-f\s]*', encoded), 'native_os_metadata_encoding')
        raw = bytes.fromhex(encoded.decode())
        inventory.require(len(raw) <= 256, 'native_os_metadata_bound')
        result[name] = base64.b64encode(raw).decode()
    return result


def bundle(inspector):
    """Hash only the seven explicitly reviewed new app entries, with no caches or old native profile."""
    inventory.require(BUNDLE.resolve() == BUNDLE, 'native_bundle_link_refused')
    actual, pending = set(), [BUNDLE]
    while pending:
        path = pending.pop(); relative = str(path.relative_to(BUNDLE)); actual.add(relative)
        inventory.require(len(actual) <= len(ENTRIES) and relative in ENTRIES, 'native_bundle_inventory_changed')
        row = path.lstat()
        inventory.require((stat.S_ISDIR(row.st_mode) if ENTRIES[relative] == 'directory' else stat.S_ISREG(row.st_mode))
            and row.st_uid == os.getuid() and not getattr(row, 'st_flags', 0), 'native_bundle_entry_refused')
        if ENTRIES[relative] == 'directory': pending.extend(path.iterdir())
    inventory.require(actual == set(ENTRIES), 'native_bundle_inventory_changed')
    records = []
    for relative, kind in sorted(ENTRIES.items()):
        path = BUNDLE / relative; row = path.lstat()
        expected_mode = 0o700 if kind == 'directory' or path == BINARY else \
            0o600 if relative == 'Contents/Info.plist' else 0o644
        inventory.require(stat.S_IMODE(row.st_mode) == expected_mode and (kind == 'directory' or row.st_nlink == 1),
            'native_bundle_mode_refused')
        metadata = operating_system_metadata(inspector, path)
        value = {'path': str(path), 'relative': relative, 'kind': kind, 'mode': expected_mode,
            'uid': row.st_uid, 'inode': row.st_ino, 'device': row.st_dev, 'mtime_ns': row.st_mtime_ns,
            'bytes': 0 if kind == 'directory' else row.st_size}
        value['os_metadata_evidence_only'] = metadata
        if kind == 'file':
            inventory.require(row.st_size <= 256 * 1024**2, 'native_bundle_file_bound')
            descriptor = os.open(path, os.O_RDONLY | os.O_NOFOLLOW)
            with os.fdopen(descriptor, 'rb') as stream:
                inventory.require(generation(os.fstat(stream.fileno())) == generation(row), 'native_bundle_changed')
                value['sha256'] = hashlib.file_digest(stream, 'sha256').hexdigest()
            inventory.require(generation(path.lstat()) == generation(row), 'native_bundle_changed')
        records.append(value)
    record, metadata = inventory.public_json(BUILD_RECEIPT.parent, BUILD_RECEIPT.name)
    binary = next(row for row in records if row['path'] == str(BINARY))
    inventory.require(record['status'] == 'built_policy_verified'
        and binary['sha256'] == EXPECTED_SHA == record['native_sha256']
        and plistlib.loads((BUNDLE / 'Contents/Info.plist').read_bytes())['CFBundleIdentifier'] == 'dev.ortak.private20260905',
        'native_bundle_build_identity_mismatch')
    resumed, receipt_metadata = inventory.public_json(RESUME_RECEIPT.parent, RESUME_RECEIPT.name)
    process = resumed.get('native', {})
    inventory.require(receipt_metadata['sha256']==inventory.CURRENT_OWNERS_SHA
        and set(resumed)==set(inventory.NATIVE_WRITERS)|{'native'} and process.get('pid') == SELECTED_PID
        and process.get('session_id') == SELECTED_SESSION and process.get('sha256') == EXPECTED_SHA
        and process.get('executable') == str(BINARY)
        and process.get('cwd') == str(inventory.STATE) and process.get('inode') == binary['inode']
        and process.get('launcher') == str(LAUNCHER)
        and process.get('launcher_sha256') == LAUNCHER_SHA
        and process.get('identity', '').split() == [str(SELECTED_PID), str(os.getuid()), *SELECTED_STARTED.split()],
        'native_resume_receipt_mismatch')
    return {'path': str(BUNDLE), 'binary': str(BINARY), 'binary_sha256': EXPECTED_SHA,
        'build_receipt': metadata, 'resume_receipt': receipt_metadata,
        'entries': records, 'old_native_profile_access': False,
        'os_metadata_restore': 'never_reapply_trust_or_provenance'}


def candidates(inspector):
    """Only process-name matches with the selected private cwd enter the ingress fence."""
    ids = json.loads(inspector.run([sys.executable, '-c', PID_SCAN, 'buzz-desktop'], limit=1024))
    result = []
    for pid in ids:
        cwd = inspector.run(['/usr/sbin/lsof', '-a', '-p', pid, '-d', 'cwd', '-Fn'], limit=4096).decode().splitlines()
        if 'n' + str(inventory.STATE) in cwd: result.append(pid)
    return result


def observe(inspector):
    """A stopped native client is allowed; a changed or unknown private client refuses preparation."""
    artifact = bundle(inspector)
    selected = candidates(inspector)
    inventory.require(len(selected) <= 1, 'native_ingress_ambiguous')
    process = None
    if selected:
        pid = selected[0]
        uid = inspector.run(['/bin/ps', '-p', pid, '-o', 'uid='], limit=128).decode().strip()
        executable = inspector.run(['/bin/ps', '-p', pid, '-o', 'comm='], limit=4096).decode().strip()
        started = inspector.run(['/bin/ps', '-p', pid, '-o', 'lstart='], limit=128).decode().strip()
        inode = next(row['inode'] for row in artifact['entries'] if row['path'] == str(BINARY))
        loaded = inspector.run(['/usr/sbin/lsof', '-a', '-p', pid, '-d', 'txt', '-Fni'], limit=16384).decode().splitlines()
        inventory.require(int(pid) == SELECTED_PID and started.split() == SELECTED_STARTED.split()
            and uid == str(os.getuid()) and executable == str(BINARY)
            and any(loaded[index] == 'i' + str(inode) and loaded[index + 1] == 'n' + str(BINARY)
                for index in range(len(loaded) - 1)), 'native_ingress_loaded_identity_mismatch')
        process = {'pid': int(pid), 'uid': int(uid), 'started_at': started, 'executable': executable,
            'cwd': str(inventory.STATE), 'inode': inode, 'sha256': EXPECTED_SHA,
            'session_id': SELECTED_SESSION, 'session_provenance': 'explicit_root_task_selection'}
    return {'artifact': artifact, 'process': process, 'running': process is not None}


def require_stopped(inspector, expected):
    """Fresh process absence and unchanged artifact both hold; old PID absence alone never qualifies."""
    inventory.require(not candidates(inspector), 'private_native_ingress_still_running')
    inventory.require(bundle(inspector) == expected['artifact'], 'paused_native_bundle_changed')


def capture_entries(inspector, expected):
    """Only the reviewed stopped bundle enters the archive; its profile and caches stay excluded."""
    require_stopped(inspector, expected)
    return [(Path(row['path']), 'native-client/Ortak Private.app' +
        ('' if row['relative'] == '.' else '/' + row['relative'])) for row in expected['artifact']['entries']]
