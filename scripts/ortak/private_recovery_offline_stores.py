"""Fresh storage-only Docker destinations; no source mount/network/socket or application entrypoint."""

import json
from pathlib import Path
import re
import time

from backup_private_database import Commands, Refused, private_directory
from backup_private_honcho import HonchoCommands
from prepare_private_recovery import save
from private_recovery_inventory import require
import recovery_archive_io

ROLE = {'main': 'ortak', 'honcho': 'ortak_honcho'}
DATABASE = {'main': 'ortak', 'honcho': 'ortak_honcho_adapter_test'}
LABEL = 'org.ortak.offline_recovery'


def fresh_volume(command, operation, kind):
    """Only a generated empty labeled volume gains write authority; occupied names refuse."""
    require(re.fullmatch(r'[0-9a-f]{32}', operation) and kind in ('main', 'honcho', 'redis', 'minio'), 'offline_volume_scope')
    name = 'ortak_offline_' + operation + '_' + kind
    existing = command.run(kind + '-existing-volume', command.docker('volume', 'ls', '--filter',
        'name=^' + name + '$', '--format', '{{.Name}}'), ceiling=256)
    require(not existing.strip(), 'offline_volume_occupied')
    save(command.root / (kind + '-volume-intent.json'), {'name': name, 'operation_id': operation, 'kind': kind})
    created = command.run(kind + '-create-volume', command.docker('volume', 'create', '--label',
        LABEL + '=' + operation, '--label', 'org.ortak.offline_store=' + kind, name), ceiling=256)
    require(created.decode().strip() == name, 'offline_volume_create_mismatch')
    row = json.loads(command.run(kind + '-volume-owner', command.docker('volume', 'inspect', '--format',
        '{"name":{{json .Name}},"driver":{{json .Driver}},"labels":{{json .Labels}},"created_at":{{json .CreatedAt}}}', name)))
    require(row['name'] == name and row['driver'] == 'local'
        and row['labels'] == {LABEL: operation, 'org.ortak.offline_store': kind}, 'offline_volume_owner_mismatch')
    return row


def image_present(command, image, label):
    """Never pull a floating tag or trust an image name as identity."""
    require(isinstance(image, str) and re.fullmatch(r'sha256:[0-9a-f]{64}', image), 'offline_image_identity')
    value = command.run(label, command.docker('image', 'inspect', '--format', '{{.Id}}', image), ceiling=128)
    require(value.decode().strip() == image, 'offline_image_missing')


class Postgres(Commands):
    """One generated no-network PostgreSQL owner with exact new data/password mounts."""

    def __init__(self, root, operation, kind, image, password):
        super().__init__(root)
        require(kind in ROLE and re.fullmatch(r'[0-9a-f]{32}', operation), 'offline_postgres_scope')
        self.operation, self.kind, self.image, self.password = operation, kind, image, password
        self.role, self.database = ROLE[kind], DATABASE[kind]
        self.name = 'ortak-offline-' + operation + '-' + kind
        self.volume = None
        self.restore_allowed = False

    def psql(self, database):
        """Only the generated container's maintenance or exact restored database may be selected."""
        require(database in (self.database, 'postgres'), 'offline_database_scope')
        return self.command('psql', '--no-psqlrc', '--quiet', '--no-align', '--tuples-only',
            '--no-password', '--set', 'ON_ERROR_STOP=1', '-h', '/var/run/postgresql',
            '-U', self.role, '-d', database)

    def launch(self):
        """Create a retained storage service with no published ports, external network or reused volume."""
        image_present(self, self.image, 'image')
        require(self.password.is_file() and self.password.resolve() == self.password
            and self.password.stat().st_size <= 1024, 'offline_password_file_scope')
        self.volume = fresh_volume(self, self.operation, self.kind)
        save(self.root / 'container-intent.json', {'name': self.name, 'image': self.image,
            'volume': self.volume, 'operation_id': self.operation, 'network': 'none',
            'password_file': str(self.password), 'published_ports': [], 'docker_socket': False})
        args = self.docker('run', '-d', '--pull', 'never', '--name', self.name,
            '--label', LABEL + '=' + self.operation, '--label', 'org.ortak.offline_store=' + self.kind,
            '--network', 'none', '--read-only', '--cap-drop', 'ALL',
            '--security-opt', 'no-new-privileges', '--pids-limit', '64', '--memory', '384m', '--cpus', '0.5',
            '--tmpfs', '/tmp:rw,noexec,nosuid,nodev,size=64m',
            '--tmpfs', '/var/run/postgresql:rw,noexec,nosuid,nodev,size=16m',
            '--mount', 'type=volume,source=' + self.volume['name'] + ',target=/var/lib/postgresql/data,volume-nocopy',
            '--mount', 'type=bind,source=' + str(self.password) + ',target=/run/secrets/offline-postgres-password,readonly',
            '--env', 'POSTGRES_USER=' + self.role, '--env', 'POSTGRES_DB=postgres',
            '--env', 'POSTGRES_PASSWORD_FILE=/run/secrets/offline-postgres-password')
        for cap in ['CHOWN', 'DAC_OVERRIDE', 'FOWNER', 'SETUID', 'SETGID']:
            args += ['--cap-add', cap]
        identifier = self.run('start', args + [self.image], ceiling=128).decode().strip()
        require(re.fullmatch(r'[0-9a-f]{64}', identifier), 'offline_container_id_refused')
        self.container = identifier
        save(self.root / 'created-container.json', {'id': identifier, 'name': self.name,
             'image': self.image, 'operation_id': self.operation, 'volume': self.volume})
        self.inspect()
        # PID1 must have reached the final server, not initdb's temporary server.
        ready = False
        for attempt in range(100):
            self.remaining()
            command = self.command('sh', '-c', 'test "$(cat /proc/1/comm)" = postgres && pg_isready -q -h /var/run/postgresql')
            try:
                self.run('ready-' + str(attempt), command, ceiling=128)
                ready = True; break
            except Refused:
                time.sleep(0.2)
        require(ready, 'offline_postgres_not_ready')
        self.inspect()

    def inspect(self, *, running=True):
        """Recheck only this new owner; no environment or source application is inspected."""
        require(self.container is not None and self.volume is not None, 'offline_owner_missing')
        row = json.loads(self.run('owner-' + str(time.monotonic_ns()), self.docker('inspect', '--format',
            '{"id":{{json .Id}},"name":{{json .Name}},"image":{{json .Image}},"running":{{json .State.Running}},'
            '"exit_code":{{json .State.ExitCode}},"oom":{{json .State.OOMKilled}},'
            '"network":{{json .HostConfig.NetworkMode}},"ports":{{json .HostConfig.PortBindings}},'
            '"mounts":{{json .Mounts}},"labels":{{json .Config.Labels}}}', self.container)))
        expected = {('volume', self.volume['name'], '/var/lib/postgresql/data', True),
                    ('bind', str(self.password), '/run/secrets/offline-postgres-password', False)}
        # Docker Desktop exposes this exact host bind through /host_mnt inside
        # the Linux VM. Only the one recorded secret path may use that alias;
        # never strip an arbitrary prefix from a discovered mount.
        mounts = {(m['Type'], m.get('Name') if m['Type'] == 'volume' else
                   str(self.password) if m['Source'] == '/host_mnt' + str(self.password) else m['Source'],
                   m['Destination'], m['RW']) for m in row['mounts']}
        require(row['id'] == self.container and row['name'] == '/' + self.name and row['image'] == self.image
            and row['running'] is running and not row['oom'] and (running or row['exit_code'] == 0)
            and row['network'] == 'none' and not row['ports'] and mounts == expected
            and row['labels'].get(LABEL) == self.operation
            and row['labels'].get('org.ortak.offline_store') == self.kind, 'offline_owner_changed')
        return {'id': self.container, 'image': self.image, 'name': self.name, 'volume': self.volume,
                'network': 'none', 'published_ports': [], 'source_mounts': False, 'docker_socket': False,
                'running': running}

    def create_database(self, settings):
        """An empty template0 destination is created once; unsupported locale/settings refuse."""
        self.inspect()
        database = settings['database']
        require(database['owner'] == self.role and database['tablespace'] == 'pg_default'
            and database['locale_provider'] == 'c' and database['encoding'] == 'UTF8'
            and type(database['connection_limit']) is int and -1 <= database['connection_limit'] <= 10000
            and all(re.fullmatch(r'[A-Za-z0-9_.@-]+', database[key]) for key in ['collation', 'ctype']),
            'offline_database_attributes_unsupported')
        self.run('create-database', self.command('createdb', '--no-password', '-h', '/var/run/postgresql',
            '-U', self.role, '--maintenance-db=postgres', '--template=template0', '--owner=' + self.role,
            '--encoding=UTF8', '--lc-collate=' + database['collation'], '--lc-ctype=' + database['ctype'],
            self.database), ceiling=128)
        if database['connection_limit'] != -1:
            self.run('database-connection-limit', self.psql('postgres'), sql='ALTER DATABASE "' + self.database
                + '" WITH CONNECTION LIMIT ' + str(database['connection_limit']) + ';', ceiling=128)
        self.restore_allowed = True

    def restore(self, archive, *, source_checks=None):
        """Only the once-created owned database receives a verified archive; partial restores remain retained."""
        self.inspect()
        require(self.restore_allowed, 'offline_fresh_database_authority_required')
        self.restore_allowed = False
        if self.kind == 'main':
            from private_restore_credential_functions import restore_sections
            return restore_sections(self, self.database, archive)
        self.run('restore', self.command('pg_restore', '--no-password', '--exit-on-error', '--single-transaction',
            '-h', '/var/run/postgresql', '-U', self.role, '-d', self.database), archive=archive)
        from private_restore_honcho_checks import repair_checks
        self.honcho_check_restore_authority = self.database
        return repair_checks(self, self.database, source_checks)

    def restored_metadata(self):
        """Bind the original production catalog/count/content checks to this isolated destination."""
        if self.kind == 'main': return Commands.metadata(self, self.database, 'restored')
        with HonchoCommands.snapshot(self) as snapshot:
            return HonchoCommands.metadata(self, self.database, 'restored', snapshot)

    def stop_retained(self):
        """Gracefully stop only this generated offline owner; its container/database/volume remain retained."""
        self.inspect()
        stopped = self.run('stop-retained', self.docker('stop', '--timeout', '30', self.container), ceiling=128)
        require(stopped.decode().strip() == self.container, 'offline_stop_confirmation')
        result = self.inspect(running=False)
        save(self.root / 'stopped-retained.json', result)
        return result


def restore_volume(root, operation, kind, image, archive, maximum):
    """Extract and re-read one complete tree on a new owned volume, with no service startup."""
    require(kind in ('redis', 'minio'), 'offline_volume_kind')
    command = Commands(root)
    image_present(command, image, 'image')
    volume = fresh_volume(command, operation, kind)
    name = 'ortak-offline-' + operation + '-' + kind + '-restore'
    script = Path(recovery_archive_io.__file__).read_text()
    save(root / 'reader-intent.json', {'name': name, 'image': image, 'volume': volume,
        'network': 'none', 'docker_socket': False, 'source_mounts': False})
    args = command.docker('run', '--pull', 'never', '--name', name, '--label', LABEL + '=' + operation,
        '--network', 'none', '--read-only', '--user', '0:0', '--cap-drop', 'ALL',
        '--cap-add', 'CHOWN', '--cap-add', 'DAC_OVERRIDE', '--cap-add', 'FOWNER',
        '--security-opt', 'no-new-privileges', '--pids-limit', '16', '--memory', '256m',
        '--mount', 'type=volume,source=' + volume['name'] + ',target=/restore-target,volume-nocopy',
        '--entrypoint', '/usr/local/bin/python', '-i', image, '-u', '-c', script, str(maximum))
    result = json.loads(command.run('extract-verify', args, archive=archive, ceiling=1024))
    require(result.pop('status') == 'verified', 'offline_volume_restore_failed')
    with archive.open('rb') as stream: expected = recovery_archive_io.archive(stream, maximum)
    require(result == expected, 'offline_volume_tree_mismatch')
    return {'volume': volume, 'retained_reader': name, 'image': image, **result,
            'network': 'none', 'service_started': False, 'application_semantics_verified': False}
