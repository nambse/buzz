"""Image-only real Hermes run-loop regression with fixed provider-I/O fixtures.

This is not a live OAuth/model/provider smoke. Model metadata is an explicit fixture. It keeps real constructor, prompt building,
Responses normalization, run_conversation, tool dispatch and bridge persistence.
Four cases replace the two provider request methods. The fifth uses the real SDK
with a socket transport fixture and fixed Linux OS-header metadata. Network and
subprocess attempts fail under the strict constructor-smoke audit guard.
"""
import json
import base64
import time
from unittest.mock import patch
import sqlite3
import sys
import tempfile
from pathlib import Path
from types import SimpleNamespace
from uuid import uuid4
from ortak_hermes_bridge import HERMES_REVISION
from ortak_hermes_bridge.candidate_smoke import ForbiddenSmokeIO
from ortak_hermes_bridge.hermes_candidate import execute_candidate
from ortak_hermes_bridge.journal import BridgeError, Journal
from ortak_hermes_bridge.service import EMPTY_POLICY
from ortak_hermes_bridge.verify_source import verify_source
from ortak_hermes_bridge.worker import arm_deadline, prepare_home


def response(tool=False, long=False):
    """Fixed Responses shape accepted by the inspected real codex transport."""
    if tool:
        item = SimpleNamespace(type='function_call', id='fc_fixture', call_id='call_fixture',
                               status='completed', name='terminal', arguments='{"command":"never execute"}')
    else:
        text = ('A reviewed project plan has clear owners, bounded tasks, and explicit human acceptance. ' * 12
                if long else 'Fixed fixture response.')
        item = SimpleNamespace(type='message', id='msg_fixture', role='assistant', status='completed',
                               phase='final_answer', content=[SimpleNamespace(type='output_text', text=text)])
    return SimpleNamespace(id='resp_fixture', status='completed', output=[item], model='gpt-6-astra',
                           usage=None, error=None, incomplete_details=None)


def main():
    """Invoke production execute_candidate with real AIAgent and explicit I/O DI."""
    if sqlite3.sqlite_version_info < (3, 51, 3):
        raise RuntimeError('patched SQLite required')
    lock = verify_source()
    arm_deadline(60)
    attempted = []
    def audit(event, args):
        if event in {'socket.connect', 'socket.getaddrinfo', 'subprocess.Popen',
                     'os.system', 'os.exec', 'os.posix_spawn', 'os.fork'}:
            attempted.append(event)
            raise ForbiddenSmokeIO()
    sys.addaudithook(audit)
    counts = []
    with tempfile.TemporaryDirectory(prefix='ortak-run-loop-') as temporary:
        prepare_home(Path(temporary) / 'home')
        sys.path.insert(0, '/opt/hermes')
        from run_agent import AIAgent
        # Explicit model metadata fixture: this is not an account entitlement proof.
        from agent import model_metadata
        metadata_patch = patch.object(model_metadata, '_fetch_codex_oauth_context_lengths_with_source',
                                      return_value=({'gpt-6-astra': 131072}, True))
        metadata_patch.start()
        for boundary in ('_interruptible_api_call', '_interruptible_streaming_api_call'):
            if not callable(getattr(AIAgent, boundary, None)):
                raise RuntimeError('pinned provider request boundary changed')
        journal = Journal(Path(temporary) / 'journal.sqlite')
        for tool, long, failure in ((False, False, False), (False, True, False), (True, False, False), (False, False, True)):
            run, company = str(uuid4()), str(uuid4())
            key = f'ortak-run:{company}:{run}'
            spec = {'run_id': run, 'employee_id': 'fixture', 'revision_id': str(uuid4()),
                    'binding': {'model': 'gpt-6-astra', 'options': {'reasoning_effort': 'max'}}, 'permissions': EMPTY_POLICY,
                    'input': 'Reply with the fixed fixture answer.', 'context': {}, 'idempotency_key': key}
            calls = []
            def fixture_request(self, *args, **kwargs):
                if journal.lookup(key)['status'] != 'running' or calls:
                    raise RuntimeError('provider fixture called outside admission or more than once')
                if not args or args[0].get('model') != 'gpt-6-astra' or args[0].get('reasoning', {}).get('effort') != 'max':
                    raise RuntimeError('actual Codex wire model/effort changed')
                if self._try_refresh_codex_client_credentials() or self._try_refresh_env_client_credentials() or self._try_activate_fallback():
                    raise RuntimeError('worker enabled ambient credential recovery')
                for retry_bit in (False, True):
                    if self._recover_with_credential_pool(status_code=401, has_retried_429=retry_bit) != (False, retry_bit):
                        raise RuntimeError('worker changed the pinned credential recovery return contract')
                calls.append(True)
                if failure:
                    import httpx
                    from openai import AuthenticationError
                    request = httpx.Request('POST', 'https://example.invalid/fixture')
                    raise AuthenticationError('private fixture failure text',
                        response=httpx.Response(401, request=request), body={'private': 'fixture provider body'})
                return response(tool, long)
            fixture_agent = type('ProviderFixtureAIAgent', (AIAgent,), {
                '_interruptible_api_call': fixture_request,
                '_interruptible_streaming_api_call': fixture_request,
            })
            journal.reserve(spec)
            claims = {'exp': int(time.time()) + 3600, 'https://api.openai.com/auth': {'chatgpt_account_id': 'fixture-account'}}
            middle = base64.urlsafe_b64encode(json.dumps(claims).encode()).decode().rstrip('=')
            token = 'fixture-header.' + middle + '.fixture-signature'
            try:
                execute_candidate(spec, journal, fixture_agent, 'openai-codex', token)
            except BridgeError as error:
                if not failure or error.code != 'provider_incomplete':
                    raise RuntimeError('real provider error was masked before terminal result validation') from None
            page = journal.events(key)
            state = journal.lookup(key)['status']
            if not page['terminal'] or len(calls) != 1:
                raise RuntimeError('real run loop did not reach a single fixture and durable terminal')
            event_types = [event['payload']['event_type'] for event in page['events']]
            if failure:
                with journal.connection() as db:
                    raw = db.execute('SELECT diagnostic FROM private_failure_diagnostics WHERE start_key=?', (key,)).fetchone()[0]
                diagnostic = json.loads(raw)
                original = diagnostic.get('provider_failure', {})
                if state != 'failed' or diagnostic['kind'] != 'bridge' or diagnostic['boundary'] != 'provider_incomplete':
                    raise RuntimeError('real provider failure did not preserve the incomplete terminal result')
                if original.get('kind') != 'provider_auth' or original.get('http_status') != 401 or original.get('reason') != 'auth':
                    raise RuntimeError('original provider failure was not captured before Hermes recovery')
                if 'private fixture' in raw or 'fixture provider body' in raw or 'provider_failure' in json.dumps(page):
                    raise RuntimeError('provider diagnostic leaked raw context or entered public events')
            elif tool:
                failures = [event['payload'] for event in page['events'] if event['payload']['event_type'] == 'run.failed']
                if state != 'failed' or 'delivery.intent' in event_types or not failures:
                    raise RuntimeError('real loop did not deny forged tool output')
                if 'policy_denied' not in json.dumps(failures):
                    raise RuntimeError('tool rejection did not bind the production execution guard')
            elif state != 'completed' or event_types != ['run.started', 'assistant.delta', 'delivery.intent', 'run.completed']:
                raise RuntimeError('real run-loop return did not commit the expected bridge output')
            elif long and len(page['events'][1]['payload']['delta']['text'].split()) < 100:
                raise RuntimeError('long final-answer fixture did not preserve complete text')
            counts.append(len(calls))
        from codex_sdk_fixture import sdk_timeout_fixture
        sdk_evidence = sdk_timeout_fixture(AIAgent, journal)
        metadata_patch.stop()
        if attempted:
            raise RuntimeError('real run loop attempted network or subprocess access')
    print(json.dumps({**sdk_evidence, 'source_revision': HERMES_REVISION, 'verified_source_files': len(lock['source_files']),
                      'real_codex_constructor_and_loop': 'passed', 'wire_model': 'gpt-6-astra', 'wire_effort': 'max', 'metadata_source': 'explicit fixture', 'real_loop_tool_denial': 'passed',
                      'fixture_responses': sum(counts), 'provider_requests': 0,
                      'long_final_answer_fixture': 'passed', 'provider_failure_recovery_fixture': 'passed',
                      'network_calls': 0, 'scope': 'real Codex Hermes loop with model metadata and provider I/O fixtures; not OAuth health'}, sort_keys=True))

if __name__ == '__main__':
    main()
