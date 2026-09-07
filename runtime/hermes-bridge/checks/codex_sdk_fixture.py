"""Real pinned loop/SDK/HTTPX request seam with one bounded synthetic SSE error.

Socket transport, model metadata and SDK OS-header discovery are fixtures; no real network, headers,
request text, provider body or credential values are returned as evidence.
"""
import base64
import json
import time
from uuid import uuid4
from unittest.mock import patch

from ortak_hermes_bridge.hermes_candidate import execute_candidate
from ortak_hermes_bridge.journal import BridgeError
from ortak_hermes_bridge.service import EMPTY_POLICY


def sdk_timeout_fixture(base, journal):
    import httpx
    import sys
    import openai._base_client as sdk_metadata
    if sys.platform != 'linux':
        raise RuntimeError('SDK fixture requires the selected Linux image')
    calls, clients, observed_timeouts = [], [], []
    original_factory = base._build_keepalive_http_client
    client_expected = {'connect': 15.0, 'read': None, 'write': 15.0, 'pool': 10.0}
    # build_api_kwargs -> Codex transport forwards _resolved_api_call_timeout.
    # This per-request scalar overrides the keepalive client's base policy.
    expected = {name: 1800.0 for name in client_expected}

    def mock_wire(request):
        if len(calls) >= 3:
            raise RuntimeError('SDK fixture exceeded the pinned three-request retry ceiling')
        # Inspect only fixed numeric socket budgets, never URL/headers/body.
        timeout = request.extensions.get('timeout')
        if len(observed_timeouts) < 8:
            observed_timeouts.append({name: value if value is None or type(value) in (int, float) else 'invalid'
                for name, value in (timeout.items() if isinstance(timeout, dict) else [])
                if name in expected})
        if timeout != expected:
            raise RuntimeError('actual SDK request timeout differs from selected per-call policy')
        calls.append(dict(timeout))
        class ErrorStream(httpx.SyncByteStream):
            def __iter__(self):
                # HTTP200 SSE API error is raised by the real OpenAI SDK as
                # APIError, without an HTTP status. This matches the ambiguous
                # first-error category seen in the selected live run.
                yield b'data: {"error":{"message":"Synthetic request timed out","type":"server_error","code":"timeout"}}\n\n'
        return httpx.Response(200, headers={'content-type': 'text/event-stream'}, stream=ErrorStream())

    def fixture_factory(base_url='', *, verify=True):
        original = original_factory(base_url, verify=verify)
        if original is None:
            raise RuntimeError('selected keepalive client could not be constructed')
        timeout = original.timeout
        snapshot = {name: getattr(timeout, name) for name in ('connect', 'read', 'write', 'pool')}
        original.close()
        if snapshot != client_expected:
            raise RuntimeError('selected client timeout changed')
        if len(clients) >= 8:
            raise RuntimeError('SDK fixture exceeded bounded client creation')
        clients.append(snapshot)
        return httpx.Client(transport=httpx.MockTransport(mock_wire), timeout=timeout, trust_env=False)

    run, company = str(uuid4()), str(uuid4())
    key = f'ortak-run:{company}:{run}'
    spec = {'run_id': run, 'employee_id': 'fixture', 'revision_id': str(uuid4()),
            'binding': {'model': 'gpt-6-astra', 'options': {'reasoning_effort': 'max'}},
            'permissions': EMPTY_POLICY, 'input': 'Reply with the fixed fixture answer.',
            'context': {}, 'idempotency_key': key}
    claims = {'exp': int(time.time()) + 3600,
              'https://api.openai.com/auth': {'chatgpt_account_id': 'fixture-account'}}
    token = 'fixture-header.' + base64.urlsafe_b64encode(json.dumps(claims).encode()).decode().rstrip('=') + '.fixture-signature'
    journal.reserve(spec)
    # SDK client copies rediscover OS headers through platform.platform(), which
    # launches uname on Python 3.13. Pin only that non-provider metadata seam;
    # the process/network audit remains enabled for the entire real request.
    with patch.object(sdk_metadata, 'get_platform', return_value='Linux'), \
            patch.object(base, '_build_keepalive_http_client', staticmethod(fixture_factory)):
        try:
            execute_candidate(spec, journal, base, 'openai-codex', token)
        except BridgeError as error:
            if error.code != 'provider_incomplete':
                raise RuntimeError('SDK SSE error was masked before terminal result validation') from None
    with journal.connection() as db:
        row = db.execute('SELECT diagnostic FROM private_failure_diagnostics WHERE start_key=?', (key,)).fetchone()
    if row is None or journal.lookup(key)['status'] != 'failed':
        raise RuntimeError('SDK SSE error did not leave a terminal private diagnostic')
    diagnostic = json.loads(row[0])
    original = diagnostic.get('provider_failure', {})
    if len(calls) != 3 or original.get('kind') != 'provider_api' or original.get('http_status') is not None or original.get('reason') != 'timeout':
        print(json.dumps({'sdk_fixture_diagnostic': {'request_count': len(calls), 'client_count': len(clients),
            'observed_timeouts': observed_timeouts, 'kind': original.get('kind'),
            'reason': original.get('reason'), 'http_status': original.get('http_status')}}, sort_keys=True))
        raise RuntimeError('SDK SSE error did not preserve exact class and bounded retry behavior')
    if 'Synthetic request' in row[0] or token in row[0] or 'provider_failure' in json.dumps(journal.events(key)):
        raise RuntimeError('SDK fixture raw context escaped the private diagnostic boundary')
    return {'sdk_sse_error_fixture': 'passed', 'sdk_request_count': len(calls),
            'sdk_client_timeout': client_expected, 'sdk_request_timeout': expected, 'sdk_sse_error_kind': original['kind'],
            'sdk_sse_error_reason': original['reason']}
