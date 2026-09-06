#!/usr/bin/env python3
"""Fresh Redis/MinIO semantic fixtures; never open a source volume, credential or service."""

import argparse
import hashlib
import json
from pathlib import Path
import re
import tarfile
import time
from uuid import uuid4

from backup_private_database import Commands, Refused, digest, private_binary, private_directory
from prepare_private_recovery import save, sha
import private_recovery_inventory as inventory
from private_recovery_offline_stores import LABEL, fresh_volume, image_present, restore_volume
from private_recovery_payloads import VOLUME_READER
import recovery_archive_io
import recovery_minio_client_fixture

MAXIMUM = 256 * 1024**2
REDIS_ARGS = ['redis-server', '--appendonly', 'yes', '--appendfsync', 'always',
    '--aof-load-truncated', 'no', '--save', '', '--bind', '127.0.0.1', '--port', '6379']
KEYS = ['fixture:persistent', 'fixture:expires', 'fixture:survives', 'fixture:counter', 'fixture:metadata']


class Service(Commands):
    """Only a newly generated labeled volume and synthetic credential leaves gain service authority."""

    def __init__(self, root, operation, kind, volume, secrets=None):
        super().__init__(root)
        inventory.require(re.fullmatch(r'[0-9a-f]{32}', operation) and kind in ('redis', 'minio')
            and volume['name'] == 'ortak_offline_' + operation + '_' + kind
            and volume['labels'] == {LABEL: operation, 'org.ortak.offline_store': kind}, 'service_fixture_scope')
        self.operation, self.kind, self.volume = operation, kind, volume
        self.image = inventory.SERVICES[kind][2]
        self.name = 'ortak-offline-services-' + operation + '-' + kind
        self.secrets = secrets or {}
        inventory.require(set(self.secrets) == ({'user', 'password'} if kind == 'minio' else set()), 'fixture_secret_scope')
        self.sequence = 0

    def launch_args(self):
        """No published ports, routes, socket, source path, runtime entrypoint or ambient credential."""
        args = self.docker('run', '-d', '--pull', 'never', '--name', self.name,
            '--label', LABEL + '=' + self.operation, '--label', 'org.ortak.offline_store=' + self.kind,
            '--network', 'none', '--read-only', '--user', '10001:10001', '--cap-drop', 'ALL',
            '--security-opt', 'no-new-privileges', '--pids-limit', '128', '--memory', '1g', '--cpus', '1',
            '--tmpfs', '/tmp:rw,noexec,nosuid,nodev,size=64m',
            '--mount', 'type=volume,source=' + self.volume['name'] + ',target=/data,volume-nocopy')
        for key, path in sorted(self.secrets.items()):
            args += ['--mount', 'type=bind,source=' + str(path) + ',target=/run/secrets/fixture-minio-' + key + ',readonly',
                '--env', 'MINIO_ROOT_' + key.upper() + '_FILE=/run/secrets/fixture-minio-' + key]
        if self.kind == 'minio': args += ['--env', 'MINIO_BROWSER=off', '--env', 'MINIO_UPDATE=off']
        return args + [self.image] + (REDIS_ARGS if self.kind == 'redis' else
            ['server', '/data', '--address', '127.0.0.1:9000'])

    def launch(self):
        """Create once and immediately retain immutable identity before any readiness check."""
        image_present(self, self.image, 'image')
        save(self.root / 'intent.json', {'name': self.name, 'image': self.image, 'volume': self.volume,
            'network': 'none', 'published_ports': [], 'source_access': False})
        identifier = self.run('start', self.launch_args(), ceiling=128).decode().strip()
        inventory.require(re.fullmatch(r'[0-9a-f]{64}', identifier), 'fixture_container_id')
        self.container = identifier
        save(self.root / 'created.json', {'id': identifier, 'name': self.name, 'operation': self.operation,
            'image': self.image, 'volume': self.volume})
        self.inspect()

    def inspect(self, *, running=True):
        """Exact new UUID, image, mounts, labels, isolation and graceful exit are all required."""
        inventory.require(self.container is not None, 'fixture_owner_missing')
        row = json.loads(self.run('owner-' + str(time.monotonic_ns()), self.docker('inspect', '--format',
            '{"id":{{json .Id}},"name":{{json .Name}},"image":{{json .Image}},"running":{{json .State.Running}},'
            '"exit":{{json .State.ExitCode}},"oom":{{json .State.OOMKilled}},"network":{{json .HostConfig.NetworkMode}},'
            '"ports":{{json .HostConfig.PortBindings}},"readonly":{{json .HostConfig.ReadonlyRootfs}},'
            '"privileged":{{json .HostConfig.Privileged}},"mounts":{{json .Mounts}},"labels":{{json .Config.Labels}}}', self.container)))
        aliases = {'/host_mnt' + str(path): str(path) for path in self.secrets.values()}
        mounts = {(m['Type'], m.get('Name') if m['Type'] == 'volume' else aliases.get(m['Source'], m['Source']),
            m['Destination'], m['RW']) for m in row['mounts']}
        expected = {('volume', self.volume['name'], '/data', True)} | {
            ('bind', str(path), '/run/secrets/fixture-minio-' + key, False) for key, path in self.secrets.items()}
        inventory.require(row['id'] == self.container and row['name'] == '/' + self.name and row['image'] == self.image
            and row['running'] is running and not row['oom'] and (running or row['exit'] == 0)
            and row['network'] == 'none' and not row['ports'] and row['readonly'] and not row['privileged']
            and mounts == expected and row['labels'].get(LABEL) == self.operation
            and row['labels'].get('org.ortak.offline_store') == self.kind, 'fixture_owner_changed')
        return {'id': self.container, 'name': self.name, 'image': self.image, 'volume': self.volume,
            'running': running, 'network': 'none', 'published_ports': [], 'source_access': False}

    def stop_retained(self):
        """Only this verified generated service may stop, with a finite fresh cleanup budget."""
        self.deadline = time.monotonic() + 45
        self.inspect()
        value = self.run('stop-' + str(time.monotonic_ns()), self.docker('stop', '--timeout', '30', self.container), ceiling=128)
        inventory.require(value.decode().strip() == self.container, 'fixture_stop_confirmation')
        result = self.inspect(running=False)
        save(self.root / 'stopped.json', result)
        return result

    def redis(self, *args):
        """Built-in redis-cli accesses only this new namespace's loopback; no source endpoint."""
        inventory.require(self.kind == 'redis', 'fixture_redis_kind')
        self.sequence += 1
        value = self.run('redis-' + str(self.sequence), self.docker('exec', '--user', '10001:10001', self.container,
            'timeout', '5', 'redis-cli', '--raw', '-h', '127.0.0.1', '-p', '6379', *args), ceiling=4096)
        return value.decode().strip()


def initialize_volume(command, volume, operation, kind):
    """Ownership changes are limited to one brand-new empty labeled Linux volume."""
    script = "import os;from pathlib import Path;r=Path('/new-data');assert not any(r.iterdir());os.chown(r,10001,10001);r.chmod(0o700)"
    command.run('initialize', command.docker('run', '--pull', 'never', '--name', 'ortak-offline-init-' + operation + '-' + kind,
        '--label', LABEL + '=' + operation, '--network', 'none', '--read-only', '--user', '0:0', '--cap-drop', 'ALL',
        '--cap-add', 'CHOWN', '--cap-add', 'FOWNER', '--cap-add', 'DAC_OVERRIDE', '--security-opt', 'no-new-privileges',
        '--pids-limit', '16', '--memory', '64m', '--mount', 'type=volume,source=' + volume['name'] + ',target=/new-data,volume-nocopy',
        '--entrypoint', '/usr/local/bin/python', inventory.WORKER_IMAGE, '-c', script), ceiling=128)


def cold_archive(service, output):
    """The stopped fixture's only cold volume is read; production volumes never enter this path."""
    service.inspect(running=False)
    command = Commands(output)
    target = output / 'volume.tar'
    args = command.docker('run', '--pull', 'never', '--name', service.name + '-reader', '--label', LABEL + '=' + service.operation,
        '--network', 'none', '--read-only', '--user', '0:0', '--cap-drop', 'ALL', '--cap-add', 'DAC_OVERRIDE',
        '--security-opt', 'no-new-privileges', '--pids-limit', '16', '--memory', '128m',
        '--mount', 'type=volume,source=' + service.volume['name'] + ',target=/capture-source,readonly,volume-nocopy',
        '--entrypoint', '/usr/local/bin/python', inventory.SERVICES['controller'][2], '-u', '-c', VOLUME_READER, str(MAXIMUM))
    command.run('cold-volume', args, output=target, ceiling=MAXIMUM + 1024**2)
    with target.open('rb') as stream: tree = recovery_archive_io.archive(stream, MAXIMUM)
    if service.kind == 'minio':
        with tarfile.open(target) as archive:
            metadata = recovery_archive_io.archive_xattrs(archive.getmember('.minio.sys/format.json').pax_headers)
        inventory.require(set(metadata) == set(recovery_archive_io.XATTRS), 'fixture_minio_counters_not_persisted')
        tree['minio_counter_xattrs_sha256'] = sha(metadata)
        tree['minio_counter_xattrs'] = {key: 8 for key in metadata}
    service.inspect(running=False)
    return target, {'sha256': digest(target), 'archive_bytes': target.stat().st_size, **tree}


def redis_seed(service):
    """AOF contains durable data, a counter, a hash and absolute expirations spanning downtime."""
    for _ in range(25):
        try:
            if service.redis('PING') == 'PONG': break
        except Refused: pass
        time.sleep(0.2)
    else: raise Refused('fixture_redis_not_ready')
    inventory.require(service.redis('DBSIZE') == '0', 'fixture_redis_not_empty')
    for key in KEYS[:3]: inventory.require(service.redis('SET', key, 'fixture-value') == 'OK', 'fixture_set_failed')
    inventory.require(service.redis('INCR', KEYS[3]) == '1' and service.redis('INCR', KEYS[3]) == '2', 'fixture_counter_failed')
    inventory.require(service.redis('HSET', KEYS[4], 'generation', '2', 'source', 'synthetic') == '2', 'fixture_hash_failed')
    seconds, micros = map(int, service.redis('TIME').splitlines()); now = seconds * 1000 + micros // 1000
    expected = {'short_expiry_ms': now + 2000, 'long_expiry_ms': now + 300000}
    for key, expiry in [(KEYS[1], expected['short_expiry_ms']), (KEYS[2], expected['long_expiry_ms'])]:
        inventory.require(service.redis('PEXPIREAT', key, str(expiry)) == '1', 'fixture_expiry_failed')
    inventory.require(service.redis('DBSIZE') == '5', 'fixture_seed_count')
    return expected


def redis_verify(service, expected):
    """Expiry is measured against server time; restart must not extend TTL or replay INCR twice."""
    for _ in range(25):
        try:
            if service.redis('PING') == 'PONG': break
        except Refused: pass
        time.sleep(0.2)
    else: raise Refused('fixture_restored_redis_not_ready')
    seconds, micros = map(int, service.redis('TIME').splitlines()); now = seconds * 1000 + micros // 1000
    inventory.require(now > expected['short_expiry_ms'] and service.redis('EXISTS', KEYS[1]) == '0'
        and service.redis('GET', KEYS[0]) == 'fixture-value' and service.redis('GET', KEYS[2]) == 'fixture-value'
        and service.redis('GET', KEYS[3]) == '2' and service.redis('HGET', KEYS[4], 'generation') == '2'
        and service.redis('HGET', KEYS[4], 'source') == 'synthetic' and service.redis('DBSIZE') == '4', 'fixture_aof_data_mismatch')
    ttl = int(service.redis('PTTL', KEYS[2]))
    inventory.require(0 < ttl <= expected['long_expiry_ms'] - now and
        abs(int(service.redis('PEXPIRETIME', KEYS[2])) - expected['long_expiry_ms']) == 0, 'fixture_ttl_extended')
    info = dict(line.split(':', 1) for line in service.redis('INFO', 'persistence').splitlines() if ':' in line)
    inventory.require(info.get('aof_enabled') == '1' and info.get('aof_last_write_status') == 'ok'
        and info.get('aof_last_bgrewrite_status') == 'ok', 'fixture_aof_health')
    return {'keys': 4, 'expired_absent': True, 'counter': 2, 'hash_equal': True,
        'absolute_expiry_preserved': True, 'remaining_ttl_ms': ttl, 'aof_truncation_repair': False}


def minio_client(service, mode, expected=None):
    """Installed curl signs inside only this verified fixture's network-none namespace."""
    service.inspect()
    command = Commands(private_directory(service.root / ('client-' + mode), fresh=True))
    args = command.docker('run', '--pull', 'never', '--name', service.name + '-client-' + mode,
        '--label', LABEL + '=' + service.operation, '--network', 'container:' + service.container,
        '--read-only', '--user', '10001:10001', '--cap-drop', 'ALL', '--security-opt', 'no-new-privileges',
        '--pids-limit', '16', '--memory', '128m', '--tmpfs', '/tmp:rw,noexec,nosuid,nodev,size=64m')
    for key, path in service.secrets.items():
        args += ['--mount', 'type=bind,source=' + str(path) + ',target=/run/secrets/fixture-minio-' + key + ',readonly']
    request = command.root / 'request.json'; save(request, {'mode': mode, 'expected': expected})
    # Public code is argv; generated fixture secrets travel over the curl child's stdin only.
    args += ['--entrypoint', '/usr/local/bin/python', '-i', inventory.WORKER_IMAGE, '-u', '-c',
        Path(recovery_minio_client_fixture.__file__).read_text()]
    result = json.loads(command.run('s3', args, archive=request, output=command.root / 'response.json', ceiling=8192)
        or (command.root / 'response.json').read_bytes())
    inventory.require(result['status'] == 'verified' and result['real_source_keys'] is False
        and result['writes_performed'] is (mode == 'seed'), 'fixture_s3_verification_failed')
    service.inspect()
    return result


def execute():
    """Sequential fresh seed→stop→archive→new volume→verify→stop, with durable retained failures."""
    inventory.directory(inventory.STATE)
    root = private_directory(private_directory(inventory.STATE / 'recovery-service-fixtures') / uuid4().hex, fresh=True)
    report = {'format': 'ortak-offline-service-semantics/1', 'status': 'started', 'operation': root.name,
        'source_access': False, 'provider_requests': False, 'office_requests': False, 'docker_socket_mounted': False,
        'published_ports': [], 'production_bundle_restored': False, 'runtime_activation': False}
    save(root / 'intent.json', report)
    services = []
    try:
        image_present(Commands(root), inventory.WORKER_IMAGE, 'helper-image')
        image_present(Commands(root), inventory.SERVICES['controller'][2], 'capture-reader-image')
        for kind in ('redis', 'minio'):
            output = private_directory(root / kind, fresh=True)
            seeds = {}
            if kind == 'minio':
                secret_root = private_directory(output / 'synthetic-secrets', fresh=True)
                for key, value in [('user', uuid4().hex), ('password', uuid4().hex + uuid4().hex)]:
                    path = secret_root / key
                    with private_binary(path) as stream: stream.write(value.encode())
                    # This synthetic leaf is shared with UID10001 behind an owner-private parent.
                    path.chmod(0o444); seeds[key] = path
            seed_id = uuid4().hex
            seed_root = private_directory(output / 'seed', fresh=True)
            seed_command = Commands(seed_root)
            volume = fresh_volume(seed_command, seed_id, kind)
            initialize_volume(seed_command, volume, seed_id, kind)
            source = Service(seed_root, seed_id, kind, volume, seeds); services.append(source); source.launch()
            expected = redis_seed(source) if kind == 'redis' else minio_client(source, 'seed')['expected']
            save(output / 'expected.json', expected)
            stopped = source.stop_retained()
            archive, archive_proof = cold_archive(source, private_directory(output / 'cold', fresh=True))
            restored = restore_volume(private_directory(output / 'restore-volume', fresh=True), root.name,
                kind, inventory.WORKER_IMAGE, archive, MAXIMUM)
            destination = Service(private_directory(output / 'restored', fresh=True), root.name, kind, restored['volume'], seeds)
            services.append(destination)
            if kind == 'redis':
                wait = max(0, expected['short_expiry_ms'] / 1000 - time.time() + 0.1)
                inventory.require(wait <= 3, 'fixture_expiry_clock_skew'); time.sleep(wait)
            destination.launch()
            proof = redis_verify(destination, expected) if kind == 'redis' else minio_client(destination, 'verify', expected)['verification']
            report[kind] = {'seed': stopped, 'cold_archive': archive_proof, 'restored_tree': restored,
                'semantics': proof, 'restored': destination.stop_retained(), 'synthetic_fixture_only': True}
            save(output / 'proof.json', report[kind])
        report['status'] = 'verified'
    except Exception as error:
        report.update(status='failed', error_type=type(error).__name__,
            error_code=str(error) if isinstance(error, Refused) else 'fixture_failed')
    finally:
        for service in services:
            if service.container is not None and not (service.root / 'stopped.json').exists():
                try: service.stop_retained()
                except Exception as error:
                    report.update(status='failed', cleanup_required=True, cleanup_error=type(error).__name__)
        report['manifest_sha256'] = sha(report)
        save(root / 'manifest.json', report)
    inventory.require(report['status'] == 'verified', 'service_fixture_failed_retained_at_' + root.name)
    return root


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--execute-owned-fixtures', action='store_true', required=True)
    parser.parse_args()
    try:
        output = execute()
        print(json.dumps({'status': 'verified', 'manifest': str(output / 'manifest.json'), 'source_access': False}))
    except Refused as error:
        print(json.dumps({'status': 'refused', 'code': str(error)}))
        raise SystemExit(3) from None
