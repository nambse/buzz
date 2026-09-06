#!/usr/bin/env python3
"""Actual capture tail on owned fixtures: cold volumes, WAL, public files, image export, encryption.

Database acquisition and live process/schema/OAuth barrier admission are explicit
synthetic boundaries. Every later Capture method and the enclosing capture state
machine execute unchanged. No live store, source credential, native profile or
provider is read; existing image bytes and stopped generated fixture volumes only.
"""

import argparse
from contextlib import contextmanager, ExitStack
import hashlib
import io
import json
import os
from pathlib import Path
import plistlib
import re
import shutil
import sqlite3
import tarfile
import time
from unittest.mock import patch
from uuid import uuid4

from cryptography.hazmat.primitives.ciphers.aead import AESGCM
import capture_private_recovery as capture
from backup_private_database import Commands, private_directory, private_binary
from prepare_private_recovery import save, sha, canonical
import private_recovery_inventory as inventory
import private_recovery_payloads as payload
import recovery_archive_io as archive_io
import recovery_native_ingress as native


def image_export_witness(path, images):
    """Verify Docker's actual OCI manifest identities and every selected descriptor blob."""
    deadline = time.monotonic() + 60
    total, seen = 0, set()
    with tarfile.open(path) as archive:
        inventory.require(archive.getmember('index.json').size <= 65536, 'fixture_image_index_bound')
        index = json.load(archive.extractfile('index.json'))
        descriptors = index['manifests']
        inventory.require(index['schemaVersion'] == 2 and len(descriptors) == len(images)
            and {row['digest'] for row in descriptors} == set(images), 'fixture_image_export_changed')
        def read_blob(row, *, document=False):
            nonlocal total
            digest = row['digest']
            inventory.require(re.fullmatch(r'sha256:[0-9a-f]{64}', digest) is not None
                and type(row['size']) is int and 0 <= row['size'] <= 8 * 1024**3, 'fixture_image_descriptor_refused')
            member = archive.getmember('blobs/sha256/' + digest[7:])
            inventory.require(member.isfile() and member.size == row['size']
                and (not document or member.size <= 2 * 1024**2), 'fixture_image_blob_bound')
            if digest in seen and not document: return None
            seen.add(digest); total += member.size
            inventory.require(total <= 8 * 1024**3 and len(seen) <= 256, 'fixture_image_export_bound')
            hashed = hashlib.sha256(); content = bytearray()
            with archive.extractfile(member) as stream:
                while block := stream.read(65536):
                    inventory.require(time.monotonic() < deadline, 'fixture_image_verify_deadline')
                    hashed.update(block)
                    if document: content.extend(block)
            inventory.require('sha256:' + hashed.hexdigest() == digest, 'fixture_image_blob_changed')
            return json.loads(content) if document else None
        for descriptor in descriptors:
            document = read_blob(descriptor, document=True)
            inventory.require(document['schemaVersion'] == 2 and len(document['layers']) <= 128,
                'fixture_image_manifest_refused')
            read_blob(document['config'])
            for layer in document['layers']: read_blob(layer)
    return {'images': images, 'blob_count': len(seen), 'blob_bytes_verified': total,
        'identity_kind': 'oci_manifest_digest'}


def fixture_sources(path, command):
    """Only exact already-stopped, newly generated service fixtures can supply a volume."""
    inventory.require(path.parent.parent == inventory.STATE / 'recovery-service-fixtures'
        and path.name == 'manifest.json', 'capture_fixture_path_refused')
    value, _ = inventory.public_json(path.parent, path.name)
    digest = value.pop('manifest_sha256')
    inventory.require(sha(value) == digest and value['status'] == 'verified'
        and value['source_access'] is False, 'capture_fixture_receipt_refused')
    result = {}
    for kind in ('redis', 'minio'):
        row = value[kind]['seed']; volume = row['volume']
        operation = volume['labels']['org.ortak.offline_recovery']
        inventory.require(volume['name'] == 'ortak_offline_' + operation + '_' + kind
            and row['name'] == 'ortak-offline-services-' + operation + '-' + kind
            and row['running'] is False, 'capture_fixture_owner_refused')
        fmt = '{"id":{{json .Id}},"image":{{json .Image}},"running":{{json .State.Running}},"pid":{{json .State.Pid}},"exit":{{json .State.ExitCode}},"network":{{json .HostConfig.NetworkMode}},"ports":{{json .HostConfig.PortBindings}},"mounts":{{json .Mounts}}}'
        current = json.loads(command.run(kind + '-fixture-owner', command.docker('inspect', '--format', fmt, row['id'])))
        mounts = [m for m in current['mounts'] if m['Type'] == 'volume']
        inventory.require(current['id'] == row['id'] and current['image'] == inventory.SERVICES[kind][2]
            and current['running'] is False and current['pid'] == 0 and current['exit'] == 0
            and current['network'] == 'none' and not current['ports'] and len(mounts) == 1
            and mounts[0]['Name'] == volume['name'] and mounts[0]['Destination'] == '/data',
            'capture_fixture_owner_changed')
        result[kind] = row
    return result


def execute(service_manifest):
    """Retain bounded fixture artifacts and never publish a fixture into the live bundle directory."""
    out = private_directory(inventory.EVIDENCE / ('g-capture-tail-' + uuid4().hex), fresh=True)
    command = Commands(private_directory(out / 'owner-checks', fresh=True))
    sources = fixture_sources(service_manifest, command)
    state = private_directory(out / 'synthetic-state', fresh=True)
    runtime = private_directory(state / 'runtime', fresh=True)
    runtime_state = private_directory(runtime / 'state', fresh=True)
    operation = uuid4().hex
    operators = private_directory(private_directory(state / 'recovery-operations') / operation, fresh=True)
    build = private_directory(state / 'fixture-native-build', fresh=True)
    resumed = private_directory(state / 'fixture-native-resume', fresh=True)
    app = build / 'Fixture.app'; app.mkdir(mode=0o755)
    def write(path, raw, mode=0o600):
        with private_binary(path) as stream: stream.write(raw)
        path.chmod(mode)
    for name, kind in native.ENTRIES.items():
        path = app / name
        if kind == 'directory': path.mkdir(mode=0o755, exist_ok=True); path.chmod(0o755)
        else: write(path, plistlib.dumps({'CFBundleIdentifier': 'dev.ortak.private20260905'})
            if name.endswith('Info.plist') else b'synthetic inert native artifact',
            0o755 if name.endswith('buzz-desktop') else 0o644)
    binary = app / 'Contents/MacOS/buzz-desktop'; binary_sha = hashlib.sha256(binary.read_bytes()).hexdigest()
    save(build / 'receipt.json', {'status': 'built_policy_verified', 'native_sha256': binary_sha, 'fixture_only': True})
    save(resumed / 'current-native-owner.json', {'schema': inventory.MAIN_SCHEMA_VERSION, 'session': 1, 'owner': {
        'pid': 1, 'sha256': binary_sha, 'executable': str(binary), 'cwd': str(state), 'inode': binary.stat().st_ino,
        'launcher': str(resumed / 'launch-native.py'),
        'launcher_sha256': native.LAUNCHER_SHA,
        'identity': f'1 {os.getuid()} fixture-start'}})
    for name in ('resume-code', 'operator-code'):
        directory = private_directory(operators / name, fresh=True)
        write(directory / 'inert.py', b'raise SystemExit("fixture must never execute")\n', 0o500)
    backend = private_directory(state / 'fixture-backend', fresh=True)
    write(backend / 'fixture-server', b'inert fixture backend', 0o500)
    repos = private_directory(state / 'repos', fresh=True)
    private_directory(repos / 'empty-project', fresh=True)
    write(repos / 'fixture.txt', b'fixture repository bytes')
    public = state / 'public.json'; save(public, {'fixture_only': True, 'generation': 1})
    secret_root = private_directory(state / 'synthetic-secrets', fresh=True)
    secret_names = ['fixture-' + str(i) for i in range(14)]
    for index, name in enumerate(secret_names):
        write(secret_root / name, ('never-valid-fixture-secret-' + str(index)).encode(), 0o444 if index == 1 else 0o600)
    # Copy committed WAL while its writer is open, then close only that fixture
    # writer. The selected cold copy deliberately has no SHM companion.
    writer_path = state / 'fixture-writer.sqlite'; writer = sqlite3.connect(writer_path)
    writer.execute('PRAGMA journal_mode=WAL')
    writer.executescript("CREATE TABLE runs(id TEXT PRIMARY KEY,status TEXT);CREATE TABLE failure_diagnostics(run_id TEXT,diagnostic TEXT);INSERT INTO runs VALUES('fixture','failed');INSERT INTO failure_diagnostics VALUES('fixture','bounded fixture diagnostic');")
    writer.commit()
    for suffix in ('', '-wal'):
        target = runtime_state / ('journal.sqlite' + suffix)
        shutil.copyfile(Path(str(writer_path) + suffix), target); target.chmod(0o600)
    writer.close()
    cold_before = {p.name: hashlib.sha256(p.read_bytes()).hexdigest() for p in runtime_state.iterdir()}
    def files():
        _, record = inventory.public_json(state, 'public.json')
        return {'public': [record], 'secret_metadata_only': [inventory.file_metadata(secret_root, n, service_readable=True) for n in secret_names]}
    images = sorted({inventory.SERVICES['controller'][2], inventory.WORKER_IMAGE})
    services = {**inventory.SERVICES, **{kind: (row['id'], row['name'], row['image'], row['volume']['name'], '/data') for kind, row in sources.items()}}
    registry = {'operation_id': operation, 'registry_sha256': 'f' * 64, 'preparation': str(operators / 'preparation.json')}
    @contextmanager
    def barrier(*_args, **_kwargs):
        yield {'fixture_only': True, 'live_admission_exercised': False,
            'databases': {'recovery_obligations': {'fixture': True}}}
    with ExitStack() as patches:
        for module, name, value in [(inventory, 'STATE', state), (inventory, 'RUNTIME', runtime),
            (inventory, 'NATIVE_RESUME', resumed), (inventory, 'SERVICES', services),
            (inventory, 'SECRET_FILES', {secret_root: secret_names}), (capture, 'files', files),
            (native, 'BUNDLE', app), (native, 'BINARY', binary), (native, 'EXPECTED_SHA', binary_sha),
            (native, 'BUILD_RECEIPT', build / 'receipt.json'), (native, 'SELECTED_PID', 1),
            (native, 'RESUME_RECEIPT', resumed / 'current-native-owner.json'),
            (native, 'SELECTED_SESSION', 1), (native, 'SELECTED_STARTED', 'fixture-start')]:
            patches.enter_context(patch.object(module, name, value))
        patches.enter_context(patch.object(native, 'candidates', return_value=[]))
        artifact = native.bundle(inventory.Inventory(private_directory(out / 'fixture-native-observation', fresh=True)))
        prepared = {'observation': {'files': files(), 'containers': {'controller': {'image': services['controller'][2]}},
            'native_processes': {'fixture': {'executable': str(backend / 'fixture-server')}},
            'native_ingress': {'artifact': artifact, 'running': False, 'process': None}}, 'plan': {'images': images}}
        save(Path(registry['preparation']), prepared); save(operators / 'owners.json', registry)
        save(operators / 'pause.json', {'fixture_only': True})
        class Tail(capture.Capture):
            def cold_stores(self):
                # Fixtures were independently identity/exit verified above;
                # the production private-stack admission is not claimed here.
                return None
            def databases(self):
                self.current()
                self.encrypted_extras = {'fixture-settings.json': b'{"fixture":true}'}
                return {'main': {'recovery_obligations': {'evidence': {'fixture': True}}}, 'fixture_only': True}
        patches.enter_context(patch.object(capture, 'load_preparation', return_value=prepared))
        patches.enter_context(patch.object(capture, 'load_registry', return_value=registry))
        patches.enter_context(patch.object(capture, 'root_pause_receipt', return_value={'fixture_only': True}))
        bundle = capture.capture(operators / 'owners.json', operators / 'pause.json', backend_type=Tail, barrier=barrier)
        manifest = json.loads((bundle / 'manifest.json').read_text())
        inventory.require(manifest['status'] == 'captured' and not (bundle / 'failure.json').exists(), 'fixture_capture_not_sealed')
        inventory.require(cold_before == {p.name: hashlib.sha256(p.read_bytes()).hexdigest() for p in runtime_state.iterdir()}, 'fixture_cold_journal_changed')
        with sqlite3.connect(bundle / 'journal.sqlite') as db:
            inventory.require(db.execute('SELECT * FROM failure_diagnostics').fetchall() == [('fixture', 'bounded fixture diagnostic')], 'fixture_journal_rows_changed')
        with tarfile.open(bundle / 'native-and-repositories.tar') as archive:
            names = set(archive.getnames())
            inventory.require({'repos/empty-project', 'repos/fixture.txt', 'resume-code/inert.py', 'operator-code/inert.py',
                'native-client/Ortak Private.app/Contents/MacOS/buzz-desktop'} <= names, 'fixture_public_archive_incomplete')
        image_proof = image_export_witness(bundle / 'images.tar', images)
        key = (state / 'recovery-keys' / (bundle.name + '.key')).read_bytes()
        sealed = (bundle / 'secrets.aesgcm').read_bytes()
        plaintext = AESGCM(key).decrypt(sealed[8:20], sealed[20:], canonical(manifest['secrets']['aad']))
        with tarfile.open(fileobj=io.BytesIO(plaintext)) as archive:
            inventory.require(len(archive.getmembers()) == 15 and all(name in archive.getnames()
                for name in ['metadata/fixture-settings.json']), 'fixture_secret_members_changed')
        inventory.require(not any((secret_root / name).read_bytes() in sealed for name in secret_names), 'fixture_plaintext_secret_leaked')
    report = {'status': 'passed', 'fixture_only': True, 'bundle': str(bundle / 'manifest.json'),
        'real_methods': ['Capture.volumes', 'Capture.journal', 'Capture.public_artifacts', 'Capture.images', 'Capture.secrets', 'capture'],
        'synthetic_boundaries': ['database_acquisition', 'live_barrier_admission', 'native_app_identity', 'source_configuration_and_credentials'],
        'source_access': False, 'provider_calls': False, 'runtime_activation': False, 'image_ids': images,
        'wal_without_shm_rows_preserved': True, 'public_archive_members_verified': True,
        'image_export_contents_verified': True, 'image_export': image_proof, 'encrypted_members': 15,
        'service_fixture': str(service_manifest), 'automatic_full_capture_claim': False}
    save(out / 'receipt.json', report)
    return out


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--service-fixture', type=Path, required=True)
    parser.add_argument('--execute-owned-fixture', action='store_true', required=True)
    args = parser.parse_args()
    try:
        output = execute(args.service_fixture)
        print(json.dumps({'status': 'passed', 'receipt': str(output / 'receipt.json'), 'source_access': False}))
    except Exception:
        raise SystemExit('Capture tail fixture failed; private generated artifacts retained; no source mutation.') from None
