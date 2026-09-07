"""Real recovery-only HTTP/CLI check. No Docker socket or provider credentials.

Run with the patched controller image's Python. The production CLI executes in
separate processes; only fixture configuration and HTTP traffic are supplied.
"""
import http.client
import json
import os
from pathlib import Path
import secrets
import signal
import socket
import sqlite3
import subprocess
import sys
import tempfile
import time
from uuid import uuid4

from ortak_hermes_bridge import API_VERSION
from ortak_hermes_bridge.service import EMPTY_POLICY

MAX_RESPONSE = 16 * 1024


def require(condition, code):
    """Fail with a fixed code; never print response bodies or credentials."""
    if not condition:
        raise RuntimeError(code)


def request(port, token, method, path, body=None, authenticated=True):
    """Use only loopback, bounded reads and no redirect/proxy behavior."""
    headers = {'Content-Type': 'application/json'}
    if authenticated:
        headers['Authorization'] = 'Bearer ' + token
    encoded = json.dumps(body).encode() if body is not None else None
    connection = http.client.HTTPConnection('127.0.0.1', port, timeout=2)
    try:
        connection.request(method, path, body=encoded, headers=headers)
        response = connection.getresponse()
        data = response.read(MAX_RESPONSE + 1)
        require(len(data) <= MAX_RESPONSE, 'http_response_too_large')
        return response.status, json.loads(data)
    finally:
        connection.close()


def stop(process, force=False):
    """Only stop/reap the child created by this harness, with a bounded wait."""
    if process.poll() is None:
        if force:
            process.kill()
        else:
            process.terminate()
        try:
            process.wait(timeout=3)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait(timeout=3)
    require(process.poll() is not None, 'controller_not_reaped')


def start(root, token):
    """Invoke the actual production CLI, explicitly omitting execution opt-in."""
    with socket.socket() as reservation:
        reservation.bind(('127.0.0.1', 0))
        port = reservation.getsockname()[1]
    command = [sys.executable, '-m', 'ortak_hermes_bridge',
               '--config', str(root / 'config.json'),
               '--token-file', str(root / 'service-token'),
               '--journal', str(root / 'journal.sqlite'), '--port', str(port)]
    environment = {'PATH': '/opt/hermes/.venv/bin:/usr/local/bin:/usr/bin:/bin',
                   'HOME': str(root), 'PYTHONPATH': '/opt/bridge',
                   'LD_LIBRARY_PATH': '/opt/sqlite-fixed/lib',
                   'PYTHONDONTWRITEBYTECODE': '1', 'LANG': 'C.UTF-8'}
    process = subprocess.Popen(command, env=environment, cwd=root,
                               stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
                               stderr=subprocess.DEVNULL)
    deadline = time.monotonic() + 8
    try:
        while time.monotonic() < deadline:
            require(process.poll() is None, 'controller_exited_before_ready')
            try:
                status, payload = request(port, token, 'GET', '/v1/capabilities')
                if status == 200:
                    require(payload.get('api_version') == API_VERSION, 'wrong_api_version')
                    return process, port
            except (OSError, http.client.HTTPException):
                pass
            time.sleep(0.05)
        raise RuntimeError('controller_start_deadline')
    except BaseException:
        stop(process, force=True)
        raise


def run(root):
    """Exercise authenticated live HTTP, durable tombstones and process restart."""
    require(sqlite3.sqlite_version_info >= (3, 51, 3), 'sqlite_wal_fix_required')
    require(not Path('/var/run/docker.sock').exists(), 'unexpected_docker_socket')
    require(not Path('/profile').exists(), 'unexpected_provider_profile')
    company, run_id = str(uuid4()), str(uuid4())
    key, ref = f'ortak-run:{company}:{run_id}', f'ortak:{company}:{run_id}'
    token = secrets.token_hex(32)
    binding = {'adapter': 'hermes', 'profile_ref': 'http-recovery-fixture',
               'model': 'unused-fixture-model', 'workspace_ref': 'none',
               'credential_refs': [], 'options': {}}
    spec = {'run_id': run_id, 'employee_id': 'http-recovery-fixture',
            'revision_id': str(uuid4()), 'binding': binding,
            'permissions': EMPTY_POLICY, 'input': 'never execute this fixture',
            'context': {'conversation_ref': None, 'reply_to_message_id': None,
                        'work_item_id': None, 'memory_context': []},
            'idempotency_key': key}
    control = {'company_id': company, 'run_id': run_id, 'idempotency_key': key}
    configuration = {'company_id': company,
                     'profiles': [{'employee_id': spec['employee_id'], 'binding': binding}]}
    for name, value in [('config.json', json.dumps(configuration)), ('service-token', token)]:
        with (root / name).open('x') as output:
            output.write(value)
        (root / name).chmod(0o600)
    process = None
    try:
        process, port = start(root, token)
        status, caps = request(port, token, 'GET', '/v1/capabilities')
        require(status == 200 and caps.get('adapter') == 'hermes', 'capabilities_failed')
        require('run_start' not in caps['capabilities'] and
                {'run_lookup', 'run_events', 'run_cancel_start'}.issubset(caps['capabilities']),
                'recovery_capabilities_wrong')
        require(request(port, token, 'GET', '/v1/capabilities', authenticated=False)[0] == 401,
                'unauthenticated_request_accepted')
        status, health = request(port, token, 'POST', '/v1/profiles/inspect',
                                 {'company_id': company, 'binding': binding})
        require(status == 200 and health == {'profile_ref': binding['profile_ref'], 'healthy': False,
                                             'credential_references': []},
                'unavailable_profile_reported_healthy')
        require(request(port, token, 'POST', '/v1/runs/lookup', control)[0] == 404,
                'lookup_created_run')
        status, refused = request(port, token, 'POST', '/v1/runs', {'company_id': company, 'spec': spec})
        require(status == 503 and refused == {'error': 'executor_unavailable'},
                'unavailable_executor_started')
        require(request(port, token, 'POST', '/v1/runs/lookup', control)[0] == 404,
                'refused_start_reserved_run')
        wrong = {**control, 'company_id': str(uuid4())}
        require(request(port, token, 'POST', '/v1/runs/cancel', {**wrong, 'reason': 'fixture'})[0] == 404,
                'foreign_company_accepted')
        status, cancelled = request(port, token, 'POST', '/v1/runs/cancel',
                                    {**control, 'reason': 'fixture cancelled before delayed start'})
        require(status == 200 and cancelled == {'runtime_run_ref': ref, 'outcome': 'cancelled'},
                'cancel_before_start_failed')
        status, receipt = request(port, token, 'POST', '/v1/runs', {'company_id': company, 'spec': spec})
        require(status == 200 and receipt.get('runtime_run_ref') == ref and receipt.get('status') == 'cancelled',
                'delayed_start_ignored_tombstone')
        status, page = request(port, token, 'GET', f'/v1/runs/{ref}/events?after=0&limit=1')
        require(status == 200 and page.get('terminal') is True and len(page.get('events', [])) == 1,
                'terminal_replay_failed')
        require(page['events'][0]['cursor'] == '1' and
                page['events'][0]['payload']['event_type'] == 'run.cancelled', 'wrong_cancellation_event')
        stop(process, force=True)
        require(process.returncode == -signal.SIGKILL, 'controller_sigkill_not_observed')
        process, port = start(root, token)
        require(request(port, token, 'POST', '/v1/runs/lookup', control) == (200, receipt),
                'restart_lookup_changed_receipt')
        require(request(port, token, 'POST', '/v1/runs', {'company_id': company, 'spec': spec}) == (200, receipt),
                'restart_delayed_start_changed_receipt')
        require(request(port, token, 'GET', f'/v1/runs/{ref}/events?after=0&limit=1') == (200, page),
                'restart_replay_changed')
        require(request(port, token, 'GET', f'/v1/runs/{ref}/events?after=1&limit=1') ==
                (200, {'events': [], 'terminal': True}), 'exclusive_cursor_repeated_event')
        require(request(port, token, 'GET', f'/v1/runs/{ref}/events?after=2&limit=1')[0] == 409,
                'cursor_ahead_accepted')
        require(request(port, token, 'POST', '/v1/runs/cancel', {**control, 'reason': 'retry'}) ==
                (200, {'runtime_run_ref': ref, 'outcome': 'already_terminal'}),
                'repeated_cancel_changed_outcome')
    finally:
        if process is not None:
            stop(process)
    print(json.dumps({'check': 'real_controller_http_recovery', 'result': 'passed',
                      'sqlite_version': sqlite3.sqlite_version, 'service_processes': 2,
                      'forced_restart': True, 'provider_calls': 0, 'docker_socket': False,
                      'checks': ['bearer_auth', 'recovery_capabilities', 'honest_profile_health',
                                 'lookup_no_creation', 'unavailable_start_no_reservation',
                                 'company_scope', 'cancel_before_start', 'delayed_start_tombstone',
                                 'sigkill_restart_receipt', 'durable_dense_replay',
                                 'exclusive_cursor', 'idempotent_terminal_cancel']}))


def main():
    os.umask(0o077)
    with tempfile.TemporaryDirectory(prefix='ortak-http-recovery-') as directory:
        run(Path(directory))


if __name__ == '__main__':
    try:
        main()
    except Exception:
        print('real_controller_http_recovery: failed', file=sys.stderr)
        raise SystemExit(1) from None
