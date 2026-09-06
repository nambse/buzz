"""Installed pinned AIAgent + real SDK/HTTPX + durable workspace tool fixture.

Only socket transport and model/OS metadata are synthetic. No live provider,
credential, container launch or host workspace is used. Run inside the new image.
"""
import base64
import hashlib
import json
import sqlite3
import sys
import tempfile
import threading
import time
from pathlib import Path
from unittest.mock import patch
from uuid import uuid4

from ortak_hermes_bridge import HERMES_REVISION, journal_tools
from ortak_hermes_bridge.candidate_smoke import ForbiddenSmokeIO
from ortak_hermes_bridge.hermes_candidate import execute_candidate
from ortak_hermes_bridge.journal import Journal
from ortak_hermes_bridge.service import Bridge, EMPTY_POLICY
from ortak_hermes_bridge.verify_source import verify_source
from ortak_hermes_bridge.worker import arm_deadline, prepare_home
from ortak_hermes_bridge.workspace_contract import SCHEMA, TOOL, digest


class FixtureMismatch(BaseException):
    """Do not let SDK Exception retry wrappers hide a synthetic assertion."""


def mismatch(stage, coordinates):
    """Only fixed labels, booleans and counts; never print the request or output."""
    raise FixtureMismatch(json.dumps({'fixture_stage': stage, **coordinates}, sort_keys=True))


def sse(item, ordinal):
    """Complete Responses events, parsed by the installed OpenAI SDK itself."""
    response = {'id': f'resp_fixture_{ordinal}', 'object': 'response', 'created_at': 1788585600,
                'status': 'completed', 'model': 'gpt-5.6-sol', 'output': [item], 'error': None,
                'incomplete_details': None, 'usage': {'input_tokens': 1, 'output_tokens': 1, 'total_tokens': 2}}
    events = [{'type': 'response.created', 'response': {**response, 'status': 'in_progress', 'output': [], 'usage': None}}]
    if item['type'] == 'function_call':
        events += [
            {'type': 'response.output_item.added', 'output_index': 0, 'item': {**item, 'arguments': '', 'status': 'in_progress'}},
            {'type': 'response.function_call_arguments.delta', 'item_id': item['id'], 'output_index': 0, 'delta': item['arguments']},
            {'type': 'response.function_call_arguments.done', 'item_id': item['id'], 'output_index': 0, 'arguments': item['arguments']}]
    else:
        part = item['content'][0]
        events += [
            {'type': 'response.output_item.added', 'output_index': 0, 'item': {**item, 'content': [], 'status': 'in_progress'}},
            {'type': 'response.content_part.added', 'item_id': item['id'], 'output_index': 0, 'content_index': 0, 'part': {**part, 'text': ''}},
            {'type': 'response.output_text.delta', 'item_id': item['id'], 'output_index': 0, 'content_index': 0, 'delta': part['text']},
            {'type': 'response.output_text.done', 'item_id': item['id'], 'output_index': 0, 'content_index': 0, 'text': part['text']},
            {'type': 'response.content_part.done', 'item_id': item['id'], 'output_index': 0, 'content_index': 0, 'part': part}]
    events += [{'type': 'response.output_item.done', 'output_index': 0, 'item': item},
               {'type': 'response.completed', 'response': response}]
    return ''.join(f"event: {event['type']}\ndata: {json.dumps({**event, 'sequence_number': index})}\n\n"
                   for index, event in enumerate(events)).encode()


def scenario(base, journal, root, mode):
    """Production bridge and candidate with one bounded central read fixture."""
    import httpx
    import openai._base_client as sdk_metadata
    run, company = str(uuid4()), str(uuid4())
    key = f'ortak-run:{company}:{run}'
    content = ' \tSelected immutable input canary: İstanbul, "42", \\ path.\r\n'
    input_path = root / f'input-{run}.txt'
    input_path.write_bytes(content.encode())
    binding = {'adapter': 'hermes', 'profile_ref': 'fixture', 'workspace_ref': 'fixture-input',
               'model': 'gpt-5.6-sol', 'options': {'reasoning_effort': 'high'}, 'credential_refs': []}
    spec = {'run_id': run, 'employee_id': 'fixture', 'revision_id': str(uuid4()), 'binding': binding,
            'permissions': {**EMPTY_POLICY, 'allowed_tools': ['files'], 'allowed_workspaces': ['fixture-input']},
            'input': 'Read the selected file and provide a deliverable for review.',
            'context': {'work_item_id': str(uuid4())}, 'idempotency_key': key}
    file = {'file_id': str(uuid4()), 'name': 'inputs/selected.txt', 'media_type': 'text/plain',
            'bytes': len(content.encode()), 'sha256': hashlib.sha256(content.encode()).hexdigest()}
    grant = {'format': 'ortak-workspace-read/v1', 'company_id': company, 'project_id': str(uuid4()),
             'employee_id': 'fixture', 'workspace_ref': 'fixture-input', 'revision': str(uuid4()), 'files': [file]}
    grant['manifest_hash'] = digest(grant)
    bridge = Bridge(journal, company, [{'employee_id': 'fixture', 'binding': binding}])
    body = {'company_id': company, 'spec': spec, 'workspace': grant}
    bridge.validate(body)
    journal.reserve(spec, workspace=grant)
    control = {'company_id': company, 'run_id': run, 'idempotency_key': key}
    calls, clients, reads, errors, acknowledgements = [], [], [], [], []
    done = threading.Event()

    def central():
        try:
            deadline = time.monotonic() + 12
            while not done.is_set() and time.monotonic() < deadline:
                request = bridge.dispatch('POST', '/v1/runs/tools/pending', control)['request']
                if request is None:
                    done.wait(0.01)
                    continue
                if reads or acknowledgements:
                    raise RuntimeError('duplicate central fixture request')
                if mode == 'cancel':
                    journal.request_cancel(key)
                    return
                if mode == 'refuse':
                    result = {'status': 'failed', 'code': 'authority_changed'}
                else:
                    data = input_path.read_bytes()
                    reads.append(len(data))
                    result = {'status': 'completed', 'content': data.decode(), 'bytes': len(data),
                              'sha256': hashlib.sha256(data).hexdigest(), 'name': file['name']}
                request_body = {**control, 'request': request, 'result': result}
                ack = bridge.dispatch('POST', '/v1/runs/tools/resolve', request_body)
                if bridge.dispatch('POST', '/v1/runs/tools/resolve', request_body) != ack:
                    raise RuntimeError('lost ACK replay changed')
                acknowledgements.append(request_body)
                return
        except BaseException as error:
            errors.append(type(error).__name__)

    def mock_wire(request):
        wire = json.loads(request.content)
        expected = [{'type': 'function', **SCHEMA['function'], 'strict': False}]
        selected = {'route': str(request.url) == 'https://chatgpt.com/backend-api/codex/responses',
                    'method': request.method == 'POST', 'credential': request.headers.get('authorization') == 'Bearer ' + token,
                    'request_budget': len(calls) < 2, 'model': wire.get('model') == binding['model'],
                    'effort': wire.get('reasoning', {}).get('effort') == 'high',
                    'tools': wire.get('tools') == expected, 'sequential': wire.get('parallel_tool_calls') is False}
        if not all(selected.values()):
            mismatch('selection', {'request_count': len(calls), 'checks': selected})
        calls.append(True)
        if len(calls) == 1:
            item = {'type': 'function_call', 'id': 'fc_workspace', 'call_id': 'call_workspace',
                    'status': 'completed', 'name': 'terminal' if mode == 'forged' else TOOL,
                    'arguments': json.dumps({'file_id': file['file_id']})}
        else:
            results = [entry for entry in wire.get('input', []) if entry.get('type') == 'function_call_output']
            output = results[0].get('output') if len(results) == 1 else None
            expected_result = {'status': 'completed', 'content': content, 'file_id': file['file_id'],
                               'bytes': file['bytes'], 'sha256': file['sha256'], 'name': file['name']}
            expected_output = json.dumps(expected_result, sort_keys=True, separators=(',', ':'), ensure_ascii=False)
            try:
                decoded = json.loads(output) if isinstance(output, str) else None
            except ValueError:
                decoded = None
            exact = decoded == expected_result
            if mode != 'complete' or len(results) != 1 or output != expected_output or not exact:
                mismatch('tool_result', {'request_count': len(calls), 'result_count': len(results),
                    'output_is_string': isinstance(output, str), 'output_is_array': isinstance(output, list),
                    'output_length': len(output) if isinstance(output, (str, list)) else None,
                    'canonical_envelope': output == expected_output, 'exact_envelope': exact,
                    'input_count': len(wire.get('input', []))})
            data = decoded['content'].encode()
            if data != content.encode() or len(data) != decoded['bytes'] or hashlib.sha256(data).hexdigest() != decoded['sha256']:
                mismatch('tool_result_bytes', {'content_bytes': len(data), 'expected_bytes': file['bytes']})
            item = {'type': 'message', 'id': 'msg_workspace', 'role': 'assistant', 'status': 'completed',
                    'phase': 'final_answer', 'content': [{'type': 'output_text',
                        'text': 'A deliverable based on the selected input, awaiting human review.', 'annotations': []}]}
        return httpx.Response(200, headers={'content-type': 'text/event-stream'}, content=sse(item, len(calls)))

    def fixture_client(base_url='', *, verify=True):
        if len(clients) >= 8:
            raise RuntimeError('installed fixture client ceiling exceeded')
        client = httpx.Client(transport=httpx.MockTransport(mock_wire), timeout=5, trust_env=False)
        clients.append(client)
        return client

    claims = {'exp': int(time.time()) + 3600, 'https://api.openai.com/auth': {'chatgpt_account_id': 'fixture-account'}}
    token = 'fixture-header.' + base64.urlsafe_b64encode(json.dumps(claims).encode()).decode().rstrip('=') + '.fixture-signature'
    central_thread = threading.Thread(target=central)
    central_thread.start()
    try:
        with patch.object(sdk_metadata, 'get_platform', return_value='Linux'), \
                patch.object(base, '_build_keepalive_http_client', staticmethod(fixture_client)):
            execute_candidate(spec, journal, base, 'openai-codex', token, workspace=grant)
    finally:
        done.set()
        central_thread.join(2)
        for client in clients:
            client.close()
    if central_thread.is_alive() or errors:
        raise RuntimeError('central fixture did not settle cleanly')
    if mode == 'cancel':
        # The candidate has returned; the fixture now proves its one execution
        # is stopped. Production DockerExecutor additionally proves whole-child containment.
        journal.finish_cancel(key)
    page = Journal(journal.path).events(key)
    payloads = [event['payload'] for event in page['events']]
    types = [payload['event_type'] for payload in payloads]
    expected_status = 'completed' if mode == 'complete' else 'cancelled' if mode == 'cancel' else 'failed'
    if journal.lookup(key)['status'] != expected_status or not page['terminal']:
        raise RuntimeError('installed workspace scenario did not reach its expected terminal state')
    if content in json.dumps(page) or token in json.dumps(page):
        raise RuntimeError('file or credential material escaped into public Activity')
    if mode == 'complete':
        if (len(calls) != 2 or reads != [len(content.encode())]
                or types != ['run.started', 'tool_call.started', 'file.changed', 'tool_call.completed',
                             'assistant.delta', 'delivery.intent', 'run.completed']
                or payloads[-1]['delivery_intent'] != 'silent'):
            raise RuntimeError('installed file read did not produce the exact durable workflow')
        ack = bridge.dispatch('POST', '/v1/runs/tools/resolve', acknowledgements[0])
        if not ack['acknowledged']:
            raise RuntimeError('terminal lost ACK did not replay')
    elif len(calls) != 1 or reads or 'assistant.delta' in types or 'file.changed' in types:
        raise RuntimeError('denied/cancelled tool continued to model or filesystem')
    return {'case': mode, 'status': expected_status, 'sdk_requests': len(calls), 'file_reads': len(reads)}


def main():
    """Root image gate; prints bounded facts, never fixture bodies or credentials."""
    if sys.platform != 'linux' or sqlite3.sqlite_version_info < (3, 51, 3):
        raise RuntimeError('selected Linux image and patched SQLite required')
    lock = verify_source()
    arm_deadline(75)
    attempted = []
    def audit(event, args):
        if event in {'socket.connect', 'socket.getaddrinfo', 'subprocess.Popen', 'os.system',
                     'os.exec', 'os.posix_spawn', 'os.fork'}:
            attempted.append(event)
            raise ForbiddenSmokeIO()
    sys.addaudithook(audit)
    with tempfile.TemporaryDirectory(prefix='ortak-workspace-installed-') as temporary:
        root = Path(temporary)
        prepare_home(root / 'home')
        sys.path.insert(0, '/opt/hermes')
        from run_agent import AIAgent
        from agent import model_metadata
        with patch.object(model_metadata, '_fetch_codex_oauth_context_lengths_with_source',
                          return_value=({'gpt-5.6-sol': 131072}, True)):
            journal = Journal(root / 'journal.sqlite')
            results = [scenario(AIAgent, journal, root, mode) for mode in ('complete', 'cancel', 'refuse', 'forged')]
        if attempted:
            raise RuntimeError('installed workspace fixture attempted network or subprocess access')
    print(json.dumps({'source_revision': HERMES_REVISION, 'verified_source_files': len(lock['source_files']),
                      'installed_sdk_workspace': 'passed', 'cases': results, 'live_provider_calls': 0,
                      'network_calls': 0, 'scope': 'real pinned loop/SDK/HTTPX; synthetic socket and central read'}, sort_keys=True))


if __name__ == '__main__':
    main()
