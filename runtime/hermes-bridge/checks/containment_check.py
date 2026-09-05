"""Opt-in real Docker lifecycle check; no Hermes imports or provider credentials.

Run inside the controller image as UID10001 with a fresh, identically mounted
host fixture parent and Docker socket. The production DockerExecutor/Bridge and
exact worker image are used. Only the child command is replaced with fixed probe
code, keeping all production image/network/mount/limit/ownership arguments.
"""
import argparse
import json
import multiprocessing
import os
import sqlite3
import time
from pathlib import Path
from uuid import uuid4

from ortak_hermes_bridge.docker_executor import DockerEngine, DockerExecutor, container_name
from ortak_hermes_bridge.journal import Journal
from ortak_hermes_bridge.service import Bridge, EMPTY_POLICY

PROBE = r'''
import json, os, subprocess, sys, time
from pathlib import Path
from ortak_hermes_bridge.journal import Journal
from ortak_hermes_bridge.worker import arm_deadline
body = json.loads(sys.stdin.buffer.read(256 * 1024 + 1))
spec = body['spec']
journal = Journal(sys.argv[sys.argv.index('--journal') + 1])
assert journal.begin_execution(spec['idempotency_key'])
arm_deadline(3 if spec['input'] == 'fixed deadline fixture' else 120)
assert os.getuid() == 10001
assert not Path('/var/run/docker.sock').exists()
assert not Path('/usr/bin/docker').exists()
assert not Path('/run/controller').exists()
assert os.environ['HOME'] == os.environ['HERMES_HOME'] == '/tmp/hermes-home'
for location in ('/opt/ortak-probe-forbidden', '/profile/ortak-probe-forbidden'):
    try:
        with open(location, 'x') as file:
            file.write('disposable fixture only')
    except OSError:
        pass
    else:
        raise RuntimeError('readonly filesystem invariant failed')
status = Path('/proc/self/status').read_text()
assert 'NoNewPrivs:\t1' in status
assert 'CapEff:\t0000000000000000' in status
beat = Path('/ortak-state') / (spec['run_id'] + '.beat')
child = subprocess.Popen([sys.executable, '-c',
    "import pathlib,sys,time; p=pathlib.Path(sys.argv[1]); end=time.monotonic()+120\n"
    "while time.monotonic()<end:\n p.write_text(str(time.monotonic_ns())); time.sleep(.05)\n",
    str(beat)], start_new_session=True)
end = time.monotonic() + 5
while not beat.exists() and time.monotonic() < end:
    time.sleep(.05)
assert beat.exists() and child.poll() is None
(Path('/ortak-state') / (spec['run_id'] + '.ready')).write_text(json.dumps(
    {'uid': os.getuid(), 'child_pid': child.pid, 'separate_session': True}))
child.wait(timeout=125)
'''


class ProbeEngine(DockerEngine):
    """Exercise production containment with a deterministic credential-free child."""
    def launch(self, args, payload):
        args = list(args)
        index = args.index('ortak_hermes_bridge.worker')
        if args[index - 1] != '-m':
            raise RuntimeError('production worker command changed')
        args[index - 1:index + 1] = ['-c', PROBE]
        return super().launch(args, payload)


def owner(settings, spec, pipe):
    """Separate controller process deliberately killed by the recovery check."""
    journal = Journal(settings['journal'])
    executor = make_executor(settings, journal)
    try:
        receipt = Bridge(journal, settings['company'], [settings['profile']], executor).dispatch(
            'POST', '/v1/runs', {'company_id': settings['company'], 'spec': spec})
        pipe.send(receipt)
        pipe.recv()
    finally:
        executor.close()
        pipe.close()


def make_executor(settings, journal):
    """Use the production owner with a real Docker CLI, image and network."""
    return DockerExecutor(journal, settings['company'], [settings['profile']],
                          settings['image'], settings['network'], ProbeEngine(settings['docker']),
                          validated_digest=settings['image'])


def wait_ready(settings, spec):
    """Bound initialization; early child failure never looks like successful start."""
    ready = Path(settings['journal']).parent / (spec['run_id'] + '.ready')
    deadline = time.monotonic() + 25
    while time.monotonic() < deadline:
        if ready.exists():
            data = json.loads(ready.read_text())
            if data['uid'] != 10001 or not data['separate_session']:
                raise RuntimeError('invalid probe evidence')
            return
        time.sleep(.1)
    raise RuntimeError('contained probe did not become ready')


def prove_stopped(settings, spec):
    """Require daemon stop proof and a frozen heartbeat from the separate session."""
    engine = DockerEngine(settings['docker'])
    if not engine.stopped(container_name(spec['idempotency_key'])):
        raise RuntimeError('Docker did not confirm whole-container stop')
    beat = Path(settings['journal']).parent / (spec['run_id'] + '.beat')
    before = beat.read_text()
    time.sleep(.3)
    if beat.read_text() != before:
        raise RuntimeError('descendant heartbeat continued after cancellation acknowledgement')


def dense_replay(journal, key):
    """Replay one event per page to exercise exclusive durable cursor semantics."""
    cursor, events = 0, []
    for _ in range(20):
        page = journal.events(key, after=cursor, limit=1)
        for event in page['events']:
            if int(event['cursor']) != cursor + 1:
                raise RuntimeError('replay cursor skipped or repeated')
            cursor += 1
            events.append(event)
        if page['terminal']:
            if not journal.events(key, after=cursor, limit=1)['terminal']:
                raise RuntimeError('terminal replay changed')
            return events
    raise RuntimeError('replay did not terminate within expected fixture event bound')


def main():
    """Create only new disposable fixture files and operate their random run keys."""
    parser = argparse.ArgumentParser()
    parser.add_argument('--image', required=True)
    parser.add_argument('--network', required=True)
    parser.add_argument('--fixture-parent', required=True)
    parser.add_argument('--docker', default='/usr/bin/docker')
    args = parser.parse_args()
    if os.getuid() != 10001 or sqlite3.sqlite_version_info < (3, 51, 3):
        raise RuntimeError('run in the patched controller image as UID10001')
    parent = Path(args.fixture_parent)
    if not parent.is_absolute() or str(parent.resolve()) != str(parent) or ',' in str(parent):
        raise RuntimeError('fixture parent must be an identical canonical host/container path')
    # A caller supplies one writable parent. Existing child files are never adopted.
    root = parent / ('containment-' + str(uuid4()))
    root.mkdir(mode=0o700)
    (root / 'ORTAK_DISPOSABLE_CHECK.json').write_text(json.dumps({'purpose': 'hermes-containment-check'}))
    company = str(uuid4())
    profile_dir = root / 'profile'
    profile_dir.mkdir(mode=0o700)
    binding = {'adapter': 'hermes', 'profile_ref': 'fixture-' + str(uuid4()),
               'model': 'fixture-no-provider', 'workspace_ref': 'none',
               'credential_refs': ['fixture-not-a-secret'], 'options': {}}
    profile = {'employee_id': 'fixture-' + str(uuid4()), 'binding': binding, 'directory': str(profile_dir)}
    marker = {'company_id': company, 'employee_id': profile['employee_id'], 'profile_ref': binding['profile_ref']}
    for name, value in (('ORTAK_DISPOSABLE_PROFILE.json', marker),
                        ('ORTAK_RUNTIME_BINDING.json', binding),
                        ('ORTAK_PROVIDER.json', {'provider': 'openai', 'credential_ref': 'fixture-not-a-secret'})):
        (profile_dir / name).write_text(json.dumps(value))
    (profile_dir / 'provider-token').write_text('fixture-only-never-read-by-probe')
    settings = {'journal': str(root / 'state' / 'journal.sqlite'), 'company': company,
                'profile': profile, 'image': args.image, 'network': args.network, 'docker': args.docker}
    journal = Journal(settings['journal'])
    specs = []
    def spec():
        run = str(uuid4())
        value = {'run_id': run, 'employee_id': profile['employee_id'], 'revision_id': str(uuid4()),
                 'binding': binding, 'permissions': EMPTY_POLICY, 'input': 'fixed containment fixture',
                 'context': {}, 'idempotency_key': f'ortak-run:{company}:{run}'}
        specs.append(value)
        return value
    def control(value):
        return {'company_id': company, 'run_id': value['run_id'],
                'idempotency_key': value['idempotency_key'], 'reason': 'disposable containment check'}
    def start(bridge, value):
        return bridge.dispatch('POST', '/v1/runs', {'company_id': company, 'spec': value})
    executor, process = None, None
    results = []
    try:
        executor = make_executor(settings, journal)
        bridge = Bridge(journal, company, [profile], executor)
        for terminal in (None, 'failed', 'completed'):
            value = spec()
            receipt = start(bridge, value)
            wait_ready(settings, value)
            lookup = bridge.dispatch('POST', '/v1/runs/lookup', control(value))
            if (lookup['runtime_run_ref'], lookup['started_at']) != (receipt['runtime_run_ref'], receipt['started_at']):
                raise RuntimeError('lost start receipt lookup was unstable')
            if terminal == 'failed':
                journal.fail(value['idempotency_key'], 'provider_failed')
            elif terminal == 'completed':
                journal.complete(value['idempotency_key'], 'fixture complete before process exit')
            acknowledgement = bridge.dispatch('POST', '/v1/runs/cancel', control(value))
            expected = 'already_terminal' if terminal else 'cancelled'
            if acknowledgement['outcome'] != expected:
                raise RuntimeError('cancel acknowledgement has incorrect semantics')
            prove_stopped(settings, value)
            dense_replay(journal, value['idempotency_key'])
            results.append('stop_' + (terminal or 'running'))
        delayed = spec()
        bridge.dispatch('POST', '/v1/runs/cancel', control(delayed))
        start(bridge, delayed)
        if delayed['idempotency_key'] in executor.running or journal.has_start(delayed['idempotency_key']):
            raise RuntimeError('delayed start executed after cancellation tombstone')
        dense_replay(journal, delayed['idempotency_key'])
        results.append('cancel_before_start')
        executor.close()
        executor = None
        crash = spec()
        context = multiprocessing.get_context('spawn')
        incoming, outgoing = context.Pipe()
        process = context.Process(target=owner, args=(settings, crash, outgoing))
        process.start()
        outgoing.close()
        if not incoming.poll(25):
            raise RuntimeError('separate controller did not return a start receipt')
        original = incoming.recv()
        wait_ready(settings, crash)
        process.kill()
        process.join(timeout=5)
        if process.is_alive():
            raise RuntimeError('controller crash injection did not terminate')
        incoming.close()
        process = None
        executor = make_executor(settings, journal)
        bridge = Bridge(journal, company, [profile], executor)
        recovered = bridge.dispatch('POST', '/v1/runs/lookup', control(crash))
        if recovered['runtime_run_ref'] != original['runtime_run_ref'] or recovered['status'] != 'cancelled':
            raise RuntimeError('restart did not preserve identity and seal interrupted execution')
        prove_stopped(settings, crash)
        start(bridge, crash)
        if executor.running:
            raise RuntimeError('restart blindly re-executed a prior start')
        dense_replay(journal, crash['idempotency_key'])
        results.extend(['controller_sigkill_recovery', 'restart_no_blind_rerun', 'dense_durable_replay'])
        executor.close()
        executor = None
        deadline_spec = spec()
        deadline_spec['input'] = 'fixed deadline fixture'
        incoming, outgoing = context.Pipe()
        process = context.Process(target=owner, args=(settings, deadline_spec, outgoing))
        process.start()
        outgoing.close()
        if not incoming.poll(25):
            raise RuntimeError('deadline fixture controller did not start')
        incoming.recv()
        wait_ready(settings, deadline_spec)
        process.kill()
        process.join(timeout=5)
        if process.is_alive():
            raise RuntimeError('deadline fixture controller did not terminate')
        incoming.close()
        process = None
        deadline = time.monotonic() + 10
        while not DockerEngine(args.docker).stopped(container_name(deadline_spec['idempotency_key'])):
            if time.monotonic() >= deadline:
                raise RuntimeError('worker exceeded its deadline while controller was absent')
            time.sleep(.1)
        prove_stopped(settings, deadline_spec)
        executor = make_executor(settings, journal)
        dense_replay(journal, deadline_spec['idempotency_key'])
        results.append('child_deadline_without_controller')
    finally:
        if process is not None:
            process.kill()
            process.join(timeout=5)
        if executor is not None:
            executor.close()
        # Includes crash paths and terminal registry states; ownership verification
        # in the production engine precedes every possible removal.
        engine = DockerEngine(args.docker)
        for value in specs:
            if not engine.stop(value['idempotency_key'], args.image):
                raise RuntimeError('fixture containment cleanup not confirmed')
    print(json.dumps({'checks': results, 'image': args.image, 'sqlite_version': sqlite3.sqlite_version,
                      'fixture_directory': str(root), 'provider_calls': 0,
                      'scope': 'production executor with fixed probe command; not a model-call smoke'}, sort_keys=True))

if __name__ == '__main__':
    main()
