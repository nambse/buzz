#!/usr/bin/env python3
"""Check a root-coordinated pause under a real Linux lease; never pause or resume source services."""

from contextlib import contextmanager
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import stat
import subprocess
import sys
import time
from uuid import uuid4

from backup_private_database import Commands, Refused, environment, private_directory
from backup_private_honcho import HonchoCommands, SOURCE as HONCHO_DATABASE
from prepare_private_recovery import files, load_preparation, save, sha
from private_native_services import private_file
from register_private_recovery import FORMAT, OPERATOR_FILES
from private_recovery_schema_lease import held_schema, response
import private_recovery_inventory as inventory
import private_recovery_obligations as obligations
import recovery_lock_holder
import private_recovery_journal as selected_journal
from private_recovery_workspace_capture import HeldBarrierWitness
import recovery_native_ingress as native_ingress
import private_recovery_scorer as scorer

PID_SCAN = r"""import json,subprocess,sys
try:
 p=subprocess.Popen(['/usr/bin/pgrep','-x',sys.argv[1]],stdout=subprocess.PIPE,stderr=subprocess.DEVNULL)
 try:
  raw=p.stdout.read(1025)
  if len(raw)>1024: raise ValueError()
  code=p.wait(timeout=2)
  ids=raw.decode().split()
  if code not in (0,1) or len(ids)>8 or any(not x.isdigit() for x in ids): raise ValueError()
  if code==1 and ids: raise ValueError()
  print(json.dumps(ids))
 finally:
  if p.poll() is None: p.kill()
  p.wait(timeout=2)
except Exception:
 sys.exit(3)
"""
MAIN_DRAIN_SQL = "BEGIN READ ONLY; SELECT jsonb_build_object(" + ','.join(
    "'" + name + "',(SELECT count(*) FROM " + table + " WHERE company_id='" + inventory.COMPANY + "' AND " + predicate + ')'
    for name, table, predicate in [
        ('active_runs', 'runs', "status NOT IN ('completed','failed','cancelled')"),
        ('pending_cancellation', 'runtime_cancellations', "state='pending'"),
        ('pending_cancel_request', 'run_cancel_requests', "status='pending'"),
        ('pending_outbox', 'outbox', "state='pending'"),
        ('pending_office_output', 'runtime_office_outputs', "state='pending'"),
        ('pending_memory_write', 'runtime_memory_writes', "state='pending'"),
        ('pending_work_output', 'runtime_work_outputs', "state='pending'"),
        ('pending_management_command', 'employee_management_commands', "status IN ('pending','running')"),
    ]) + ", 'application_clients',(SELECT count(*) FROM pg_stat_activity WHERE datname=current_database() AND backend_type='client backend' AND pid<>pg_backend_pid())); ROLLBACK;"
HONCHO_CLIENT_SQL = "BEGIN READ ONLY; SELECT count(*) FROM pg_stat_activity WHERE datname=current_database() AND backend_type='client backend' AND pid<>pg_backend_pid(); ROLLBACK;"


def load_registry(path):
    """Only an immutable, marked exact operation owns future pause/capture checks."""
    inventory.require(path.name == 'owners.json' and path.parent.parent == inventory.STATE / 'recovery-operations'
        and re.fullmatch(r'[0-9a-f]{32}', path.parent.name), 'owner_registry_path_refused')
    row, _ = inventory.public_json(path.parent, path.name)
    expected = row.pop('registry_sha256')
    inventory.require(row['format'] == FORMAT and row['status'] == 'registered'
        and row['operation_id'] == path.parent.name and sha(row) == expected, 'owner_registry_integrity_refused')
    row['registry_sha256'] = expected
    inventory.native_writer_set(row['owners'])
    inventory.native_writer_set(row['resume_recipes'])
    inventory.require(set(row.get('operator_code', {})) == set(OPERATOR_FILES), 'operator_code_inventory_incomplete')
    all_sources = [(source, 'resume-code') for source in row['source_code'].values()] + [
        (source, 'operator-code') for source in row['operator_code'].values()]
    for source, directory in all_sources:
        frozen = source['frozen']
        path = Path(frozen['path'])
        inventory.require(path.parent == inventory.STATE / 'recovery-operations' / row['operation_id'] / directory,
            'frozen_code_scope_refused')
        data = path.lstat()
        inventory.require(stat.S_ISREG(data.st_mode) and data.st_uid == os.getuid() and data.st_nlink == 1
            and stat.S_IMODE(data.st_mode) == 0o500 and data.st_size == frozen['bytes'], 'frozen_code_metadata_refused')
        inventory.require(hashlib.sha256(private_file(path).encode()).hexdigest() == frozen['sha256'], 'frozen_code_changed')
    inventory.require(Path(__file__).resolve() == Path(row['operator_code']['check_private_recovery_gate.py']['frozen']['path']),
        'execute_selected_frozen_operator_required')
    return row


class Gate:
    """Fail closed on running owners, pending work, credentials generation drift or unknown clients."""

    def __init__(self, output, registry):
        self.output, self.registry = output, registry
        self.inspector = inventory.Inventory(output)
        self.command = self.inspector.commands
        self.command.deadline = time.monotonic() + 900
        self.preparation = load_preparation(Path(registry['preparation']))

    def stopped_owners(self):
        """Rediscover all matching native candidates; process absence never follows from old PID alone."""
        expected = self.preparation['observation']
        inventory.native_writer_set(self.registry['owners'])
        inventory.native_writer_set(expected['native_processes'])
        inventory.require(files() == expected['files'], 'paused_configuration_generation_changed')
        native_ingress.require_stopped(self.inspector, expected['native_ingress'])
        for name in self.registry['owners']:
            ids = json.loads(self.inspector.run([sys.executable, '-c', PID_SCAN, name], limit=1024))
            for pid in ids:
                cwd = self.inspector.run(['/usr/sbin/lsof', '-a', '-p', pid, '-d', 'cwd', '-Fn'], limit=4096).decode().splitlines()
                inventory.require('n' + str(inventory.STATE) not in cwd, 'private_native_writer_still_running')
        for name in ['controller', 'honcho_api']:
            current = self.inspector.container(name)
            old = expected['containers'][name]
            inventory.require(not current['running'] and all(current[key] == old[key]
                for key in ['id', 'name', 'image', 'mounts', 'started_at', 'user']), 'application_container_not_stopped')
            state = json.loads(self.inspector.run(self.command.docker('inspect', '--format',
                '{"exit_code":{{json .State.ExitCode}},"oom":{{json .State.OOMKilled}},"pid":{{json .State.Pid}},"restarting":{{json .State.Restarting}}}', current['id']), limit=1024))
            inventory.require(state['exit_code'] in (0,143) and not state['oom'] and state['pid'] == 0
                              and not state['restarting'], 'unclean_application_stop')
        for name in ['postgres', 'honcho_postgres']:
            current = self.inspector.container(name)
            inventory.require(current == expected['containers'][name] and current['running'], 'store_authority_changed')
        for name in ['redis', 'minio']:
            current = self.inspector.container(name)
            old = expected['containers'][name]
            inventory.require(all(current[key] == old[key] for key in
                ['id', 'name', 'image', 'mounts', 'started_at', 'user', 'volume']), 'cold_store_authority_changed')
            if not current['running']:
                state = json.loads(self.inspector.run(self.command.docker('inspect', '--format',
                    '{"exit_code":{{json .State.ExitCode}},"oom":{{json .State.OOMKilled}},"pid":{{json .State.Pid}}}', current['id']), limit=1024))
                inventory.require(state['exit_code'] == 0 and not state['oom'] and state['pid'] == 0,
                                  'cold_store_not_gracefully_stopped')
        children = self.inspector.children()
        inventory.require(not any(row['running'] for row in children), 'contained_worker_still_running')
        self.scorer_stopped = scorer.stopped(self.inspector, expected.get('scorer_owner'))
        return children

    def drained_databases(self):
        """Pending side effects and any other connected client refuse a quiescence witness."""
        main = Commands(private_directory(self.output / ('main-' + uuid4().hex), fresh=True))
        main.deadline=self.command.deadline
        main.inspect()
        sql = MAIN_DRAIN_SQL
        if getattr(self, 'schema_owner', None):
            owner = self.schema_owner
            pid = owner['backend_pid']
            started = owner['backend_start'].replace("'", "''")
            held = int(main.run('schema-owner', main.psql('ortak'), ceiling=128,
                sql="SELECT count(*) FROM pg_stat_activity WHERE pid=" + str(pid)
                + " AND backend_start='" + started + "'::timestamptz AND state='idle in transaction';"))
            inventory.require(held == 1, 'schema_lease_not_current')
            sql = sql.replace('pid<>pg_backend_pid()', 'pid<>pg_backend_pid() AND pid<>' + str(pid))
        counters = json.loads(main.run('drain', main.psql('ortak'), sql=sql, ceiling=4096))
        inventory.require(counters and all(type(n) is int and n == 0 for n in counters.values()), 'main_database_not_drained')
        metadata = main.metadata('ortak', 'schema')
        previous = self.preparation['observation']['main_database']
        inventory.require(metadata['schema_sha256'] == previous['schema_sha256']
            and metadata['migration_checksums'] == previous['migration_checksums'], 'main_schema_authority_changed')
        obligations.workspaces.require_capture_selection(metadata, self.preparation['observation'].get('workspace_selection'), inventory.COMPANY)
        retained = obligations.observe(main, 'ortak', metadata, inventory.COMPANY, drained=True)
        obligations.workspaces.require_capture_scope(metadata,retained)
        honcho = HonchoCommands(private_directory(self.output / ('honcho-' + uuid4().hex), fresh=True))
        honcho.deadline=self.command.deadline
        honcho.container = inventory.SERVICES['honcho_postgres'][0]  # Already verified by stopped_owners().
        count = int(honcho.run('clients', honcho.psql(HONCHO_DATABASE), sql=HONCHO_CLIENT_SQL, ceiling=128))
        inventory.require(count == 0, 'honcho_application_clients_remain')
        return {'main': counters, 'honcho_application_clients': count, 'recovery_obligations': retained}


def lease_args(command, name, image, script):
    """A dedicated Linux lock holder gets no network, Docker socket or application entrypoint."""
    return command.docker('run', '--pull', 'never', '--init', '--name', name, '--label', 'org.ortak.recovery_lease=' + name,
        '--network', 'none', '--read-only', '--user', '10001:10001', '--cap-drop', 'ALL',
        '--security-opt', 'no-new-privileges', '--pids-limit', '16', '--memory', '256m', '--cpus', '0.25',
        '--tmpfs', '/recovery-working:rw,noexec,nosuid,nodev,size=150m,mode=700,uid=10001,gid=10001',
        '--mount', selected_journal.source_mount(inventory.RUNTIME, inventory.JOURNAL_VOLUME),
        '--mount', 'type=bind,source=' + str(inventory.RUNTIME / 'oauth') + ',target=' + str(inventory.RUNTIME / 'oauth') + ',readonly',
        '--entrypoint', '/usr/local/bin/python', '-i', image, '-u', '-c', script)


@contextmanager
def held_barrier(output, registry, *, pause_receipt, gate_type=Gate):
    """A capture caller must remain inside this live context; a saved check is never authority."""
    pause = root_pause_receipt(pause_receipt, registry)
    gate = gate_type(output, registry)
    gate.stopped_owners()  # No new helper container is created while live owners remain.
    image = gate.preparation['observation']['containers']['controller']['image']
    name = 'ortak-recovery-lease-' + uuid4().hex
    protected=selected_journal.require_confidential_schema(
        gate.preparation['observation'].get('journal_confidential'),inventory.MAIN_SCHEMA_VERSION)
    raw = selected_journal.lease_script(recovery_lock_holder,confidential_reviewed=protected)
    inventory.require(len(raw) <= 98304, 'lease_script_bound')
    save(output / 'lease-intent.json', {'container_name': name, 'image': image,
        'script_sha256': hashlib.sha256(raw).hexdigest(), 'source_stop_performed': False,
        'host_oauth_enrollment_must_remain_root_fenced': True, 'network': 'none', 'docker_socket': False})
    process = subprocess.Popen(lease_args(gate.command, name, image, raw.decode()), stdin=subprocess.PIPE,
        stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, env=environment(), start_new_session=True)
    completed = False
    live = None
    try:
        witness = response(process, gate.command)
        inventory.require(witness.get('status') == 'held', 'linux_lease_not_held')
        with held_schema(private_directory(output / 'schema-lease', fresh=True)) as schema_owner:
            gate.schema_owner = schema_owner
            gate.stopped_owners()
            counters = gate.drained_databases()
            if gate.preparation['observation'].get('workspace_selection') is None:
                inventory.require(all(witness['journal'].get('workspace',{}).get(k,0)==0
                    for k in ('workspace_runs','workspace_tool_calls')),'workspace_capture_selection_required')
            value={'linux_lease': witness, 'schema_lease': schema_owner, 'databases': counters,
                'container_name': name, 'lease_reusable_after_context': False, 'automatic_activation': False}
            if getattr(gate,'scorer_stopped',None) is not None:value['scorer']=gate.scorer_stopped
            live = HeldBarrierWitness(value, gate, process)
            yield live
            live.active=False
            gate.stopped_owners()
            inventory.require(gate.drained_databases() == counters, 'drain_generation_changed')
            inventory.require(root_pause_receipt(pause_receipt, registry) == pause, 'root_pause_receipt_changed')
            process.stdin.write(b'release\n'); process.stdin.flush()
            inventory.require(response(process, gate.command) == {'status': 'released'}, 'linux_lease_release_failed')
            inventory.require(process.wait(timeout=min(3, gate.command.remaining())) == 0, 'linux_lease_process_failed')
        completed = True
    finally:
        if live is not None: live.active=False
        gate.command.stop(process)
        # The helper has its own900-second lease timeout even after CLI loss.
        # No source service or container is killed here; its retained name and
        # pending expiration remain visible for operator reconciliation.
        save(output / 'lease-finish.json', {'container_name': name, 'released_acknowledged': completed,
            'helper_retained': True, 'source_service_actions': False,
            'unacknowledged_helper_maximum_seconds': 900 if not completed else 0})


def root_pause_receipt(path, registry):
    """Root supplies this attestation only after its coordinated pause; live facts are still verified."""
    inventory.require(path == inventory.STATE / 'recovery-operations' / registry['operation_id'] / 'pause.json',
                      'pause_receipt_scope_refused')
    value, metadata = inventory.public_json(path.parent, path.name)
    inventory.require(set(value) == {'format', 'owners_sha256', 'host_oauth_enrollment_fenced',
        'root_coordinated_pause', 'resume_under_root_control'}
        and value['format'] == 'ortak-private-recovery-pause/1'
        and value['owners_sha256'] == registry['registry_sha256']
        and value['host_oauth_enrollment_fenced'] is True
        and value['root_coordinated_pause'] is True and value['resume_under_root_control'] is True,
        'root_pause_attestation_required')
    return metadata


def main():
    """Check only after root's coordinated pause; this command never creates the pause."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--owners', type=Path, required=True)
    parser.add_argument('--pause-receipt', type=Path, required=True)
    args = parser.parse_args()
    registry = load_registry(args.owners)
    pause = root_pause_receipt(args.pause_receipt, registry)
    output = private_directory(args.owners.parent / ('gate-' + uuid4().hex), fresh=True)
    save(output / 'intent.json', {'action': 'check_held_quiescence', 'owners': str(args.owners),
         'root_pause_receipt': pause, 'source_service_actions': False})
    try:
        with held_barrier(output, registry, pause_receipt=args.pause_receipt) as witness:
            save(output / 'observation.json', witness)
        save(output / 'result.json', {'status': 'observed_then_released', 'reusable_capture_authority': False,
                                     'snapshot_created': False, 'source_service_actions': False})
    except (Refused, OSError, ValueError, KeyError, TypeError, subprocess.SubprocessError):
        save(output / 'failure.json', {'status': 'refused', 'source_service_actions': False, 'snapshot_created': False})
        raise Refused('quiescence_gate_refused_private_evidence_retained') from None
    print(json.dumps({'status': 'observed_then_released', 'receipt': str(output / 'result.json'), 'reusable_capture_authority': False}))


if __name__ == '__main__':
    try:
        main()
    except (Refused, OSError, ValueError, KeyError, TypeError):
        raise SystemExit('Quiescence gate refused; no source service was paused or resumed. Private evidence retained.') from None
