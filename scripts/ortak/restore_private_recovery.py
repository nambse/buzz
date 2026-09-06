#!/usr/bin/env python3
"""Restore a sealed bundle into generated offline storage only; never activate or replace a source."""

import argparse
from contextlib import ExitStack
import ctypes
from datetime import datetime, timezone
import hashlib
import io
import json
import os
from pathlib import Path
import re
import shutil
import sqlite3
import stat
import sys
import tempfile
import time
import private_recovery_scorer as scorer
import recovery_image_export as image_export
from uuid import uuid4

from cryptography.hazmat.primitives.ciphers.aead import AESGCM

from backup_private_database import Commands, Refused, digest, private_directory
from capture_private_recovery import FORMAT as BUNDLE_FORMAT
from prepare_private_recovery import CAPTURE_LIMITS, canonical, save, sha
from private_recovery_database_metadata import selected_content, selected_extras
import private_recovery_inventory as inventory
import private_recovery_obligations as obligations
import private_recovery_offline_stores as stores
from private_recovery_payloads import MAGIC, copy_file, read_regular
import recovery_archive_io
from recovery_lock_holder import staged_journal_status
import private_recovery_workspace_capture as workspace_capture
import private_recovery_journal as selected_journal
import private_recovery_native_confidential as native_ciphertext

FORMAT = 'ortak-private-offline-recovery/1'


def artifact(bundle, row, expected, maximum):
    """A component can name only its fixed direct child; no arbitrary file can be read or mounted."""
    inventory.require(row['path'] == expected and Path(expected).name == expected
        and type(row['bytes']) is int and 0 <= row['bytes'] <= maximum
        and re.fullmatch(r'[0-9a-f]{64}', row['sha256']), 'offline_artifact_scope')
    path = bundle / expected
    before = path.lstat()
    inventory.require(path.resolve() == path and stat.S_ISREG(before.st_mode) and before.st_nlink == 1
        and before.st_uid == os.getuid() and before.st_mode & 0o777 == 0o600
        and before.st_size == row['bytes'], 'offline_artifact_changed')
    descriptor = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
    hashed, count = hashlib.sha256(), 0
    with os.fdopen(descriptor, 'rb') as stream:
        opened = os.fstat(stream.fileno())
        inventory.require((before.st_dev, before.st_ino, before.st_size) ==
            (opened.st_dev, opened.st_ino, opened.st_size), 'offline_artifact_changed')
        while block := stream.read(65536):
            count += len(block); inventory.require(count <= row['bytes'], 'offline_artifact_size')
            hashed.update(block)
    inventory.require(count == row['bytes'] and hashed.hexdigest() == row['sha256'], 'offline_artifact_changed')
    after = path.lstat()
    inventory.require((before.st_ino, before.st_size, before.st_mtime_ns) ==
        (after.st_ino, after.st_size, after.st_mtime_ns), 'offline_artifact_changed')
    return path


def load_bundle(path, fixture=False):
    """Only a complete sealed selected bundle, or explicitly tagged generated fixture, qualifies."""
    expected_parent = inventory.STATE / ('recovery-fixture-bundles' if fixture else 'recovery-bundles')
    inventory.require(path.name == 'manifest.json' and path.parent.parent == expected_parent
        and re.fullmatch(r'[0-9a-f]{32}', path.parent.name), 'offline_bundle_path')
    inventory.require(not os.path.lexists(path.parent / 'failure.json'), 'offline_capture_failed')
    value, _ = inventory.public_json(path.parent, path.name, maximum=1024 * 1024)
    expected = value.pop('manifest_sha256')
    inventory.require(sha(value) == expected and value['format'] == BUNDLE_FORMAT
        and value['operation_id'] == path.parent.name and value['status'] == 'captured'
        and value.get('fixture_only', False) is fixture
        and value['automatic_activation'] is False and value['full_restore_executed'] is False
        and set(value['components'])-{'workspace_files','native_confidential','scorer'}
            =={'databases', 'volumes', 'journal', 'public_artifacts', 'images'},
        'offline_bundle_integrity')
    value['manifest_sha256'] = expected
    return value


def preflight(bundle, manifest, fixture=False, workspace_commands=None):
    """Check every payload before opening the recovery key or creating a Docker destination."""
    components = manifest['components']
    selections = [(manifest['secrets'], 'secrets.aesgcm', CAPTURE_LIMITS['configuration_secret_bytes'] + 64)]
    for kind in ['main', 'honcho']:
        row = components['databases'][kind]
        selections += [(row, kind + '.dump', CAPTURE_LIMITS[kind + '_database_bytes']),
                       (row['receipt'], kind + '-database.json', 1024**2)]
    for kind in ['redis', 'minio']:
        selections.append((components['volumes'][kind], kind + '.tar', CAPTURE_LIMITS[kind + '_bytes']))
    selections.append((components['journal'], 'journal.sqlite', CAPTURE_LIMITS['sqlite_bytes']))
    if 'native_confidential' in components:
        selections.append((components['native_confidential'],'native-confidential.tar',CAPTURE_LIMITS['native_ciphertext_bytes']))
    journal_component=components['journal']
    if 'source_storage' in journal_component or 'raw_archive' in journal_component:
        storage=journal_component.get('source_storage',{})
        inventory.require(set(storage)=={'kind','selection','source_uid','logical_path','controller_id','controller_image'}
            and storage['kind']=='docker_volume' and storage['source_uid']==10001
            and selected_journal.selection(storage['selection']) is not None
            and storage['logical_path']==str(inventory.RUNTIME/'state/journal.sqlite')
            and re.fullmatch('[0-9a-f]{64}',storage['controller_id'])
            and re.fullmatch('sha256:[0-9a-f]{64}',storage['controller_image']), 'offline_journal_storage_refused')
        selections.append((journal_component['raw_archive'],'journal-raw.tar',selected_journal.archive.MAX_ARCHIVE_BYTES))
    for row in components['journal'].get('cold_companions', []):
        inventory.require(row['path'] in ('cold-journal.sqlite-wal', 'cold-journal.sqlite-shm'), 'offline_journal_companion_scope')
        selections.append((row, row['path'], CAPTURE_LIMITS['sqlite_bytes']))
    selections += [(components['public_artifacts']['configuration'], 'configuration.tar', CAPTURE_LIMITS['configuration_secret_bytes'] + 100000 * 2048),
                   (components['public_artifacts']['native_and_repositories'], 'native-and-repositories.tar', CAPTURE_LIMITS['native_artifacts_bytes'] + 100000 * 2048)]
    if not fixture:
        selected_image=image_export.selection(components['images'],CAPTURE_LIMITS['image_exports_bytes'])
        selections.append((components['images'],selected_image['path'],selected_image['physical_limit']))
    generation = {}
    for row, name, limit in selections:
        inventory.require(name not in generation, 'offline_duplicate_payload')
        path = artifact(bundle, row, name, limit)
        generation[path] = payload_identity(path)
    if not fixture:
        image_export.verify_gzip(bundle/selected_image['path'],components['images'],CAPTURE_LIMITS['image_exports_bytes'])
    if 'workspace_files' in components:
        row=components['workspace_files']
        inventory.require(row.get('path')=='workspace-files','offline_workspace_path_refused')
        inventory.require(workspace_commands is not None,'offline_workspace_command_output_required')
        command=workspace_commands
        proof=workspace_capture.bounded_action('verify',{'bundle':str(bundle/'workspace-files'),
            'manifest_sha256':row['manifest_sha256']},command)
        evidence=components['databases']['main']['recovery_obligations']['evidence']
        inventory.require(type(evidence['schema_version']) is int
            and evidence['schema_version'] in (74,75,76,77,78) and proof['database_evidence_sha256']==
            workspace_capture.digest(workspace_capture.canonical(evidence)),'offline_workspace_database_binding')
        for name in (workspace_capture.files.ARCHIVE,workspace_capture.files.MANIFEST):
            target=bundle/'workspace-files'/name
            generation[target]=payload_identity(target)
    return generation


def payload_identity(path):
    """Recheck the already-hashed file generation before declaring restoration complete."""
    row = path.lstat()
    return row.st_dev, row.st_ino, row.st_size, row.st_mtime_ns, row.st_mode


def decrypt(bundle, manifest, prepared, output, fixture=False):
    """Use only the separate selected local key; recover exact secrets into a private unused tree."""
    row = manifest['secrets']
    encrypted = artifact(bundle, row, 'secrets.aesgcm', CAPTURE_LIMITS['configuration_secret_bytes'] + 64)
    key = Path(row['key_reference'])
    expected_parent = inventory.STATE / ('recovery-fixture-keys' if fixture else 'recovery-keys')
    inventory.require(key == expected_parent / (bundle.name + '.key') and not key.is_relative_to(bundle),
                      'offline_key_scope')
    inventory.directory(key.parent)
    inventory.require(key.lstat().st_mode & 0o777 == 0o600, 'offline_key_mode')
    key_bytes, _ = read_regular(key, 32, secret=True)
    inventory.require(len(key_bytes) == 32, 'offline_key_mode')
    metadata = prepared['observation']['files']['secret_metadata_only']
    selected = {record['path'] for record in metadata}
    if fixture:
        fixture_root = bundle / 'fixture-secrets'
        inventory.require(selected == {str(fixture_root / key) for key in ['main-password', 'honcho-password', 'oauth-test']},
                          'offline_fixture_secret_scope')
    else:
        inventory.require(selected == {str(root / leaf) for root, leaves in inventory.SECRET_FILES.items() for leaf in leaves},
                          'offline_secret_allowlist')
    members = {'selected/' + value.lstrip('/') for value in selected}
    members |= {'metadata/main-database-settings.json', 'metadata/honcho-database-settings.json'}
    inventory.require(set(row['members']) == members and len(row['members']) == len(members)
        and row['aad'] == {'format': BUNDLE_FORMAT, 'operation_id': bundle.name,
            'components_sha256': sha(manifest['components']), 'secret_metadata_sha256': sha(metadata)},
        'offline_secret_binding')
    raw, _ = read_regular(encrypted, CAPTURE_LIMITS['configuration_secret_bytes'] + 64)
    inventory.require(raw[:8] == MAGIC and len(raw) >= 36, 'offline_envelope_format')
    plaintext = AESGCM(key_bytes).decrypt(raw[8:20], raw[20:], canonical(row['aad']))
    recovery_archive_io.archive(io.BytesIO(plaintext), CAPTURE_LIMITS['configuration_secret_bytes'],
                                output, expected_names=members)
    for record in metadata:
        target = output / ('selected/' + record['path'].lstrip('/'))
        data = target.lstat()
        inventory.require(data.st_uid == os.getuid() and data.st_nlink == 1 and data.st_size == record['bytes']
            and data.st_mode & 0o777 == record['mode'], 'offline_secret_metadata')
    settings = {kind: json.loads((output / 'metadata' / (kind + '-database-settings.json')).read_bytes())
                for kind in ['main', 'honcho']}
    passwords = ({kind: output / 'selected' / str(bundle / 'fixture-secrets' / (kind + '-password')).lstrip('/')
                  for kind in ['main', 'honcho']} if fixture else {
        'main': output / 'selected' / str(inventory.STATE / 'secrets/postgres-password').lstrip('/'),
        'honcho': output / 'selected' / str(inventory.STATE / 'honcho-tests/postgres-password').lstrip('/')})
    return settings, passwords, {'authenticated_decryption': True, 'files': len(members),
        'live_source_credentials_read': False, 'backed_up_secret_material_read': True,
        'runtime_mounted': False, 'provider_health': 'unvalidated'}


def journal(path, *, confidential_reviewed=None):
    """Inspect a complete backup through a private working copy, leaving the artifact byte-for-byte unchanged."""
    before = payload_identity(path)
    with tempfile.TemporaryDirectory(prefix='ortak-offline-journal-') as temporary:
        working = Path(temporary).resolve() / 'journal.sqlite'
        copy_file(path, working, CAPTURE_LIMITS['sqlite_bytes'])
        result = journal_working(working,confidential_reviewed=confidential_reviewed)
    inventory.require(payload_identity(path) == before, 'offline_journal_source_changed')
    return result


def journal_working(path, *, confidential_reviewed=None):
    """Read every table/cursor/diagnostic while allowing SQLite working sidecars in this disposable copy."""
    protected=selected_journal.confidential_selection(confidential_reviewed)
    counters = staged_journal_status(path,confidential_reviewed=protected is not None)
    database = sqlite3.connect(path.as_uri() + '?mode=rw', uri=True, timeout=2)
    deadline = time.monotonic() + 20
    database.set_progress_handler(lambda: time.monotonic() >= deadline, 1000)
    try:
        database.execute('PRAGMA query_only=ON')
        inventory.require(database.execute('PRAGMA integrity_check').fetchall() == [('ok',)]
            and not database.execute('PRAGMA foreign_key_check').fetchall(), 'offline_journal_integrity')
        schema = database.execute("SELECT type,name,tbl_name,sql FROM sqlite_master WHERE name NOT LIKE 'sqlite_%' ORDER BY type,name").fetchall()
        tables = [row[1] for row in schema if row[0] == 'table']
        inventory.require(len(tables) <= 128, 'offline_journal_table_bound')
        counts, hashes, total = {}, {}, 0
        for table in tables:
            quoted = '"' + table.replace('"', '""') + '"'
            rows = []
            for row in database.execute('SELECT * FROM ' + quoted):
                total += 1; inventory.require(total <= 200000 and time.monotonic() < deadline, 'offline_journal_row_bound')
                encoded = json.dumps([{'blob': value.hex()} if isinstance(value, bytes) else value for value in row],
                    sort_keys=True, separators=(',', ':'), allow_nan=False).encode()
                inventory.require(len(encoded) <= 1024**2, 'offline_journal_row_size')
                rows.append(hashlib.sha256(encoded).hexdigest())
            counts[table] = len(rows)
            hashes[table] = hashlib.sha256(''.join(sorted(rows)).encode()).hexdigest()
        diagnostics = counts.get('private_failure_diagnostics', 0)
        if 'private_failure_diagnostics' in tables:
            malformed = database.execute('SELECT count(*) FROM private_failure_diagnostics WHERE length(CAST(diagnostic AS BLOB))>2048').fetchone()[0]
            inventory.require(not malformed and diagnostics <= counters['runs'], 'offline_diagnostics_bound')
        return {'integrity': 'ok', 'counters': counters, 'tables': counts, 'logical_rows_sha256': hashes,
                'schema_sha256': sha(schema), 'private_failure_diagnostics': diagnostics}
    finally:
        database.close()


def restore_journal_component(bundle, component, output, *, confidential_reviewed=None):
    """Require physical raw extraction and logical equality before reporting a volume journal restored."""
    source=artifact(bundle,component,'journal.sqlite',CAPTURE_LIMITS['sqlite_bytes'])
    protected=selected_journal.confidential_selection(confidential_reviewed)
    inventory.require(component.get('confidential_selection')==protected
        and ('confidential_proof' in component)==(protected is not None),
        'offline_journal_source_generation_changed')
    expected=journal(source,confidential_reviewed=protected)
    if protected is not None:
        inventory.require(component['confidential_proof']==expected,'offline_journal_changed')
    copy_file(source,output/'journal.sqlite',CAPTURE_LIMITS['sqlite_bytes'])
    result=journal(output/'journal.sqlite',confidential_reviewed=protected)
    inventory.require(result==expected,'offline_journal_changed')
    if 'raw_archive' in component:
        row=component['raw_archive']
        raw=artifact(bundle,row,'journal-raw.tar',selected_journal.archive.MAX_ARCHIVE_BYTES)
        proof=selected_journal.extract(raw,output/'journal-raw',row['archive'],Commands(output))
        from private_recovery_payloads import sqlite_backup
        sqlite_backup(output/'journal-raw/journal.sqlite',output/'journal-raw-coherent.sqlite',cold=True)
        inventory.require(journal(output/'journal-raw-coherent.sqlite',confidential_reviewed=protected)==expected,
            'offline_journal_raw_changed')
        result['raw_restore']=proof
        result['source_storage']=component['source_storage']
    result['cold_companions']=[copy_file(bundle/row['path'],output/row['path'],CAPTURE_LIMITS['sqlite_bytes'])
        for row in component.get('cold_companions',[])]
    return result


def directory_identity(row):
    """Pin directory ownership and generation without treating child writes as replacement."""
    return {'device':row.st_dev,'inode':row.st_ino,'uid':row.st_uid,
        'gid':row.st_gid,'mode':stat.S_IMODE(row.st_mode)}


def workspace_destination_plan(bundle, row, output):
    """Select only the fresh output or fixed private STATE for inherited archive groups.

    The watched verifier already authenticated the archive; reread its bounded,
    hash-pinned manifest here without traversing any original workspace path.
    The existing extractor remains responsible for exact physical readback.
    """
    files=workspace_capture.files
    inventory.require(row.get('path')=='workspace-files','offline_workspace_path_refused')
    with files.Source(maximum=files.MAX_MANIFEST) as source:
        root=source.root(str(bundle/'workspace-files'))
        raw, record=source.file(root,files.MANIFEST,'manifest',files.MAX_MANIFEST,(0o600,))
        if not raw:
            raw=b''.join(source.blocks(source.entries['manifest'][1],record['bytes']))
        inventory.require(record['sha256']==row['manifest_sha256']
            and hashlib.sha256(raw).hexdigest()==row['manifest_sha256'],
            'offline_workspace_manifest_changed')
        manifest=json.loads(raw)
        inventory.require(files.canonical(manifest)==raw and manifest['format']==files.FORMAT
            and type(manifest['entries']) is list and 0<len(manifest['entries'])<=files.MAX_ENTRIES,
            'offline_workspace_manifest_changed')
        groups=set()
        for entry in manifest['entries']:
            inventory.require(type(entry['gid']) is int and 0<=entry['gid']<=2**32-1
                and type(entry['uid']) is int and entry['uid']==os.getuid(),
                'offline_workspace_destination_owner_refused')
            groups.add(entry['gid'])
        source.check()
    with files.Source() as source:
        output_fd=source.root(str(output))
        destination=directory_identity(os.fstat(output_fd))
        actor_groups=set(os.getgroups())|{os.getegid()}
        def compatible(group):
            return groups=={group} if len(groups)==1 else groups<=(actor_groups|{group})
        parent=output
        parent_identity=destination
        if not compatible(destination['gid']):
            parent=inventory.STATE
            parent_fd=source.root(str(parent))
            parent_identity=directory_identity(os.fstat(parent_fd))
        valid=(compatible(parent_identity['gid'])
            and parent_identity['device']==destination['device'])
        plan={'manifest_sha256':row['manifest_sha256'],'source_gids':sorted(groups),
            'output':str(output),'output_identity':destination,'parent':str(parent),
            'parent_identity':parent_identity,'compatible':valid,
            'strategy':'direct' if parent==output else 'inherited_group_move',
            'ownership_changed':False,'workspace_extracted':False}
        files.save(output_fd,'workspace-destination-plan.json',canonical(plan))
        source.check()
        inventory.require(valid,'offline_workspace_destination_group_mismatch')
        inventory.require(parent==output or sys.platform=='darwin',
            'offline_workspace_exclusive_rename_unavailable')
    return plan


def rename_directory_exclusive(source_fd, source_name, destination_fd, destination_name):
    """Darwin descriptor-relative atomic move; even an empty collision is retained.

    RENAME_EXCL=0x00000004 is defined by the selected macOS SDK sys/stdio.h.
    No check-then-rename or shell fallback can provide this no-overwrite contract.
    """
    inventory.require(sys.platform=='darwin','offline_workspace_exclusive_rename_unavailable')
    for name in (source_name,destination_name):
        inventory.require(type(name) is str and re.fullmatch(r'[a-z0-9][a-z0-9.-]{0,95}',name),
            'offline_workspace_destination_name_refused')
    library=ctypes.CDLL('/usr/lib/libSystem.B.dylib',use_errno=True)
    move=library.renameatx_np
    move.argtypes=[ctypes.c_int,ctypes.c_char_p,ctypes.c_int,ctypes.c_char_p,ctypes.c_uint]
    move.restype=ctypes.c_int
    if move(source_fd,source_name.encode('ascii'),destination_fd,destination_name.encode('ascii'),0x00000004)!=0:
        code=ctypes.get_errno()
        raise OSError(code,os.strerror(code))


def workspace_destination(output, plan):
    """Create one inert directory, preserving its inherited group through an exclusive move."""
    files=workspace_capture.files
    parent=Path(plan['parent'])
    inventory.require(plan['compatible'] is True and plan['output']==str(output)
        and parent in (output,inventory.STATE)
        and plan['strategy']==('direct' if parent==output else 'inherited_group_move'),
        'offline_workspace_destination_plan_changed')
    with files.Source() as source:
        output_fd=source.root(str(output)); parent_fd=source.root(str(parent))
        inventory.require(directory_identity(os.fstat(output_fd))==plan['output_identity']
            and directory_identity(os.fstat(parent_fd))==plan['parent_identity'],
            'offline_workspace_destination_plan_changed')
        target='workspace-files'
        name=target if parent==output else 'ortak-recovery-workspace-'+uuid4().hex
        intent={'plan_sha256':sha(plan),'staged_parent':str(parent),'staged_name':name,
            'destination':str(output/target),'status':'intent','ownership_changed':False}
        files.save(output_fd,'workspace-destination-intent.json',canonical(intent))
        try:
            os.mkdir(name,mode=0o700,dir_fd=parent_fd)
            fd=source.fd(name,os.O_RDONLY|os.O_DIRECTORY,parent_fd)
            identity=directory_identity(source.directory(fd,(0o700,)))
            inventory.require(identity['gid']==plan['parent_identity']['gid']
                and identity['device']==plan['output_identity']['device']
                and not source.names(fd,remember=False),'offline_workspace_destination_changed')
            os.fsync(fd);os.fsync(parent_fd)
            files.save(output_fd,'workspace-destination-staged.json',canonical({**intent,
                'status':'staged','identity':identity}))
            source.check()
            inventory.require(directory_identity(os.stat(name,dir_fd=parent_fd,follow_symlinks=False))==identity,
                'offline_workspace_destination_changed')
            if parent!=output:
                rename_directory_exclusive(parent_fd,name,output_fd,target)
                try:os.stat(name,dir_fd=parent_fd,follow_symlinks=False)
                except FileNotFoundError:pass
                else:raise Refused('offline_workspace_destination_changed')
                os.fsync(parent_fd);os.fsync(output_fd)
            inventory.require(directory_identity(os.stat(target,dir_fd=output_fd,follow_symlinks=False))==identity
                and directory_identity(os.fstat(fd))==identity
                and not source.names(fd,remember=False),'offline_workspace_destination_changed')
            source.check()
            files.save(output_fd,'workspace-destination-created.json',canonical({**intent,
                'status':'created','identity':identity}))
            source.check()
        except BaseException:
            files.save(output_fd,'workspace-destination-failure.json',canonical({**intent,
                'status':'failed_retained','partial_destination_retained':True}))
            raise
    return output/target


def restore_workspace_component(bundle, row, output, *, destination_plan=None):
    """Physical extraction and exact descriptor readback are required before foundation success."""
    inventory.require(row.get('path')=='workspace-files','offline_workspace_path_refused')
    plan=destination_plan if destination_plan is not None else workspace_destination_plan(bundle,row,output)
    inventory.require(plan['manifest_sha256']==row['manifest_sha256'],'offline_workspace_destination_plan_changed')
    target=workspace_destination(output,plan)
    command=Commands(output)
    proof=workspace_capture.bounded_action('extract',{'bundle':str(bundle/'workspace-files'),
        'manifest_sha256':row['manifest_sha256'],'destination':str(target)},command)
    inventory.require(proof['status']=='workspace_files_restored_offline' and
        proof['manifest_sha256']==row['manifest_sha256'] and proof['automatic_activation'] is False
        and proof['physical_erasure'] is False,'offline_workspace_restore_incomplete')
    save(output/'workspace-files-verified.json',proof)
    return proof


def native_destination_group(bundle, row, output, selected):
    """Refuse incompatible inherited groups before decrypting or creating stores.

    Reuse the pinned native manifest parser; no ciphertext/SQLite is decoded and
    no group is changed. Final extraction still verifies every byte and owner.
    """
    native = native_ciphertext.store
    inventory.require(row['receipt']['archive'] == {'bytes':row['bytes'],'sha256':row['sha256']}
        and row['receipt']['stopped_native_sha256'] == selected['native_owner_sha256'],
        'native_confidential_selection_refused')
    path = artifact(bundle,row,'native-confidential.tar',CAPTURE_LIMITS['native_ciphertext_bytes'])
    descriptor = os.open(path,os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
    with os.fdopen(descriptor,'rb') as stream:
        store = native.read_manifest(native.Budget(stream),row['receipt'],native.absolute(selected['app_data']))
    with ExitStack() as stack:
        fd, links = native.directory(stack,output)
        current = os.fstat(fd)
        native.wire.metadata(current,os.getuid(),root=True)
        groups = ([] if store['state']=='absent' else
            sorted({store['root']['gid'],*[entry['gid'] for entry in store['files']]}))
        compatible = not groups or groups == [current.st_gid]
        proof = {'state':store['state'],'source_gids':groups,
            'destination':{'device':current.st_dev,'inode':current.st_ino,'uid':current.st_uid,
                'gid':current.st_gid,'mode':stat.S_IMODE(current.st_mode)},
            'compatible':compatible,'ownership_changed':False,'ciphertext_extracted':False}
        save(output/'native-confidential-destination.json',proof)
        native.wire.check_links(links)
        inventory.require(compatible,'offline_native_destination_group_mismatch')
    return proof


def restore(path, *, fixture=False):
    """Retain one new offline storage rehearsal; failure never cleans up or promotes a target."""
    manifest = load_bundle(path, fixture)
    bundle = path.parent
    output = private_directory(private_directory(inventory.STATE / 'recovery-offline-restores') / uuid4().hex, fresh=True)
    result = {'format': FORMAT, 'operation_id': output.name, 'status': 'started', 'fixture_only': fixture,
        'bundle_manifest_sha256': manifest['manifest_sha256'], 'started_at': datetime.now(timezone.utc).isoformat(),
        'source_mutations': False, 'provider_requests': False, 'office_requests': False,
        'application_started': False, 'runtime_activation': False, 'independent_host_verified': False,
        'published_ports': [], 'network': 'none', 'docker_socket_mounted': False, 'components': {}}
    save(output / 'intent.json', result)
    try:
        inventory.require(shutil.disk_usage(output).free >= 4 * 1024**3, 'offline_restore_capacity')
        components = manifest['components']
        generation = preflight(bundle, manifest, fixture,Commands(output))
        config = artifact(bundle, components['public_artifacts']['configuration'], 'configuration.tar',
                          CAPTURE_LIMITS['configuration_secret_bytes'] + 100000 * 2048)
        public = private_directory(output / 'public-artifacts', fresh=True)
        with config.open('rb') as stream:
            result['components']['configuration'] = recovery_archive_io.archive(stream,
                CAPTURE_LIMITS['configuration_secret_bytes'], public)
        prepared = json.loads((public / 'operation/preparation.json').read_bytes())
        scorer_proof=scorer.verify_offline(prepared['observation'].get('scorer_owner'),
            components.get('scorer'),public,prepared['observation']['files']['secret_metadata_only'])
        if scorer_proof is not None:result['components']['scorer']=scorer_proof
        source_controller=prepared['observation']['containers']['controller']
        volume=source_controller.get('volume')
        if volume and 'created_at' in volume:
            storage=components['journal'].get('source_storage',{})
            inventory.require(storage.get('kind')=='docker_volume'
                and storage.get('controller_id')==source_controller['id']
                and storage.get('controller_image')==source_controller['image']
                and storage.get('selection')=={'name':volume['name'],'created_at':volume['created_at'],
                    'owner_id':volume['owner']} and 'raw_archive' in components['journal'],
                'offline_journal_source_generation_changed')
        else:
            inventory.require('source_storage' not in components['journal']
                and 'raw_archive' not in components['journal'],'offline_journal_source_generation_changed')
        archived_metadata = {kind: json.loads((bundle / (kind + '-database.json')).read_bytes())['expected']
            for kind in ['main', 'honcho']}
        recovery_contract = obligations.stack_contract(archived_metadata['main'], archived_metadata['honcho'])
        if recovery_contract['main']['schema_version'] >= 68 or 'recovery_contract' in prepared['plan']:
            inventory.require(prepared['plan'].get('recovery_contract') == recovery_contract,
                'offline_recovery_contract_changed')
        result['recovery_contract'] = recovery_contract
        protected=selected_journal.require_confidential_schema(
            prepared['observation'].get('journal_confidential'),recovery_contract['main']['schema_version'])
        inventory.require(components['journal'].get('confidential_selection')==protected,
            'offline_journal_source_generation_changed')
        native_store=native_ciphertext.selection(prepared['observation'].get('native_confidential'),
            recovery_contract['main']['schema_version'],prepared['observation']['native_ingress'])
        inventory.require(('native_confidential' in components)==(native_store is not None),
            'native_confidential_selection_refused')
        if native_store is not None:
            inventory.require(components['native_confidential']['selection']==native_store,
                'native_confidential_selection_refused')
            native_destination_group(bundle,components['native_confidential'],output,native_store)
        selected=prepared['observation'].get('workspace_selection')
        obligations.workspaces.require_capture_selection(archived_metadata['main'],selected,inventory.COMPANY)
        if recovery_contract['main']['schema_version']>=74:
            obligations.workspaces.require_capture_scope(archived_metadata['main'],
                components['databases']['main']['recovery_obligations']['evidence'])
        inventory.require(('workspace_files' in components)==(selected is not None),
            'offline_workspace_component_required')
        if selected is not None:
            command=Commands(output)
            proof=workspace_capture.bounded_action('verify',{'bundle':str(bundle/'workspace-files'),
                'manifest_sha256':components['workspace_files']['manifest_sha256']},command)
            inventory.require(proof['selection']==selected,'offline_workspace_selection_changed')
            workspace_plan=workspace_destination_plan(bundle,components['workspace_files'],output)
        secret_output = private_directory(output / 'secret-material', fresh=True)
        settings, passwords, result['components']['secrets'] = decrypt(bundle, manifest, prepared, secret_output, fixture)
        images = components['images']['images']
        inventory.require(len(images) == len(set(images)) and set(images) == set(prepared['plan']['images']), 'offline_image_set')
        if scorer_proof is not None:
            inventory.require(prepared['observation']['scorer_owner']['selection']['container']['image'] in images,
                'offline_scorer_image_not_captured')
        if fixture:
            inventory.require(components['images'].get('fixture_existing_images_only') is True, 'offline_fixture_images')
        else:
            selected_image=image_export.selection(components['images'],CAPTURE_LIMITS['image_exports_bytes'])
            artifact(bundle,components['images'],selected_image['path'],selected_image['physical_limit'])
        result['components']['images'] = {'images': images, 'new_export_created': False, 'archive_copied': False,
                                          'fixture_existing_images_only': fixture}
        if not fixture and selected_image['compression']=='gzip':
            result['components']['images'].update(format=image_export.FORMAT,compression='gzip',
                uncompressed_bytes=components['images']['uncompressed_bytes'],footer_verified_during_preflight=True,
                image_loading_performed=False)
        for kind, service in [('main', 'postgres'), ('honcho', 'honcho_postgres')]:
            row = components['databases'][kind]
            archive = artifact(bundle, row, kind + '.dump', CAPTURE_LIMITS[kind + '_database_bytes'])
            receipt_path = artifact(bundle, row['receipt'], kind + '-database.json', 1024**2)
            receipt = json.loads(receipt_path.read_bytes())
            inventory.require(receipt['status'] == 'verified' and receipt['expected'] == receipt['restored'], 'offline_database_receipt')
            image = prepared['observation']['containers'][service]['image']
            inventory.require(image in images, 'offline_database_image_not_captured')
            command = stores.Postgres(private_directory(output / kind, fresh=True), output.name, kind, image, passwords[kind])
            command.launch(); command.create_database(settings[kind])
            if kind == 'honcho':
                command.restore(archive, source_checks=receipt.get('source_checks'))
            else:
                command.restore(archive)
            observed = command.restored_metadata()
            inventory.require(observed == receipt['expected'], 'offline_database_catalog_or_counts_mismatch')
            content = selected_content(command, command.database, 'verified-content', observed['tables'])
            inventory.require(content == row['logical_rows_sha256'], 'offline_database_receipt_bytes_mismatch')
            inventory.require(selected_extras(command, command.database, 'settings') == settings[kind],
                              'offline_database_role_settings_or_sequence_mismatch')
            recovery = {}
            if kind == 'main':
                contract = obligations.main_contract(observed)
                if contract['schema_version'] >= 68 or 'recovery_obligations' in row:
                    # Old archives containing new tables without a frozen witness
                    # fail closed; they cannot acquire activation authority here.
                    inventory.require('recovery_obligations' in row, 'offline_recovery_obligations_missing')
                    recovery = obligations.verify_restore(command, command.database, observed,
                        inventory.COMPANY, row['recovery_obligations']['evidence'])
            else:
                contract = obligations.verify_honcho(command, command.database, observed)
                inventory.require(contract == row.get('recovery_contract', contract)
                    and (contract['reviewed_wire_family'] is None or 'recovery_contract' in row),
                    'offline_honcho_recovery_contract_missing')
            result['components'][kind] = {'owner': command.stop_retained(), 'tables': len(observed['tables']),
                'schema_sha256': observed['schema_sha256'], 'logical_rows_sha256': content,
                'sequence_settings_verified': True, 'source_receipt_sha256': row['receipt']['sha256'],
                'recovery_contract': contract, 'recovery_obligations': recovery}
            save(output / (kind + '-verified.json'), result['components'][kind])
        python_image = prepared['observation']['containers']['controller']['image']
        inventory.require(python_image in images, 'offline_reader_image_not_captured')
        for kind in ['redis', 'minio']:
            maximum = CAPTURE_LIMITS[kind + '_bytes']
            archive = artifact(bundle, components['volumes'][kind], kind + '.tar', maximum)
            result['components'][kind] = stores.restore_volume(private_directory(output / kind, fresh=True),
                output.name, kind, python_image, archive, maximum)
        result['components']['journal']=restore_journal_component(bundle,components['journal'],output,
            confidential_reviewed=protected)
        if native_store is not None:
            row=components['native_confidential']
            path=artifact(bundle,row,'native-confidential.tar',CAPTURE_LIMITS['native_ciphertext_bytes'])
            inventory.require(row['receipt']['archive']=={'bytes':row['bytes'],'sha256':row['sha256']}
                and row['receipt']['stopped_native_sha256']==native_store['native_owner_sha256'],
                'native_confidential_selection_refused')
            result['components']['native_confidential']=workspace_capture.bounded_action('native-confidential-extract',
                {'app_data':native_store['app_data'],'archive':str(path),'destination':str(output/'native-confidential'),
                 'receipt':row['receipt']},Commands(output))
        if 'workspace_files' in components:
            row=components['workspace_files']
            result['components']['workspace_files']=restore_workspace_component(bundle,row,output,
                destination_plan=workspace_plan)
        natives = components['public_artifacts']['native_and_repositories']
        archive = artifact(bundle, natives, 'native-and-repositories.tar', CAPTURE_LIMITS['native_artifacts_bytes'] + 100000 * 2048)
        with archive.open('rb') as stream:
            result['components']['native_and_repositories'] = recovery_archive_io.archive(stream,
                CAPTURE_LIMITS['native_artifacts_bytes'], private_directory(output / 'native-and-repositories', fresh=True))
        inventory.require(all(payload_identity(path) == expected for path, expected in generation.items())
            and load_bundle(bundle / 'manifest.json', fixture) == manifest, 'offline_bundle_generation_changed')
        result.update(status='offline_foundation_verified', completed_at=datetime.now(timezone.utc).isoformat(),
            activation_requires=obligations.ACTIVATION_GATES,
            limitations=['redis_aof_load_and_expiry_semantics_not_verified', 'minio_service_semantics_not_verified',
                'no_application_or_provider_activation', 'same_host_storage_only_not_independent_disaster_recovery'])
        if 'raw_restore' in result['components']['journal']:
            result['activation_requires']=[*result['activation_requires'],
                *result['components']['journal']['raw_restore']['activation_requires']]
        result['manifest_sha256'] = sha(result)
        save(output / 'manifest.json', result)
    except Exception as error:
        result.update(status='failed', error_type=type(error).__name__, error_code='offline_restore_failed_retained',
            failure_code=error.args[0] if type(error) is Refused else 'offline_component_failed')
        save(output / 'manifest.json', result)
        raise Refused('offline_restore_failed_retained') from None
    return output


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--bundle', type=Path, required=True)
    args = parser.parse_args()
    try:
        output = restore(args.bundle)
        print(json.dumps({'status': 'offline_foundation_verified', 'manifest': str(output / 'manifest.json'), 'runtime_activation': False}))
    except Exception:
        raise SystemExit('Offline recovery refused; new private artifacts/volumes/databases remain retained. Sources unchanged.') from None
