"""The production candidate persists only bounded, closed failure coordinates."""
import copy
import json
import sqlite3
import tempfile
import unittest
from pathlib import Path
from uuid import uuid4
from enum import Enum

from ortak_hermes_bridge.failure_diagnostics import FailureStage, HERMES_FILES, validate_diagnostic
from ortak_hermes_bridge.hermes_candidate import execute_candidate, guarded_agent_class
from ortak_hermes_bridge.journal import BridgeError, Journal
from ortak_hermes_bridge.service import EMPTY_POLICY


class Diagnostics(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.path = Path(temporary.name) / 'journal.sqlite'
        self.journal = Journal(self.path)
        run, company = str(uuid4()), str(uuid4())
        self.key = f'ortak-run:{company}:{run}'
        self.spec = {'idempotency_key': self.key, 'run_id': run, 'employee_id': 'fixture',
                     'revision_id': str(uuid4()), 'permissions': EMPTY_POLICY,
                     'binding': {'model': 'fixture-model', 'options': {}},
                     'input': 'private user input', 'context': {}}
        self.journal.reserve(self.spec)

    def private(self):
        with self.journal.connection() as db:
            row = db.execute('SELECT diagnostic FROM private_failure_diagnostics WHERE start_key=?', (self.key,)).fetchone()
            return json.loads(row[0]) if row else None

    def agent(self, operation):
        class Agent:
            tools = []
            def __init__(inner, **kwargs):
                pass
            def _get_transport(inner):
                return None
            def run_conversation(inner, *args, **kwargs):
                return operation()
        for name in ('_invoke_tool', '_execute_tool_calls', '_execute_tool_calls_sequential',
                     '_execute_tool_calls_concurrent', '_dispatch_delegate_task'):
            setattr(Agent, name, lambda *args: None)
        return Agent

    def test_candidate_keeps_known_boundary_private_while_public_code_stays_closed(self):
        def operation():
            raise BridgeError('runtime_reasoning_changed', 503)
        with self.assertRaisesRegex(BridgeError, '^provider_failed$'):
            execute_candidate(self.spec, self.journal, self.agent(operation), 'openai', 'selected-private-token')
        value = self.private()
        self.assertEqual({k: value[k] for k in ('stage', 'kind', 'boundary')},
                         {'stage': 'conversation_run', 'kind': 'bridge', 'boundary': 'runtime_reasoning_changed'})
        public = json.dumps([self.journal.lookup(self.key), self.journal.events(self.key)])
        for absent in ('runtime_reasoning_changed', 'conversation_run', 'private user input', 'selected-private-token', 'frames'):
            self.assertNotIn(absent, public)
        execute_candidate(self.spec, self.journal, self.agent(lambda: self.fail('terminal replay executed')), 'openai', 'selected-private-token')
        self.assertEqual(self.private(), value)

    def test_pinned_coordinates_exclude_source_text_exception_arguments_and_untrusted_paths(self):
        class Sensitive(RuntimeError):
            def __str__(self):
                raise AssertionError('exception must never be stringified')
        namespace = {'Sensitive': Sensitive}
        # No file is read or created. This supplies a traceback code coordinate
        # and malicious local values through the real candidate failure path.
        exec(compile("def fail():\n secret='private frame local'\n raise Sensitive('secret exception detail')\n",
                     '/opt/hermes/agent/conversation_loop.py', 'exec'), namespace)
        with self.assertRaisesRegex(BridgeError, '^provider_failed$'):
            execute_candidate(self.spec, self.journal, self.agent(namespace['fail']), 'openai', 'selected-private-token')
        value = self.private()
        self.assertEqual(value['kind'], 'runtime')
        self.assertIn({'source': 'hermes', 'file': 'agent/conversation_loop.py', 'line': 3}, value['frames'])
        rendered = json.dumps(value)
        for absent in ('private frame local', 'secret exception detail', 'selected-private-token', '/opt/', 'Sensitive', 'fail'):
            self.assertNotIn(absent, rendered)
        for filename in ('/private/token.py', '/opt/hermes/../../secret.py', '/opt/hermes/unreviewed.py'):
            namespace = {}
            exec(compile("def invoke():\n raise KeyError('private detail')\n", filename, 'exec'), namespace)
            try:
                namespace['invoke']()
            except KeyError as error:
                sanitized = FailureStage().capture(error)
            self.assertEqual(sanitized['frames'], [])

    def test_provider_boundary_stage_binds_actual_guard_wrapper(self):
        def unused():
            self.fail('base run implementation unused')
        base = self.agent(unused)
        def request(inner, *args, **kwargs):
            raise TimeoutError('private request data')
        base._interruptible_streaming_api_call = request
        base.run_conversation = lambda inner, *args, **kwargs: inner._interruptible_streaming_api_call({'private': 'request'})
        with self.assertRaisesRegex(BridgeError, '^provider_failed$'):
            execute_candidate(self.spec, self.journal, base, 'openai', 'selected-private-token')
        self.assertEqual(self.private()['stage'], 'provider_request')
        self.assertEqual(self.private()['kind'], 'timeout')

    def test_disabled_credential_pool_preserves_pinned_tuple_and_retry_bit(self):
        base = self.agent(lambda: None)
        def forbidden(*args, **kwargs):
            self.fail('ambient recovery implementation entered')
        for name in ('_try_refresh_codex_client_credentials', '_try_refresh_env_client_credentials',
                     '_recover_with_credential_pool', '_try_activate_fallback', 'switch_model', '_swap_credential'):
            setattr(base, name, forbidden)
        agent = guarded_agent_class(base, self.journal, self.key, ('openai-codex', 'gpt-6-astra', 'max'))()
        for bit in (False, True):
            recovered, retry_bit = agent._recover_with_credential_pool(
                status_code=401, has_retried_429=bit, error_context={'secret': 'never retain'})
            self.assertIs(recovered, False)
            self.assertIs(retry_bit, bit)
        self.assertFalse(agent._try_refresh_codex_client_credentials())
        self.assertFalse(agent._try_refresh_env_client_credentials())
        self.assertFalse(agent._try_activate_fallback())

    def test_original_request_error_survives_upstream_masking_without_raw_context(self):
        class Sensitive(TimeoutError):
            def __str__(self):
                raise AssertionError('exception must never be stringified')
        base = self.agent(lambda: None)
        def request(inner, *args):
            raise Sensitive('private provider response')
        def conversation(inner, *args, **kwargs):
            try:
                inner._interruptible_api_call({'private': 'provider request'})
            except Sensitive:
                raise TypeError('private outer wrapper') from None
        base._interruptible_api_call, base.run_conversation = request, conversation
        with self.assertRaisesRegex(BridgeError, '^provider_failed$'):
            execute_candidate(self.spec, self.journal, base, 'openai', 'selected-private-token')
        value = self.private()
        self.assertEqual(value['kind'], 'type')
        self.assertEqual(value['provider_failure']['kind'], 'timeout')
        self.assertEqual(value['provider_failure']['stage'], 'provider_request')
        for text in ('private provider', 'private outer', 'selected-private-token', 'Sensitive'):
            self.assertNotIn(text, json.dumps(value))
        self.assertNotIn('provider_failure', json.dumps(self.journal.events(self.key)))

    def test_first_provider_classification_is_closed_and_not_replaced_by_later_retry(self):
        class Reason(Enum):
            auth = 'auth'
            secret = 'private provider text'
        stage = FailureStage()
        stage.at('provider_request')
        stage.provider_error(TimeoutError('private timeout'))
        stage.provider_classification(401, Reason.auth)
        stage.provider_error(TypeError('later error'))
        stage.provider_classification(429, Reason.secret)
        value = stage.capture(RuntimeError('outer error'))
        original = value['provider_failure']
        self.assertEqual(original['kind'], 'timeout')
        self.assertEqual(original['http_status'], 401)
        self.assertEqual(original['reason'], 'auth')
        self.assertTrue(validate_diagnostic(value))
        for status, reason in ((True, Reason.secret), (999, 'auth'), ('401', ['secret'])):
            stage = FailureStage()
            stage.at('provider_request')
            stage.provider_error(ValueError('private'))
            stage.provider_classification(status, reason)
            original = stage.capture(ValueError())['provider_failure']
            self.assertIsNone(original['http_status'])
            self.assertIsNone(original['reason'])

    def test_original_diagnostic_envelope_is_bounded_and_revalidated_at_persistence(self):
        stage = FailureStage()
        stage.at('provider_request')
        stage.provider_error(TimeoutError())
        value = stage.capture(TypeError())
        longest = max(HERMES_FILES, key=len)
        frame = {'source': 'hermes', 'file': longest, 'line': 100_000}
        value['frames'] = [frame] * 8
        value['provider_failure']['frames'] = [frame] * 4
        self.assertLessEqual(len(json.dumps(value)), 2048)
        self.assertTrue(validate_diagnostic(value))
        for invalid in ({'http_status': True}, {'http_status': 999}, {'reason': 'private'},
                        {'error_context': 'private'}, {'frames': [frame] * 5},
                        {'provider_failure': {}}, {'stage': 'load_runtime'}):
            bad = copy.deepcopy(value)
            bad['provider_failure'].update(invalid)
            with self.subTest(invalid=invalid), self.assertRaisesRegex(BridgeError, '^invalid_failure_diagnostic$'):
                self.journal.fail(self.key, diagnostic=bad)

    def test_diagnostic_failure_rolls_back_the_failure_event_and_state(self):
        self.journal.begin_execution(self.key)
        with self.journal.connection() as db:
            db.execute("CREATE TRIGGER fixture_fail BEFORE INSERT ON private_failure_diagnostics BEGIN SELECT RAISE(ABORT, 'fixture storage failure'); END")
        diagnostic = {'stage': 'conversation_run', 'kind': 'runtime', 'boundary': None, 'frames': []}
        with self.assertRaises(sqlite3.IntegrityError):
            self.journal.fail(self.key, 'provider_failed', diagnostic=diagnostic)
        self.assertEqual(self.journal.lookup(self.key)['status'], 'running')
        self.assertEqual(len(self.journal.events(self.key)['events']), 1)
        self.assertIsNone(self.private())
        with self.journal.connection() as db:
            db.execute('DROP TRIGGER fixture_fail')
        self.journal.fail(self.key, 'provider_failed', diagnostic=diagnostic)
        self.assertEqual(self.private(), diagnostic)
        self.assertEqual(self.journal.lookup(self.key)['status'], 'failed')

    def test_persistence_refuses_arbitrary_diagnostic_text_or_oversized_coordinates(self):
        self.journal.begin_execution(self.key)
        base = {'stage': 'conversation_run', 'kind': 'runtime', 'boundary': None, 'frames': []}
        for patch in ({'stage': 'private-stage'}, {'stage': []}, {'kind': 'private-error'},
                      {'boundary': 'private-secret'}, {'boundary': []}, {'locals': 'secret'},
                      {'frames': [{'source': 'hermes', 'file': 'private-secret.py', 'line': 4}]},
                      {'frames': [{'source': 'hermes', 'file': [], 'line': 4}]},
                      {'frames': [{'source': 'hermes', 'file': 'run_agent.py', 'line': True}]},
                      {'frames': [{'source': 'hermes', 'file': 'run_agent.py', 'line': 4}] * 9}):
            with self.subTest(patch=patch), self.assertRaisesRegex(BridgeError, '^invalid_failure_diagnostic$'):
                self.journal.fail(self.key, 'provider_failed', diagnostic={**copy.deepcopy(base), **patch})
        self.assertEqual(self.journal.lookup(self.key)['status'], 'running')
        self.assertIsNone(self.private())

    def test_cancellation_wins_over_late_private_failure(self):
        self.journal.begin_execution(self.key)
        self.journal.request_cancel(self.key)
        self.journal.fail(self.key, 'provider_failed', diagnostic={
            'stage': 'provider_return', 'kind': 'runtime', 'boundary': None, 'frames': []})
        self.assertEqual(self.journal.lookup(self.key)['status'], 'cancelling')
        self.assertIsNone(self.private())

    def test_traceback_file_allowlist_is_exactly_the_pinned_python_seams(self):
        lock = json.loads((Path(__file__).parents[1] / 'hermes-source-lock.json').read_text())
        self.assertEqual(HERMES_FILES, {name for name in lock['source_files'] if name.endswith('.py')})

    def test_httpx_failures_have_closed_distinct_coordinates_without_exception_text(self):
        for name, expected in (('ReadTimeout', 'http_read_timeout'), ('RemoteProtocolError', 'http_protocol'),
                               ('ConnectTimeout', 'http_connect_timeout'), ('PrivateError', 'other')):
            # Exact SDK module/class metadata; this test needs no optional SDK installed.
            error_type = type(name, (Exception,), {'__module__': 'httpx'})
            value = FailureStage().capture(error_type('private server payload'))
            self.assertEqual(value['kind'], expected)
            self.assertNotIn('private', json.dumps(value))
            self.assertTrue(validate_diagnostic(value))

    def test_sdk_base_api_error_has_closed_kind_for_sse_error_events(self):
        error_type = type('APIError', (Exception,), {'__module__': 'openai'})
        value = FailureStage().capture(error_type('private stream timeout body'))
        self.assertEqual(value['kind'], 'provider_api')
        self.assertNotIn('private', json.dumps(value))
        self.assertTrue(validate_diagnostic(value))
