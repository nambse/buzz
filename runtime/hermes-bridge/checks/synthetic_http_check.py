"""Synthetic controller→real pinned AIAgent/SDK→fixture→journal integration.

Run only inside the reviewed controller with a new identically mounted fixture
parent. No real model, employee activation, Office routing or provider credential
is involved. The only override redirects AIAgent's constructor to a test endpoint.
"""
import argparse
import hashlib
from http.client import HTTPConnection
import json
import os
import re
import signal
import sqlite3
import stat
import threading
import time
from http.server import HTTPServer
from pathlib import Path
from uuid import uuid4

from ortak_hermes_bridge import HERMES_REVISION
from ortak_hermes_bridge.docker_executor import DockerEngine, DockerExecutor, container_name
from ortak_hermes_bridge.journal import Journal, reference
from ortak_hermes_bridge.service import Bridge, EMPTY_POLICY, handler

TOKEN = 'ortak-synthetic-not-a-provider-credential'
LAUNCHER = r'''
import socket, sys, traceback, json
from pathlib import Path
from ortak_hermes_bridge import worker
host = '__ORTAK_FIXTURE_HOST__'
address = socket.gethostbyname(host)
def audit(event, args):
    allowed = True
    if event == 'socket.getaddrinfo': allowed = args[0] == host and args[1] == 8080
    elif event == 'socket.connect': allowed = args[1] == (address, 8080)
    elif event in {'subprocess.Popen','os.system','os.exec','os.posix_spawn','os.fork'}: allowed = False
    if not allowed:
        Path('/ortak-state/synthetic-audit-denied').write_text(json.dumps({'event': event, 'stack': [(f.filename, f.lineno, f.name) for f in traceback.extract_stack(limit=12)]}))
        raise RuntimeError('synthetic I/O boundary denied')
sys.addaudithook(audit)
original = worker.load_hermes
def load_fixture_agent():
    base = original()
    # SDK-created client copies do not preserve _platform. This test-only
    # metadata seam reports the actual kernel without executing uname.
    import openai._base_client as sdk_metadata
    if sys.platform != 'linux':
        raise RuntimeError('synthetic Linux image required')
    sdk_metadata.get_platform = lambda: 'Linux'
    class FixtureEndpointAgent(base):
        def __init__(self, **kwargs):
            if kwargs.get('provider') != 'openai' or kwargs.get('api_key') != 'ortak-synthetic-not-a-provider-credential':
                raise RuntimeError('synthetic credential selection changed')
            kwargs['base_url'] = 'http://' + host + ':8080/v1'
            kwargs['api_mode'] = 'codex_responses'
            super().__init__(**kwargs)
    return FixtureEndpointAgent
worker.load_hermes = load_fixture_agent
worker.main()
'''


class FixtureEngine(DockerEngine):
    """Keep all production container arguments and stdin; replace only test entry."""
    def __init__(self, docker, host):
        super().__init__(docker)
        if not re.fullmatch(r'ortak-synthetic-[0-9a-f]{32}\.api\.openai\.com\.invalid', host):
            raise ValueError('invalid synthetic host')
        self.launcher = LAUNCHER.replace('__ORTAK_FIXTURE_HOST__', host)

    def launch(self, args, payload):
        args = list(args)
        index = args.index('ortak_hermes_bridge.worker')
        if args[index - 1] != '-m': raise RuntimeError('worker command changed')
        args[index - 1:index + 1] = ['-c', self.launcher]
        return super().launch(args, payload)


def write(path, value):
    """Persist fresh test evidence only; never overwrite an existing artifact."""
    with path.open('x') as file:
        file.write(json.dumps(value, sort_keys=True, indent=2) + '\n')
        file.flush()
        os.fsync(file.fileno())


def bounded_wait(check, seconds=30):
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        value = check()
        if value: return value
        time.sleep(0.1)
    raise RuntimeError('synthetic wait expired')


def http(port, token, method, path, value=None):
    connection = HTTPConnection('127.0.0.1', port, timeout=10)
    try:
        body = json.dumps(value).encode() if value is not None else None
        connection.request(method, path, body=body, headers={'Authorization': 'Bearer ' + token, 'Content-Type': 'application/json'})
        response = connection.getresponse()
        raw = response.read(262145)
        if len(raw) > 262144: raise RuntimeError('controller response exceeded cap')
        return response.status, json.loads(raw)
    finally: connection.close()


def remove_owned(command, kind, name, identifier, image):
    """Never mistake daemon failure for absence or remove resources without proof."""
    listing = [kind, 'ls', '--filter', 'name=' + name, '--format', '{{.Name}}' if kind == 'network' else '{{.Names}}']
    if kind == 'container': listing.insert(2, '--all')
    names = command(listing).splitlines()
    if not names: return True
    if names != [name]: return False
    template = '{{index .Labels "org.ortak.synthetic"}}' if kind == 'network' else '{{index .Config.Labels "org.ortak.synthetic"}}|{{.Config.Image}}'
    expected = identifier if kind == 'network' else identifier + '|' + image
    if command([kind, 'inspect', '--format', template, name]) != expected: return False
    command([kind, 'rm', *(['--force'] if kind == 'container' else []), name])
    return not command(listing)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--image', required=True)
    parser.add_argument('--fixture-parent', type=Path, required=True)
    parser.add_argument('--docker', default='/usr/bin/docker')
    args = parser.parse_args()
    os.umask(0o077)
    os.environ.clear()
    os.environ.update(PATH='/usr/local/bin:/usr/bin:/bin', HOME='/tmp', LANG='C.UTF-8',
                      DOCKER_HOST='unix:///var/run/docker.sock')
    if os.getuid() != 10001 or sqlite3.sqlite_version_info < (3, 51, 3):
        raise RuntimeError('patched controller UID10001 required')
    parent = args.fixture_parent
    if not parent.is_absolute() or parent.resolve() != parent or ',' in str(parent):
        raise RuntimeError('canonical identical parent required')
    info = parent.lstat()
    if not stat.S_ISDIR(info.st_mode) or info.st_uid != 10001 or info.st_mode & 0o077:
        raise RuntimeError('private UID10001 parent required')
    if not re.fullmatch(r'sha256:[0-9a-f]{64}', args.image): raise RuntimeError('immutable image required')
    identifier = uuid4().hex
    root = parent / ('synthetic-http-' + identifier)
    root.mkdir(mode=0o700)
    fixture = root / 'fixture'; fixture.mkdir(mode=0o700)
    profile_dir = root / 'profile'; profile_dir.mkdir(mode=0o700)
    company = str(uuid4())
    network = 'ortak-synthetic-' + identifier
    provider = network + '-provider'
    host = network + '.api.openai.com.invalid'
    engine = FixtureEngine(args.docker, host)
    if not engine.validated_image(args.image): raise RuntimeError('source image not present')
    provider_code = Path(__file__).with_name('synthetic_provider.py').read_text()
    receipt = {'scope': 'synthetic real Hermes SDK/controller integration; no employee activation or real model',
               'worker_image': args.image, 'source_revision': HERMES_REVISION, 'sqlite': sqlite3.sqlite_version,
               'controller_check_sha256': hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
               'launcher_sha256': hashlib.sha256(engine.launcher.encode()).hexdigest(),
               'provider_fixture_sha256': hashlib.sha256(provider_code.encode()).hexdigest(),
               'company_id': company, 'network': network, 'fixture_container': provider, 'checks': []}
    write(root / 'intent.json', receipt)
    binding = {'adapter': 'hermes', 'profile_ref': 'synthetic-' + identifier, 'model': 'gpt-4o-mini',
               'workspace_ref': 'none', 'credential_refs': ['synthetic-fixture-only'], 'options': {}}
    profile = {'employee_id': 'synthetic-' + identifier, 'directory': str(profile_dir), 'binding': binding}
    write(profile_dir / 'ORTAK_DISPOSABLE_PROFILE.json', {'company_id': company, 'employee_id': profile['employee_id'], 'profile_ref': binding['profile_ref']})
    write(profile_dir / 'ORTAK_RUNTIME_BINDING.json', binding)
    write(profile_dir / 'ORTAK_PROVIDER.json', {'provider': 'openai', 'credential_ref': 'synthetic-fixture-only'})
    (profile_dir / 'provider-token').write_text(TOKEN)
    journal = Journal(root / 'state' / 'journal.sqlite')
    executor = server = thread = None
    network_issued = provider_issued = False
    specs = []
    def command(arguments):
        code, result = engine.command(arguments)
        if code: raise RuntimeError('synthetic engine operation failed')
        return result
    def counts():
        try: return json.loads((fixture / 'counts.json').read_text())
        except (OSError, ValueError): return {}
    def expire(signum, frame):
        raise RuntimeError('synthetic total deadline expired')
    signal.signal(signal.SIGALRM, expire)
    signal.alarm(180)
    try:
        network_issued = True
        receipt['stage'] = 'create_internal_network'
        command(['network', 'create', '--internal', '--label', 'org.ortak.synthetic=' + identifier, network])
        if command(['network', 'inspect', '--format', '{{.Internal}}|{{.Driver}}|{{index .Labels "org.ortak.synthetic"}}', network]) != f'true|bridge|{identifier}':
            raise RuntimeError('internal network validation failed')
        provider_issued = True
        receipt['stage'] = 'start_fixture_provider'
        command(['run', '--pull=never', '--detach', '--init', '--name', provider, '--network', network, '--network-alias', host,
                 '--label', 'org.ortak.synthetic=' + identifier, '--entrypoint', 'python', '--read-only', '--cap-drop', 'ALL',
                 '--security-opt', 'no-new-privileges', '--pids-limit', '32', '--memory', '128m', '--cpus', '0.5',
                 '--user', '10001:10001', '--log-driver', 'none', '--tmpfs', '/tmp:rw,noexec,nosuid,size=16777216',
                 '--mount', f'type=bind,src={fixture},dst=/fixture', args.image, '-c', provider_code])
        receipt['stage'] = 'await_fixture_provider'
        bounded_wait(lambda: (fixture / 'ready').exists(), 15)
        receipt['stage'] = 'executor_initialization'
        executor = DockerExecutor(journal, company, [profile], args.image, network, engine, validated_digest=args.image)
        receipt['stage'] = 'controller_initialization'
        bridge = Bridge(journal, company, [profile], executor)
        bearer = uuid4().hex + uuid4().hex
        server = HTTPServer(('127.0.0.1', 0), handler(bridge, bearer))
        server.request_queue_size = 8
        thread = threading.Thread(target=server.serve_forever, kwargs={'poll_interval': 0.1})
        thread.start()
        def request(method, path, value=None):
            status, result = http(server.server_port, bearer, method, path, value)
            if status != 200: raise RuntimeError('controller request failed')
            return result
        receipt['stage'] = 'controller_authentication'
        if http(server.server_port, 'wrong-fixture-bearer', 'GET', '/v1/capabilities')[0] != 401:
            raise RuntimeError('authentication fence failed')
        if 'run_start' not in request('GET', '/v1/capabilities')['capabilities']:
            raise RuntimeError('validated executor unavailable')
        if not request('POST', '/v1/profiles/inspect', {'company_id': company, 'binding': binding})['healthy']:
            raise RuntimeError('actual profile inspection failed')
        receipt['checks'].append('authenticated_real_controller_profile_inspect')
        for kind in ('normal', 'tool', 'slow'):
            receipt['stage'] = kind
            run = str(uuid4())
            spec = {'run_id': run, 'employee_id': profile['employee_id'], 'revision_id': str(uuid4()),
                    'binding': binding, 'permissions': EMPTY_POLICY, 'input': 'ORTAK_SYNTHETIC_' + kind.upper(),
                    'context': {}, 'idempotency_key': f'ortak-run:{company}:{run}'}
            specs.append(spec)
            control = {'company_id': company, 'run_id': run, 'idempotency_key': spec['idempotency_key']}
            start = request('POST', '/v1/runs', {'company_id': company, 'spec': spec})
            replay = request('POST', '/v1/runs', {'company_id': company, 'spec': spec})
            if (start['runtime_run_ref'], start['started_at']) != (replay['runtime_run_ref'], replay['started_at']):
                raise RuntimeError('start idempotency failed')
            bounded_wait(lambda: counts().get(kind) == 1, 40)
            if kind == 'slow':
                cancelled = request('POST', '/v1/runs/cancel', {**control, 'reason': 'synthetic in-flight SDK cancellation'})
                if cancelled['outcome'] != 'cancelled' or not engine.stopped(container_name(spec['idempotency_key'])):
                    raise RuntimeError('SDK cancellation did not stop container')
            else:
                bounded_wait(lambda: request('POST', '/v1/runs/lookup', control)['status'] in {'completed', 'failed'}, 30)
            events, cursor = [], 0
            for _ in range(16):
                page = request('GET', f"/v1/runs/{reference(spec['idempotency_key'])}/events?after={cursor}&limit=1")
                for event in page['events']:
                    if int(event['cursor']) != cursor + 1: raise RuntimeError('cursor gap')
                    cursor += 1; events.append(event)
                if page['terminal']: break
            else: raise RuntimeError('journal never terminated')
            types = [event['payload']['event_type'] for event in events]
            if kind == 'normal' and types != ['run.started', 'assistant.delta', 'delivery.intent', 'run.completed']:
                raise RuntimeError('real SDK completion missing')
            if kind == 'normal' and 'Synthetic bridge fixture answer.' not in json.dumps(events):
                raise RuntimeError('fixture answer not durable')
            if kind == 'tool' and ('run.failed' not in types or 'policy_denied' not in json.dumps(events) or 'delivery.intent' in types):
                raise RuntimeError('real SDK tool intent not denied')
            terminal = request('POST', '/v1/runs/lookup', control)
            request('POST', '/v1/runs', {'company_id': company, 'spec': spec})
            if request('POST', '/v1/runs/lookup', control) != terminal or counts().get(kind) != 1:
                raise RuntimeError('terminal replay restarted execution')
            request('POST', '/v1/runs/cancel', {**control, 'reason': 'confirm synthetic containment'})
            if not engine.stopped(container_name(spec['idempotency_key'])):
                raise RuntimeError('terminal container remained alive')
            write(root / (kind + '-events.json'), events)
            receipt['checks'].append('real_sdk_' + kind)
        if counts() != {'normal': 1, 'tool': 1, 'slow': 1, 'metadata': 2, 'invalid': 0}:
            raise RuntimeError('unexpected SDK fixture traffic')
        if (root / 'state' / 'synthetic-audit-denied').exists():
            raise RuntimeError('unexpected process or network attempt')
        receipt.update(result='passed', fixture_http_requests=5, synthetic_model_requests=3, metadata_404_requests=2, external_provider_requests=0)
    except Exception as error:
        receipt.update(result='failed', error='synthetic_bridge_check_failed',
                       error_type=type(error).__name__, error_code=getattr(error, 'code', None))
        raise
    finally:
        # Leave a bounded independent cleanup budget even after the test deadline.
        signal.signal(signal.SIGALRM, signal.SIG_DFL)
        signal.alarm(60)
        cleanup = []
        if server is not None:
            server.shutdown(); server.server_close(); thread.join(timeout=3)
            cleanup.append(not thread.is_alive())
        if executor is not None:
            try: executor.close()
            except Exception: cleanup.append(False)
        for spec in specs:
            try: cleanup.append(engine.stop(spec['idempotency_key'], args.image))
            except Exception: cleanup.append(False)
        # Inventory even after an uncertain creation response. Absence requires
        # a successful exact-name list; only this fresh label/image can be removed.
        for kind, name, issued in (('container', provider, provider_issued), ('network', network, network_issued)):
            if not issued: continue
            try:
                cleanup.append(remove_owned(command, kind, name, identifier, args.image))
            except Exception: cleanup.append(False)
        receipt['owned_resources_removed'] = all(cleanup)
        if not all(cleanup): receipt.update(result='failed', error='synthetic_cleanup_failed')
        write(root / 'receipt.json', receipt)
        signal.alarm(0)
        if not all(cleanup): raise RuntimeError('synthetic cleanup failed')
    print(json.dumps({'result': receipt['result'], 'receipt': str(root / 'receipt.json'), 'checks': receipt['checks'],
                      'scope': receipt['scope'], 'external_provider_requests': 0}))


if __name__ == '__main__':
    try: main()
    except BaseException:
        print(json.dumps({'result': 'failed', 'scope': 'synthetic bridge fixture', 'error': 'see retained private receipt'}))
        raise SystemExit(1) from None
