#!/usr/bin/env python3
"""Root-only coordinated pause of one frozen G69 selection; never capture or resume.

This file may be frozen separately from the nineteen capture operators. It loads
only the explicitly hashed registry closure, and has no default execution mode.
"""

import argparse
from contextlib import contextmanager
from datetime import datetime, timezone
import hashlib
import importlib
import json
import os
from pathlib import Path
import re
import signal
import stat
import subprocess
import sys
import time
from uuid import uuid4

OPERATIONS = Path('/private/tmp/ortak-private-20260905/recovery-operations')
PYTHON = '/Users/nambse/.pyenv/versions/3.12.8/bin/python3'
PROCESS_ORDER = ('native', 'ortak-server', 'buzz-relay', 'ortak-management', 'ortak-worker')
CONTAINER_ORDER = ('controller', 'honcho_api', 'redis', 'minio')
COUNTERS = frozenset(('active_runs', 'pending_cancellation', 'pending_cancel_request', 'pending_outbox',
    'pending_office_output', 'pending_memory_write', 'pending_work_output', 'pending_management_command',
    'application_clients'))
STATE_FORMAT = ('{"exit_code":{{json .State.ExitCode}},"oom":{{json .State.OOMKilled}},'
    '"pid":{{json .State.Pid}},"restarting":{{json .State.Restarting}},'
    '"running":{{json .State.Running}},"finished_at":{{json .State.FinishedAt}}}')
PID_STATE = r"""import json,subprocess,sys
p=subprocess.Popen(['/bin/ps','-p',sys.argv[1],'-o','pid=,uid=,lstart=,comm='],stdout=subprocess.PIPE,stderr=subprocess.PIPE)
try:
 out,err=p.communicate(timeout=2)
 if len(out)>8192 or err or p.returncode not in (0,1): raise ValueError()
 if p.returncode==1 and out.strip(): raise ValueError()
 print(json.dumps(out.decode().strip() if p.returncode==0 else None))
finally:
 if p.poll() is None: p.kill()
 p.wait(timeout=2)
"""


class Refused(Exception):
    """A fixed public refusal code, without provider or credential diagnostics."""


def require(condition, code):
    """Refuse an unexpected identity or incomplete state."""
    if not condition:
        raise Refused(code)


@contextmanager
def overall_deadline(seconds=900):
    """A process-wide timer also bounds nested frozen helpers' independent SQL deadlines."""
    require(signal.getitimer(signal.ITIMER_REAL) == (0.0, 0.0), 'existing_process_timer_refused')
    previous = signal.getsignal(signal.SIGALRM)
    def expired(_signal, _frame):
        raise Refused('pause_deadline_exceeded')
    signal.signal(signal.SIGALRM, expired)
    signal.setitimer(signal.ITIMER_REAL, seconds)
    try:
        yield
    finally:
        signal.setitimer(signal.ITIMER_REAL, 0)
        signal.signal(signal.SIGALRM, previous)


def public_bytes(path, mode, maximum):
    """Read one owner-private immutable public file without following links."""
    before = path.lstat()
    require(stat.S_ISREG(before.st_mode) and before.st_uid == os.getuid() and before.st_nlink == 1
        and stat.S_IMODE(before.st_mode) == mode and before.st_size <= maximum, 'public_file_metadata_refused')
    with os.fdopen(os.open(path, os.O_RDONLY | os.O_NOFOLLOW), 'rb') as source:
        opened = os.fstat(source.fileno())
        raw = source.read(maximum + 1)
    after = path.lstat()
    generation = lambda row: (row.st_ino, row.st_dev, row.st_size, row.st_mtime_ns, row.st_mode)
    require(len(raw) <= maximum and generation(before) == generation(opened) == generation(after),
        'public_file_changed')
    return raw


def load_selected(owners, selected_sha):
    """Validate all frozen code before importing any of it; no mutable repo imports."""
    require(sys.executable == PYTHON, 'selected_python_required')
    operation = owners.parent
    require(owners.name == 'owners.json' and operation.parent == OPERATIONS
        and re.fullmatch(r'[0-9a-f]{32}', operation.name)
        and re.fullmatch(r'[0-9a-f]{64}', selected_sha), 'explicit_selected_registry_required')
    for path in (operation.parent.parent, operation.parent, operation, operation / 'operator-code',
                 operation / 'resume-code'):
        row = path.lstat()
        require(stat.S_ISDIR(row.st_mode) and row.st_uid == os.getuid()
            and stat.S_IMODE(row.st_mode) == 0o700, 'selected_directory_refused')
    registry = json.loads(public_bytes(owners, 0o600, 1024 * 1024))
    digest = registry.pop('registry_sha256')
    canonical = json.dumps(registry, sort_keys=True, separators=(',', ':'), ensure_ascii=True).encode()
    require(digest == selected_sha == hashlib.sha256(canonical).hexdigest(), 'selected_registry_changed')
    registry['registry_sha256'] = digest
    for group, directory in (('operator_code', 'operator-code'), ('source_code', 'resume-code')):
        for entry in registry[group].values():
            frozen = entry['frozen']
            path = Path(frozen['path'])
            require(path.parent == operation / directory, 'frozen_scope_refused')
            raw = public_bytes(path, 0o500, 65536)
            require(len(raw) == frozen['bytes'] and hashlib.sha256(raw).hexdigest() == frozen['sha256'],
                'frozen_code_changed')
    sys.dont_write_bytecode = True
    sys.path.insert(0, str(operation / 'operator-code'))
    gate = importlib.import_module('check_private_recovery_gate')
    require(Path(gate.__file__).parent == operation / 'operator-code', 'selected_import_required')
    require(gate.load_registry(owners) == registry, 'registry_recheck_failed')
    return gate, registry


def drained_counts(counters, *, connected_writers):
    """The same semantic counters must be zero before and after ingress closes."""
    require(set(counters) == COUNTERS and all(type(n) is int and n >= 0 for n in counters.values()),
        'drain_counter_shape_refused')
    require(all(n == 0 for key, n in counters.items() if key != 'application_clients'), 'pending_work_refused')
    require(connected_writers or counters['application_clients'] == 0, 'application_clients_remain')


class Pause:
    """Bounded signal state machine with durable intents and no force-kill/resume fallback."""

    def __init__(self, gate_module, registry, output):
        self.api, self.registry, self.output = gate_module, registry, output
        self.operation = OPERATIONS / registry['operation_id']
        self.gate = gate_module.Gate(output, registry)
        self.inspector, self.command = self.gate.inspector, self.gate.command
        self.expected = self.gate.preparation['observation']
        self.deadline = time.monotonic() + 900
        self.sequence, self.effects = 0, []

    def event(self, phase, value):
        """Every acknowledged or uncertain prefix remains in a unique fsynced file."""
        self.sequence += 1
        row = {'phase': phase, 'at': datetime.now(timezone.utc).isoformat(), **value}
        self.api.save(self.output / f'{self.sequence:03d}-{phase}.json', row)
        return row

    def remaining(self, maximum=900):
        """A single wall-clock bound covers observation, signalling and held verification."""
        seconds = min(maximum, self.deadline - time.monotonic())
        require(seconds > 0, 'pause_deadline_exceeded')
        return seconds

    def preflight(self):
        """Fresh schemas/owners/configs bind authority; ordinary changed DB rows are allowed."""
        require(not (self.operation / 'pause.json').exists(), 'prior_pause_receipt_refused')
        prepare = importlib.import_module('prepare_private_recovery')
        current = prepare.observe(self.api.private_directory(self.output / 'preflight', fresh=True))
        require(prepare.authority(current) == prepare.authority(self.expected), 'prepared_authority_changed')
        require(not any(row['running'] for row in current['contained_children']), 'runtime_children_remain')
        version = self.api.obligations.schema_version(current['main_database'])
        self.event('preflight', {'owners_sha256': self.registry['registry_sha256'], 'schema': version,
            'dynamic_database_rows_frozen_yet': False})

    def drain(self, label):
        """Read the selected reviewed schema's guards without asking any executor to run."""
        main = self.api.Commands(self.api.private_directory(self.output / ('drain-' + label), fresh=True))
        main.deadline = time.monotonic() + self.remaining(90)
        main.inspect()
        counters = json.loads(main.run('counters', main.psql('ortak'), sql=self.api.MAIN_DRAIN_SQL, ceiling=4096))
        drained_counts(counters, connected_writers=True)
        metadata = main.metadata('ortak', 'schema')
        expected = self.expected['main_database']
        require(metadata['schema_sha256'] == expected['schema_sha256']
            and metadata['migration_checksums'] == expected['migration_checksums'], 'main_schema_authority_changed')
        retained = self.api.obligations.observe(main, 'ortak', metadata, self.api.inventory.COMPANY, drained=True)
        self.event('drained', {'stage': label, 'counters': counters, 'obligations': retained})

    def process(self, name):
        """Use production loaded-inode/hash/cwd/start checks immediately before each signal."""
        if name == 'native':
            value = self.api.native_ingress.observe(self.inspector)
            require(value == self.expected['native_ingress'] and value['running'], 'native_owner_changed')
            return value['process']
        value = self.inspector.native(name)
        require(value == self.registry['owners'][name]['live_process'], 'writer_owner_changed')
        return value

    def stop_process(self, name):
        """Signal only the exact current PID once; absence is not an invented exit status."""
        current = self.process(name)
        effect = {'kind': 'process', 'name': name, 'identity': current, 'signal': 'SIGTERM',
            'outcome': 'signal_not_yet_acknowledged'}
        self.effects.append(effect)
        self.event('signal-intent', effect)
        require(self.process(name) == current, 'process_changed_before_signal')
        # macOS has no pidfd signal API. The full production identity is checked
        # twice, immediately before this one PID-scoped call; never signal a name.
        os.kill(current['pid'], signal.SIGTERM)
        effect['outcome'] = 'signal_sent_waiting'
        self.event('signal-sent', effect)
        until = time.monotonic() + self.remaining(30)
        while True:
            state = json.loads(self.inspector.run([PYTHON, '-c', PID_STATE, str(current['pid'])], limit=9000))
            if state is None:
                break
            require(state.split() == [str(current['pid']), str(current['uid']),
                *current['started_at'].split(), *current['executable'].split()], 'pid_reused_after_signal')
            require(time.monotonic() < until, 'process_graceful_exit_timeout')
            time.sleep(0.2)
        effect['outcome'] = 'verified_pid_absent'
        self.event('process-stopped', {**effect, 'exit_status_available': False})

    def container_state(self, identifier):
        """Read only public lifecycle fields, never Config.Env or full inspect."""
        return json.loads(self.inspector.run(self.command.docker('inspect', '--format', STATE_FORMAT, identifier), limit=2048))

    def stop_container(self, name):
        """Manual Docker stop with infinite daemon grace and a bounded local wait; no SIGKILL."""
        old = self.expected['containers'][name]
        require(self.inspector.container(name) == old and old['running'], 'container_owner_changed')
        if name == 'controller':
            require(not any(row['running'] for row in self.inspector.children()), 'runtime_children_remain')
        effect = {'kind': 'container', 'name': name, 'id': old['id'], 'image': old['image'],
            'started_at': old['started_at'], 'signal': 'SIGTERM', 'daemon_force_kill': False,
            'outcome': 'stop_not_yet_acknowledged'}
        self.effects.append(effect)
        self.event('stop-intent', effect)
        require(self.inspector.container(name) == old, 'container_changed_before_signal')
        previous_deadline = self.command.deadline
        self.command.deadline = time.monotonic() + self.remaining(45)
        try:
            # Docker --timeout=-1 never schedules SIGKILL. If transport times out,
            # its daemon stop may remain pending: retain that uncertainty and do
            # not resume automatically. The bounded runner kills only its CLI.
            self.inspector.run(self.command.docker('stop', '--signal', 'SIGTERM', '--timeout', '-1', old['id']), limit=1024)
        finally:
            self.command.deadline = min(previous_deadline, self.deadline)
        state = self.container_state(old['id'])
        allowed = (0,) if name in ('redis', 'minio') else (0, 143)
        require(state['exit_code'] in allowed and not state['running'] and not state['oom']
            and not state['restarting'] and state['pid'] == 0, 'unclean_container_exit')
        effect['outcome'] = 'graceful_stop_acknowledged'
        self.event('container-stopped', {**effect, 'state': state})

    def final_stopped(self):
        """Fresh universal owner/client checks also reject any replacement writer."""
        self.gate.stopped_owners()
        for name in ('redis', 'minio'):
            require(not self.inspector.container(name)['running'], 'cold_store_still_running')
        return self.gate.drained_databases()

    def stop_scorer(self):
        """Stop the explicit OAuth maintenance owner even when zero scores are active."""
        expected=self.expected.get('scorer_owner')
        if expected is None:
            require(self.api.scorer.selection() is None,'scorer_selection_changed')
            return
        selected={k:expected[k] for k in ('receipt','selection')}
        require(self.api.scorer.selection()==selected,'scorer_selection_changed')
        require(self.api.scorer.prepare(self.inspector,selected)==expected,'scorer_owner_changed')
        identity=expected['selection']['container']
        effect={'kind':'container','name':'scorer','id':identity['id'],'image':identity['image'],
            'started_at':identity['started_at'],'signal':'SIGTERM','daemon_force_kill':False,
            'outcome':'stop_not_yet_acknowledged','active_scores_is_not_credential_drain':True}
        self.effects.append(effect);self.event('scorer-stop-intent',effect)
        require(self.api.scorer.prepare(self.inspector,selected)==expected,'scorer_owner_changed')
        previous=self.command.deadline
        self.command.deadline=time.monotonic()+self.remaining(45)
        try:
            self.inspector.run(self.command.docker('stop','--signal','SIGTERM','--timeout','-1',identity['id']),limit=1024)
        finally:self.command.deadline=min(previous,self.deadline)
        actual=self.api.scorer.owner(self.inspector,selected);state=actual['state']
        require(not state['running'] and state['pid']==0 and state['exit_code']==0
            and not state['oom'] and not state['restarting'],'scorer_not_cleanly_stopped')
        effect['outcome']='graceful_stop_acknowledged'
        self.event('scorer-stopped',{**effect,'state':state,
            'shared_oauth_writers_require_final_gate':True})

    def publish_held_pause(self):
        """Publish the real pause only while Linux executor/OAuth and schema leases hold."""
        self.final_stopped()
        name = 'ortak-recovery-lease-' + uuid4().hex
        protected = self.api.selected_journal.require_confidential_schema(
            self.expected.get('journal_confidential'), self.api.inventory.MAIN_SCHEMA_VERSION)
        raw = self.api.selected_journal.lease_script(self.api.recovery_lock_holder,
            confidential_reviewed=protected)
        require(len(raw) <= 98304, 'lease_script_bound')
        self.event('lease-intent', {'name': name, 'image': self.expected['containers']['controller']['image'],
            'script_sha256': hashlib.sha256(raw).hexdigest(), 'network': 'none', 'docker_socket': False})
        process = subprocess.Popen(self.api.lease_args(self.command, name,
            self.expected['containers']['controller']['image'], raw.decode()), stdin=subprocess.PIPE,
            stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, env=self.api.environment(), start_new_session=True)
        completed = False
        try:
            witness = self.api.response(process, self.command)
            require(witness.get('status') == 'held', 'linux_lease_not_held')
            with self.api.held_schema(self.api.private_directory(self.output / 'schema-lease', fresh=True)) as schema:
                self.gate.schema_owner = schema
                before = self.final_stopped()
                self.event('held', {'linux_lease': witness, 'schema_lease': schema, 'databases': before})
                require(self.final_stopped() == before, 'drain_generation_changed')
                require(process.poll() is None, 'linux_lease_lost')
                self.api.save(self.operation / 'pause.json', {'format': 'ortak-private-recovery-pause/1',
                    'owners_sha256': self.registry['registry_sha256'], 'host_oauth_enrollment_fenced': True,
                    'root_coordinated_pause': True, 'resume_under_root_control': True})
                self.api.root_pause_receipt(self.operation / 'pause.json', self.registry)
                process.stdin.write(b'release\n'); process.stdin.flush()
                require(self.api.response(process, self.command) == {'status': 'released'}, 'linux_lease_release_failed')
                require(process.wait(timeout=self.remaining(3)) == 0, 'linux_lease_process_failed')
            completed = True
        finally:
            self.command.stop(process)  # Only this owned helper CLI process group.
            self.event('lease-finish', {'name': name, 'released_acknowledged': completed, 'retained': True,
                'unacknowledged_helper_maximum_seconds': 0 if completed else 900, 'reusable_capture_authority': False})

    def resume_plan(self):
        """Concrete original-owner commands remain root-controlled, including uncertain stops."""
        touched = {row['name'] for row in self.effects}
        containers = [name for name in ('redis', 'minio', 'honcho_api', 'controller') if name in touched]
        result={'automatic_resume': False, 'steps': [
            {'name': name, 'argv': self.command.docker('start', self.expected['containers'][name]['id'])}
            for name in containers] + [
            {'name': name, 'argv': self.registry['resume_recipes'][name], 'persistent_session_required': True}
            for name in ('buzz-relay', 'ortak-server', 'ortak-management', 'ortak-worker') if name in touched],
            'native': {'binary': self.expected['native_ingress']['artifact']['binary'],
                'root_selected_existing_profile_launch_required': 'native' in touched},
            'cwd': str(self.api.inventory.STATE),
            'preconditions': ['rediscover each current stopped identity; never start an already-running owner',
                'resolve any unacknowledged daemon stop or lease helper before restarting its source',
                'verify stores then application health; record new PID/start/session identities',
                'keep new ingress fenced until root deliberately releases admission'],
            'source_postgres_stop_or_restore': False}
        if 'scorer' in touched:
            # A failed stop must leave a concrete recovery obligation, never an
            # executable start against an unknown or still-running owner.
            expected=self.expected.get('scorer_owner')
            result['scorer']={'automatic_resume':False,'create_or_replace':False,
                'selection_sha256':expected['receipt']['sha256'],
                'container_id':expected['selection']['container']['id'],
                'requires':'fresh private_recovery_scorer.stopped proof then resume_argv; never replay an uncertain stop',
                'verify':'private_recovery_scorer.verify_resumed'}
        return result

    def run(self):
        """Any failed prefix is retained with explicit source resume/reconciliation requirements."""
        self.event('intent', {'action': 'root_coordinated_pause', 'owners_sha256': self.registry['registry_sha256'],
            'host_oauth_enrollment_fenced_by_root': True, 'source_capture': False,
            'source_postgres_stop': False, 'helper_sha256': hashlib.sha256(Path(__file__).read_bytes()).hexdigest()})
        try:
            self.preflight()
            self.drain('before-native')
            for name in PROCESS_ORDER:
                self.stop_process(name)
                if name in ('native', 'buzz-relay', 'ortak-worker'):
                    self.drain('after-' + name)
            self.stop_scorer()
            for name in CONTAINER_ORDER:
                self.stop_container(name)
            self.publish_held_pause()
            self.event('result', {'status': 'paused_verified', 'pause_receipt': str(self.operation / 'pause.json'),
                'resume_plan': self.resume_plan(), 'capture_completed': False, 'postgres_running': True,
                'capture_must_acquire_new_held_barrier': True})
        except BaseException as error:
            safe = str(error) if isinstance(error, (Refused, self.api.Refused)) else 'unexpected_pause_failure'
            if not safe.replace('_', '').isalnum() or len(safe) > 128:
                safe = 'bounded_pause_refusal'
            self.event('failure', {'status': 'failed_root_resume_or_reconciliation_required',
                'code': safe, 'effects': self.effects, 'resume_plan': self.resume_plan(),
                'capture_authorized_by_this_attempt': False, 'force_kill_performed': False})
            raise Refused('pause_failed_private_receipt_retained') from None


def main():
    """Execute only after root explicitly fences all host OAuth enrollment activity."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--execute-root-pause', action='store_true', required=True)
    parser.add_argument('--host-oauth-enrollment-fenced', action='store_true', required=True)
    parser.add_argument('--owners', type=Path, required=True)
    parser.add_argument('--owners-sha256', required=True)
    args = parser.parse_args()
    with overall_deadline():
        gate, registry = load_selected(args.owners, args.owners_sha256)
        output = gate.private_directory(args.owners.parent / ('pause-attempt-' + uuid4().hex), fresh=True)
        try:
            Pause(gate, registry, output).run()
        except Refused:
            print(json.dumps({'status': 'failed_root_resume_or_reconciliation_required', 'evidence': str(output)}))
            return 1
        print(json.dumps({'status': 'paused_verified', 'evidence': str(output), 'pause_receipt': str(args.owners.parent / 'pause.json')}))
        return 0


if __name__ == '__main__':
    try:
        raise SystemExit(main())
    except (Refused, OSError, ValueError, KeyError, TypeError):
        raise SystemExit('Pause refused; exact private evidence retained when an attempt was created.') from None
