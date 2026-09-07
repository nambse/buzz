"""Actual bridge/SQLite/dispatcher boundaries; no provider, Docker or live files."""
import copy
import hashlib
import json
import tempfile
import threading
import time
import unittest
from concurrent.futures import ThreadPoolExecutor
from http.client import HTTPConnection
from http.server import HTTPServer
from pathlib import Path
from types import SimpleNamespace
from uuid import uuid4

from ortak_hermes_bridge import journal_tools
from ortak_hermes_bridge.hermes_candidate import execute_candidate, ToolDenied
from ortak_hermes_bridge.journal import BridgeError, Journal
from ortak_hermes_bridge.service import Bridge, EMPTY_POLICY, handler
from ortak_hermes_bridge.workspace_contract import SCHEMA, TOOL, arguments, digest
from ortak_hermes_bridge.workspace_tools import WorkspaceTransport, workspace_agent_class


def selected(spec, company, content='Workspace private canary\nİstanbul text.'):
    """Independently form the shared wire fixture, including its metadata hash."""
    file = {'file_id': str(uuid4()), 'name': 'inputs/readme.txt', 'media_type': 'text/plain',
            'bytes': len(content.encode()), 'sha256': hashlib.sha256(content.encode()).hexdigest()}
    grant = {'format': 'ortak-workspace-read/v1', 'company_id': company,
             'project_id': str(uuid4()), 'employee_id': spec['employee_id'],
             'workspace_ref': spec['binding']['workspace_ref'], 'revision': str(uuid4()), 'files': [file]}
    grant['manifest_hash'] = digest(grant)
    spec['permissions'] = {**EMPTY_POLICY, 'allowed_tools': ['files'], 'allowed_workspaces': [grant['workspace_ref']]}
    spec['context'] = {'work_item_id': str(uuid4())}
    return grant, {'status': 'completed', 'content': content, 'sha256': file['sha256'],
                   'name': file['name'], 'bytes': file['bytes']}


class Executor:
    available = True
    workspace_text_read = True
    stopped = True
    def __init__(self):
        self.starts = []
    def start(self, spec, journal, *, workspace=None):
        self.starts.append((spec, workspace))
    def stop(self, key):
        return self.stopped


class Workspace(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.path = Path(self.temp.name) / 'journal.sqlite'
        self.journal = Journal(self.path)
        self.company, self.run = str(uuid4()), str(uuid4())
        self.key = f'ortak-run:{self.company}:{self.run}'
        binding = {'adapter': 'hermes', 'profile_ref': 'fixture', 'model': 'fixture-model',
                   'workspace_ref': 'selected-input', 'credential_refs': [], 'options': {}}
        self.spec = {'run_id': self.run, 'employee_id': 'fixture', 'revision_id': str(uuid4()),
                     'binding': binding, 'permissions': EMPTY_POLICY, 'input': 'Read the selected input.',
                     'context': {}, 'idempotency_key': self.key}
        self.grant, self.result = selected(self.spec, self.company)
        self.body = {'company_id': self.company, 'spec': self.spec, 'workspace': self.grant}
        self.control = {'company_id': self.company, 'run_id': self.run, 'idempotency_key': self.key}
        self.executor = Executor()
        self.bridge = Bridge(self.journal, self.company, [{'employee_id': 'fixture', 'binding': binding}], self.executor)

    def admit(self):
        self.bridge.dispatch('POST', '/v1/runs', self.body)
        self.journal.begin_execution(self.key)

    def request(self, call_id='call_fixture', seconds=10):
        file_id = self.grant['files'][0]['file_id']
        return journal_tools.reserve(self.journal, self.key, call_id, file_id, digest({'file_id': file_id}), seconds)

    def resolve(self, request, result=None):
        return self.bridge.dispatch('POST', '/v1/runs/tools/resolve',
                                    {**self.control, 'request': request, 'result': result or self.result})

    def payloads(self):
        return [row['payload'] for row in self.journal.events(self.key)['events']]

    def test_workspace_grant_and_capability_fail_before_admission(self):
        for mutation in ('company', 'hash', 'path', 'duplicate', 'office', 'policy', 'unknown', 'unsorted'):
            body = copy.deepcopy(self.body)
            if mutation == 'company': body['workspace']['company_id'] = str(uuid4())
            if mutation == 'hash': body['workspace']['manifest_hash'] = '0' * 64
            if mutation == 'path': body['workspace']['files'][0]['name'] = 'inputs/../secret'
            if mutation == 'duplicate': body['workspace']['files'] *= 2
            if mutation == 'office': body['spec']['context'] = {'conversation_ref': str(uuid4())}
            if mutation == 'policy': body['spec']['permissions']['allowed_tools'].append('terminal')
            if mutation == 'unknown': body['workspace']['host_path'] = '/never-read'
            if mutation == 'unsorted':
                body['workspace']['files'] += [{**body['workspace']['files'][0], 'file_id': str(uuid4()), 'name': 'second.txt'}]
                body['workspace']['files'].sort(key=lambda f: f['file_id'], reverse=True)
            if mutation != 'hash':
                body['workspace']['manifest_hash'] = digest({k: v for k, v in body['workspace'].items() if k != 'manifest_hash'})
            with self.subTest(mutation=mutation), self.assertRaises(BridgeError):
                self.bridge.dispatch('POST', '/v1/runs', body)
        self.executor.workspace_text_read = False
        with self.assertRaisesRegex(BridgeError, 'unsupported_permission_policy'):
            self.bridge.dispatch('POST', '/v1/runs', self.body)
        self.assertNotIn('workspace_text_read', self.bridge.dispatch('GET', '/v1/capabilities')['capabilities'])
        self.assertIsNone(self.journal.lookup(self.key))
        self.assertEqual(self.executor.starts, [])

    def test_start_replay_pins_full_grant_across_restart(self):
        self.bridge.dispatch('POST', '/v1/runs', self.body)
        self.bridge.dispatch('POST', '/v1/runs', self.body)
        self.assertEqual(len(self.executor.starts), 1)
        changed = copy.deepcopy(self.body)
        changed['workspace']['revision'] = str(uuid4())
        changed['workspace']['manifest_hash'] = digest({k: v for k, v in changed['workspace'].items() if k != 'manifest_hash'})
        with self.assertRaisesRegex(BridgeError, 'start_conflict'):
            self.bridge.dispatch('POST', '/v1/runs', changed)
        self.assertEqual(journal_tools.workspace(Journal(self.path), self.key), self.grant)
        with self.assertRaisesRegex(BridgeError, 'workspace_start_conflict'):
            execute_candidate(self.spec, self.journal, None, 'openai', 'synthetic', workspace=changed['workspace'])

    def test_resolve_lost_ack_atomic_events_and_content_retirement(self):
        self.admit()
        request = self.request()
        self.assertEqual(journal_tools.pending(Journal(self.path), self.key), {'request': request})
        ack = self.resolve(request)
        self.assertEqual(self.resolve(request), ack)
        with self.assertRaisesRegex(BridgeError, 'unsettled_workspace_tool'):
            self.journal.complete(self.key, 'must not overtake pending content', work_output=True)
        self.assertEqual(journal_tools.consume(Journal(self.path), self.key, request), self.result)
        self.journal.complete(self.key, 'Reviewed deliverable.', work_output=True)
        self.assertEqual(self.resolve(request), ack)
        with self.journal.connection() as db:
            row = db.execute('SELECT state,result_json,result_hash FROM workspace_tool_calls').fetchone()
        self.assertEqual(tuple(row), ('consumed', None, digest(self.result)))
        payloads = self.payloads()
        self.assertEqual([p['event_type'] for p in payloads], ['run.started', 'tool_call.started', 'file.changed',
            'tool_call.completed', 'assistant.delta', 'delivery.intent', 'run.completed'])
        self.assertEqual(payloads[-1]['delivery_intent'], 'silent')
        self.assertNotIn('Workspace private canary', json.dumps(payloads))
        self.assertEqual(journal_tools.pending(self.journal, self.key), {'request': None})

    def test_changed_duplicate_and_scope_or_input_mismatch_refused(self):
        self.admit()
        request = self.request()
        for field, value in [('content', 'forged'), ('name', '../secret'), ('bytes', True), ('sha256', '0' * 64)]:
            with self.subTest(field=field), self.assertRaisesRegex(BridgeError, 'invalid_tool_result'):
                self.resolve(request, {**self.result, field: value})
        with self.assertRaisesRegex(BridgeError, 'run_not_found'):
            self.bridge.dispatch('POST', '/v1/runs/tools/pending', {**self.control, 'company_id': str(uuid4())})
        with self.assertRaisesRegex(BridgeError, 'tool_call_conflict'):
            self.resolve({**request, 'ordinal': 2})
        self.resolve(request)
        with self.assertRaisesRegex(BridgeError, 'tool_result_conflict'):
            self.resolve(request, {'status': 'failed', 'code': 'authority_changed'})

    def test_cancel_closes_pending_and_refuses_new_late_result(self):
        self.admit()
        request = self.request()
        self.executor.stopped = False
        with self.assertRaisesRegex(BridgeError, 'execution_not_stopped'):
            self.bridge.dispatch('POST', '/v1/runs/cancel', {**self.control, 'reason': 'stop'})
        self.assertEqual(self.journal.lookup(self.key)['status'], 'cancelling')
        self.assertFalse(self.journal.events(self.key)['terminal'])
        self.assertEqual(journal_tools.pending(self.journal, self.key), {'request': None})
        with self.assertRaisesRegex(BridgeError, 'tool_run_not_running'):
            self.resolve(request)
        self.executor.stopped = True
        self.bridge.dispatch('POST', '/v1/runs/cancel', {**self.control, 'reason': 'stop'})
        self.assertEqual([p['event_type'] for p in self.payloads()], ['run.started', 'tool_call.started', 'tool_call.failed', 'run.cancelled'])

    def test_cancel_resolved_content_never_releases_to_model_but_exact_ack_replays(self):
        self.admit()
        request = self.request()
        ack = self.resolve(request)
        self.journal.request_cancel(self.key)
        with self.assertRaisesRegex(BridgeError, 'tool_run_not_running'):
            journal_tools.consume(self.journal, self.key, request)
        with self.journal.connection() as db:
            self.assertIsNone(db.execute('SELECT result_json FROM workspace_tool_calls').fetchone()[0])
        self.assertEqual(self.resolve(request), ack)
        self.assertFalse(self.journal.complete(self.key, 'late', work_output=True))

    def test_recovery_requires_containment_and_never_reexecutes_tool(self):
        self.admit()
        request = self.request()
        other = Journal(self.path)
        with self.assertRaisesRegex(BridgeError, 'execution_owner_not_stopped'):
            other.recover(lambda key: False)
        self.assertEqual(journal_tools.pending(other, self.key)['request'], request)
        other.recover(lambda key: key == self.key)
        self.assertEqual(other.lookup(self.key)['status'], 'failed')
        self.assertEqual(journal_tools.pending(other, self.key), {'request': None})
        self.assertFalse(other.begin_execution(self.key))
        with self.assertRaisesRegex(BridgeError, 'tool_run_not_running'):
            self.resolve(request)

    def test_concurrent_call_and_four_call_limit_are_durable(self):
        self.admit()
        def reserve(index):
            try: return self.request(f'call_{index}')
            except BridgeError: return None
        with ThreadPoolExecutor(max_workers=2) as pool:
            requests = list(pool.map(reserve, range(2)))
        self.assertEqual(sum(r is not None for r in requests), 1)
        request = next(r for r in requests if r)
        for ordinal in range(1, 5):
            if ordinal > 1: request = self.request(f'next_{ordinal}')
            self.assertEqual(request['ordinal'], ordinal)
            self.resolve(request)
            journal_tools.consume(self.journal, self.key, request)
            with self.assertRaisesRegex(BridgeError, 'tool_call_conflict'):
                self.request(request['call_id'])
        with self.assertRaisesRegex(BridgeError, 'tool_capacity'):
            self.request('fifth')

    def test_deadline_and_atomic_result_commit_failure_do_not_release_content(self):
        self.admit()
        request = self.request(seconds=0.01)
        time.sleep(0.02)
        with self.assertRaisesRegex(BridgeError, 'tool_run_not_running'):
            self.resolve(request)
        self.assertEqual(journal_tools.pending(self.journal, self.key), {'request': None})
        with self.journal.connection() as db:
            db.execute('UPDATE workspace_tool_calls SET deadline=?', (time.time() + 10,))
            db.execute("CREATE TRIGGER reject_tool_event BEFORE INSERT ON events WHEN json_extract(NEW.payload,'$.event_type')='tool_call.completed' BEGIN SELECT RAISE(ABORT,'test commit rejection'); END")
        with self.assertRaisesRegex(Exception, 'test commit rejection'):
            self.resolve(request)
        with self.journal.connection() as db:
            self.assertEqual(tuple(db.execute('SELECT state,result_json,result_hash FROM workspace_tool_calls').fetchone()), ('pending', None, None))
        self.assertEqual([p['event_type'] for p in self.payloads()], ['run.started', 'tool_call.started'])

    def test_raw_and_normalized_transport_keep_only_exact_schema_and_function(self):
        file_id = self.grant['files'][0]['file_id']
        valid = {'type': 'function_call', 'name': TOOL, 'call_id': 'call_good', 'arguments': json.dumps({'file_id': file_id})}
        def deny(): raise ToolDenied()
        upstream = SimpleNamespace(validate_response=lambda value: True,
            normalize_response=lambda value, **kw: SimpleNamespace(tool_calls=[SimpleNamespace(id='call_good',
                function=SimpleNamespace(name=TOOL, arguments=json.dumps({'file_id': file_id})))]),
            build_kwargs=lambda model, messages, tools, **kw: {'model': model, 'tools': tools})
        transport = WorkspaceTransport(upstream, deny)
        self.assertTrue(transport.validate_response({'output': [valid]}))
        self.assertEqual(transport.build_kwargs('fixture', [], [SCHEMA])['tools'], [SCHEMA])
        self.assertEqual(len(transport.normalize_response({'output': [valid]}).tool_calls), 1)
        bad = [{**valid, 'name': 'terminal'}, {**valid, 'type': 'web_search_call'},
               {**valid, 'arguments': '{"file_id":"' + file_id + '","file_id":"' + file_id + '"}'},
               {**valid, 'arguments': json.dumps({'file_id': file_id, 'path': '/never'})},
               {**valid, 'call_id': '../secret'}]
        for item in bad:
            with self.subTest(item=item), self.assertRaises(ToolDenied):
                transport.validate_response({'output': [item]})
        with self.assertRaises(ToolDenied): transport.validate_response({'output': [valid, valid]})
        with self.assertRaises(ToolDenied): transport.build_kwargs('fixture', [], [])
        with self.assertRaises(BridgeError): arguments('{"file_id":"' + file_id + '","x":0}')

    def test_dispatcher_actual_authenticated_http_pull_resolve_and_empty_bypass_guards(self):
        server = HTTPServer(('127.0.0.1', 0), handler(self.bridge, 'workspace-fixture-service-token-32chars'))
        server_thread = threading.Thread(target=server.serve_forever)
        server_thread.start()
        def cleanup():
            server.shutdown(); server.server_close(); server_thread.join(2)
        self.addCleanup(cleanup)
        def http(path, body, token='workspace-fixture-service-token-32chars'):
            connection = HTTPConnection('127.0.0.1', server.server_port, timeout=2)
            try:
                connection.request('POST', path, json.dumps(body), {'Authorization': 'Bearer ' + token})
                response = connection.getresponse()
                return response.status, json.loads(response.read(65537))
            finally: connection.close()
        self.assertEqual(http('/v1/runs', self.body)[0], 200)
        outer = self
        class Agent:
            tools = []
            def __init__(self, **kwargs):
                outer.assertEqual(kwargs['enabled_toolsets'], [])
                outer.assertEqual(kwargs['max_iterations'], 5)
            def _get_transport(self): return None
            def run_conversation(self, text, **kwargs):
                outer.assertEqual(self.tools, [SCHEMA])
                messages = []
                self._execute_tool_calls(SimpleNamespace(tool_calls=[SimpleNamespace(id='call_http', function=SimpleNamespace(
                    name=TOOL, arguments=json.dumps({'file_id': outer.grant['files'][0]['file_id']})))]), messages, 'task', 1)
                outer.assertEqual(json.loads(messages[0]['content']),
                                  {**outer.result, 'file_id': outer.grant['files'][0]['file_id']})
                return {'completed': True, 'final_response': 'Deliverable for review.'}
        for name in ('_invoke_tool', '_execute_tool_calls', '_execute_tool_calls_sequential', '_execute_tool_calls_concurrent', '_dispatch_delegate_task'):
            setattr(Agent, name, lambda *args: outer.fail('upstream executor entered'))
        with ThreadPoolExecutor(max_workers=1) as pool:
            running = pool.submit(execute_candidate, self.spec, self.journal, Agent, 'openai', 'synthetic-key', workspace=self.grant)
            deadline = time.monotonic() + 2
            request = None
            while request is None and time.monotonic() < deadline:
                status, value = http('/v1/runs/tools/pending', self.control)
                self.assertEqual(status, 200)
                request = value['request']
                if request is None: time.sleep(0.01)
            self.assertIsNotNone(request)
            self.assertEqual(http('/v1/runs/tools/resolve', {**self.control, 'request': request, 'result': self.result}, token='bad')[0], 401)
            payload = {**self.control, 'request': request, 'result': self.result}
            status, ack = http('/v1/runs/tools/resolve', payload)
            self.assertEqual(status, 200)
            running.result(2)
            self.assertEqual(http('/v1/runs/tools/resolve', payload), (200, ack))
        self.assertEqual(self.journal.lookup(self.key)['status'], 'completed')
        guarded = workspace_agent_class(Agent, self.journal, self.key, None, None, time.monotonic() + 1)(max_iterations=5, enabled_toolsets=[])
        for name in ('_invoke_tool', '_execute_tool_calls_sequential', '_execute_tool_calls_concurrent', '_dispatch_delegate_task'):
            with self.subTest(boundary=name), self.assertRaises(ToolDenied): getattr(guarded, name)()

    def test_dispatcher_envelope_preserves_exact_bytes_through_message_trimming(self):
        class Agent:
            def _get_transport(self): return None
        for name in ('_invoke_tool', '_execute_tool_calls', '_execute_tool_calls_sequential',
                     '_execute_tool_calls_concurrent', '_dispatch_delegate_task'):
            setattr(Agent, name, lambda *args: self.fail('upstream executor entered'))
        for content in (' \n\tİstanbul "quoted" \\ text\r\n', '', '\t\r\n ', '\x01\\\"\n'):
            with self.subTest(content_bytes=len(content.encode())):
                spec = copy.deepcopy(self.spec)
                spec['run_id'] = str(uuid4())
                key = spec['idempotency_key'] = f"ortak-run:{self.company}:{spec['run_id']}"
                grant, result = selected(spec, self.company, content)
                file_id = grant['files'][0]['file_id']
                self.journal.reserve(spec, workspace=grant)
                self.journal.begin_execution(key)
                agent = workspace_agent_class(Agent, self.journal, key, None, None,
                                              time.monotonic() + 3)()
                message = SimpleNamespace(tool_calls=[SimpleNamespace(id='call_exact',
                    function=SimpleNamespace(name=TOOL, arguments=json.dumps({'file_id': file_id})))])
                messages = []
                with ThreadPoolExecutor(max_workers=1) as pool:
                    running = pool.submit(agent._execute_tool_calls, message, messages, 'task', 1)
                    stop = time.monotonic() + 2
                    request = None
                    while request is None and time.monotonic() < stop:
                        request = journal_tools.pending(self.journal, key)['request']
                        if request is None: time.sleep(0.01)
                    self.assertIsNotNone(request)
                    journal_tools.resolve(self.journal, key, request, result)
                    running.result(2)
                self.assertEqual(len(messages), 1)
                wire = messages[0]['content'].strip()
                expected = {**result, 'file_id': file_id}
                self.assertEqual(wire, json.dumps(expected, sort_keys=True, separators=(',', ':'), ensure_ascii=False))
                decoded = json.loads(wire)
                self.assertEqual(decoded, expected)
                data = decoded['content'].encode()
                self.assertEqual(data, content.encode())
                self.assertEqual(len(data), decoded['bytes'])
                self.assertEqual(hashlib.sha256(data).hexdigest(), decoded['sha256'])


if __name__ == '__main__':
    unittest.main()
