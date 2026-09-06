"""Image-only real Hermes run-loop regression with fixed provider-I/O fixtures.

This is not a model/provider smoke. It keeps real constructor, prompt building,
Responses normalization, run_conversation, tool dispatch and bridge persistence.
Only the two real provider request methods are replaced; network and subprocess
attempts fail under the same strict audit guard as the constructor smoke.
"""
import json
import sqlite3
import sys
import tempfile
from pathlib import Path
from types import SimpleNamespace
from uuid import uuid4
from ortak_hermes_bridge import HERMES_REVISION
from ortak_hermes_bridge.candidate_smoke import ForbiddenSmokeIO
from ortak_hermes_bridge.hermes_candidate import execute_candidate
from ortak_hermes_bridge.journal import Journal
from ortak_hermes_bridge.service import EMPTY_POLICY
from ortak_hermes_bridge.verify_source import verify_source
from ortak_hermes_bridge.worker import arm_deadline, prepare_home


def response(tool=False):
    """Fixed Responses shape accepted by the inspected real codex transport."""
    if tool:
        item = SimpleNamespace(type='function_call', id='fc_fixture', call_id='call_fixture',
                               status='completed', name='terminal', arguments='{"command":"never execute"}')
    else:
        item = SimpleNamespace(type='message', id='msg_fixture', role='assistant', status='completed',
                               content=[SimpleNamespace(type='output_text', text='Fixed fixture response.')])
    return SimpleNamespace(id='resp_fixture', status='completed', output=[item], model='gpt-4o-mini',
                           usage=None, error=None, incomplete_details=None)


def main():
    """Invoke production execute_candidate with real AIAgent and explicit I/O DI."""
    if sqlite3.sqlite_version_info < (3, 51, 3):
        raise RuntimeError('patched SQLite required')
    lock = verify_source()
    arm_deadline(40)
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
        for boundary in ('_interruptible_api_call', '_interruptible_streaming_api_call'):
            if not callable(getattr(AIAgent, boundary, None)):
                raise RuntimeError('pinned provider request boundary changed')
        journal = Journal(Path(temporary) / 'journal.sqlite')
        for tool in (False, True):
            run, company = str(uuid4()), str(uuid4())
            key = f'ortak-run:{company}:{run}'
            spec = {'run_id': run, 'employee_id': 'fixture', 'revision_id': str(uuid4()),
                    'binding': {'model': 'gpt-4o-mini', 'options': {}}, 'permissions': EMPTY_POLICY,
                    'input': 'Reply with the fixed fixture answer.', 'context': {}, 'idempotency_key': key}
            calls = []
            def fixture_request(self, *args, **kwargs):
                if journal.lookup(key)['status'] != 'running' or calls:
                    raise RuntimeError('provider fixture called outside admission or more than once')
                calls.append(True)
                return response(tool)
            fixture_agent = type('ProviderFixtureAIAgent', (AIAgent,), {
                '_interruptible_api_call': fixture_request,
                '_interruptible_streaming_api_call': fixture_request,
            })
            journal.reserve(spec)
            execute_candidate(spec, journal, fixture_agent, 'openai', 'fixture-only-not-a-provider-key')
            page = journal.events(key)
            state = journal.lookup(key)['status']
            if not page['terminal'] or len(calls) != 1:
                raise RuntimeError('real run loop did not reach a single fixture and durable terminal')
            event_types = [event['payload']['event_type'] for event in page['events']]
            if tool:
                failures = [event['payload'] for event in page['events'] if event['payload']['event_type'] == 'run.failed']
                if state != 'failed' or 'delivery.intent' in event_types or not failures:
                    raise RuntimeError('real loop did not deny forged tool output')
                if 'policy_denied' not in json.dumps(failures):
                    raise RuntimeError('tool rejection did not bind the production execution guard')
            elif state != 'completed' or event_types != ['run.started', 'assistant.delta', 'delivery.intent', 'run.completed']:
                raise RuntimeError('real run-loop return did not commit the expected bridge output')
            counts.append(len(calls))
        if attempted:
            raise RuntimeError('real run loop attempted network or subprocess access')
    print(json.dumps({'source_revision': HERMES_REVISION, 'verified_source_files': len(lock['source_files']),
                      'real_run_loop': 'passed', 'real_loop_tool_denial': 'passed',
                      'fixture_responses': sum(counts), 'provider_requests': 0,
                      'network_calls': 0, 'scope': 'real Hermes loop with provider I/O fixtures'}, sort_keys=True))

if __name__ == '__main__':
    main()
