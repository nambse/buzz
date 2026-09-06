"""Focused production-seam regressions; no Hermes, credential or network access."""
import copy
import json
import sqlite3
import tempfile
import unittest
from types import SimpleNamespace
from concurrent.futures import ThreadPoolExecutor
from contextlib import nullcontext
from pathlib import Path
from uuid import uuid4

from ortak_hermes_bridge.journal import BridgeError, Journal, identity, reference
from ortak_hermes_bridge.service import Bridge, EMPTY_POLICY
from ortak_hermes_bridge.hermes_candidate import execute_candidate, guarded_agent_class, ToolDenied, CredentialDenied, agent_constructor_kwargs

class Fixture(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.path = Path(self.temp.name) / 'journal.sqlite'
        self.journal = Journal(self.path)
        self.company = str(uuid4())
        self.run = str(uuid4())
        self.key = f'ortak-run:{self.company}:{self.run}'
        self.binding = {'adapter': 'hermes', 'profile_ref': 'disposable-profile', 'model': 'test-model',
                        'workspace_ref': 'no-tools-workspace', 'credential_refs': [], 'options': {}}
        self.spec = {'run_id': self.run, 'employee_id': 'disposable', 'revision_id': str(uuid4()),
                     'binding': self.binding, 'permissions': copy.deepcopy(EMPTY_POLICY), 'input': 'hello',
                     'context': {'conversation_ref': None, 'reply_to_message_id': None,
                                 'work_item_id': None, 'memory_context': []}, 'idempotency_key': self.key}
        self.profiles = [{'employee_id': 'disposable', 'binding': self.binding}]
        self.body = {'company_id': self.company, 'spec': self.spec}
        self.control = {'company_id': self.company, 'run_id': self.run, 'idempotency_key': self.key}

    def test_stable_start_and_read_only_lookup_after_restart(self):
        self.assertIsNone(self.journal.lookup(self.key))
        receipt, fresh = self.journal.reserve(self.spec)
        self.assertTrue(fresh)
        other = Journal(self.path)
        self.assertEqual(other.lookup(self.key), receipt)
        self.assertEqual(other.reserve(self.spec), (receipt, False))
        self.assertEqual(other.events(self.key)['events'], [])

    def test_concurrent_reserve_invokes_only_one_winner(self):
        with ThreadPoolExecutor(max_workers=8) as workers:
            values = list(workers.map(lambda _: self.journal.reserve(self.spec), range(16)))
        self.assertEqual(sum(fresh for _, fresh in values), 1)
        self.assertEqual(len({r['runtime_run_ref'] for r, _ in values}), 1)

    def test_changed_pinned_policy_or_input_conflicts(self):
        self.journal.reserve(self.spec)
        for field, value in [('input', 'different'), ('revision_id', str(uuid4())), ('permissions', {'allowed_tools': ['terminal']})]:
            changed = {**self.spec, field: value}
            with self.subTest(field=field), self.assertRaisesRegex(BridgeError, 'start_conflict'):
                self.journal.reserve(changed)

    def test_cancel_before_start_survives_restart_and_blocks_execution(self):
        self.assertEqual(self.journal.request_cancel(self.key), 'cancelled')
        self.journal.finish_cancel(self.key)
        other = Journal(self.path)
        receipt, fresh = other.reserve(self.spec)
        self.assertFalse(fresh)
        self.assertEqual(receipt['status'], 'cancelled')
        self.assertFalse(other.begin_execution(self.key))
        self.assertEqual(other.request_cancel(self.key), 'already_terminal')

    def test_cancel_between_reservation_and_provider_boundary(self):
        self.journal.reserve(self.spec)
        self.journal.request_cancel(self.key)
        self.assertFalse(self.journal.begin_execution(self.key))
        self.assertFalse(self.journal.complete(self.key, 'late result'))
        self.journal.finish_cancel(self.key)
        events = self.journal.events(self.key)
        self.assertEqual([e['payload']['event_type'] for e in events['events']], ['run.cancelled'])

    def test_cancel_after_execution_blocks_late_completion(self):
        self.journal.reserve(self.spec)
        self.assertTrue(self.journal.begin_execution(self.key))
        self.journal.request_cancel(self.key)
        self.assertFalse(self.journal.complete(self.key, 'late result'))
        self.journal.finish_cancel(self.key)
        self.assertEqual(self.journal.lookup(self.key)['status'], 'cancelled')

    def test_replay_dense_exclusive_and_terminal_only_final_page(self):
        self.journal.reserve(self.spec)
        self.journal.begin_execution(self.key)
        self.journal.complete(self.key, 'reply')
        page = self.journal.events(self.key, 0, 2)
        self.assertEqual([e['cursor'] for e in page['events']], ['1', '2'])
        self.assertFalse(page['terminal'])
        self.assertEqual(page, self.journal.events(self.key, 0, 2))
        page2 = Journal(self.path).events(self.key, 2, 2)
        self.assertEqual([e['cursor'] for e in page2['events']], ['3', '4'])
        self.assertTrue(page2['terminal'])
        self.assertEqual(self.journal.events(self.key, 4), {'events': [], 'terminal': True})
        for after, limit in [(5, 10), (-1, 10), (True, 10), (0, 101)]:
            with self.subTest(after=after, limit=limit), self.assertRaises(BridgeError):
                self.journal.events(self.key, after, limit)

    def test_atomic_terminal_rolls_back_output_when_commit_fails(self):
        self.journal.reserve(self.spec)
        self.journal.begin_execution(self.key)
        with self.journal.connection() as db:
            db.execute("CREATE TRIGGER refuse_terminal BEFORE UPDATE OF status ON runs WHEN NEW.status='completed' BEGIN SELECT RAISE(ABORT,'test failure'); END")
        with self.assertRaises(sqlite3.IntegrityError):
            self.journal.complete(self.key, 'must roll back')
        self.assertEqual(len(self.journal.events(self.key)['events']), 1)
        self.assertEqual(self.journal.lookup(self.key)['status'], 'running')

    def test_recovery_requires_stopped_owner_and_never_reruns(self):
        self.journal.reserve(self.spec)
        self.journal.begin_execution(self.key)
        other = Journal(self.path)
        with self.assertRaisesRegex(BridgeError, 'execution_owner_not_stopped'):
            other.recover(lambda _: False)
        self.assertEqual(other.lookup(self.key)['status'], 'running')
        checked = []
        other.recover(lambda key: checked.append(key) or True)
        self.assertEqual(checked, [self.key])
        self.assertEqual(other.lookup(self.key)['status'], 'failed')
        self.assertFalse(other.reserve(self.spec)[1])
        self.assertFalse(other.begin_execution(self.key))

    def test_terminal_text_redacts_before_persistence(self):
        self.journal.reserve(self.spec)
        self.journal.begin_execution(self.key)
        secret = 'fixture-private-value'
        self.journal.complete(self.key, f'api_key="abc def" Bearer xyz {secret} sk-fixture123456', (secret,))
        rendered = json.dumps(self.journal.events(self.key))
        for value in ('abc def', 'xyz', secret, 'sk-fixture123456'):
            self.assertNotIn(value, rendered)
        self.assertIn('[redacted]', rendered)

    def test_disabled_executor_refuses_without_reserving_and_honest_health(self):
        bridge = Bridge(self.journal, self.company, self.profiles)
        with self.assertRaisesRegex(BridgeError, 'executor_unavailable'):
            bridge.dispatch('POST', '/v1/runs', self.body)
        self.assertIsNone(self.journal.lookup(self.key))
        self.assertNotIn('run_start', bridge.dispatch('GET', '/v1/capabilities')['capabilities'])
        self.assertFalse(bridge.dispatch('POST', '/v1/profiles/inspect', {'company_id': self.company, 'binding': self.binding})['healthy'])

    def test_service_cross_company_and_profile_refusal(self):
        bridge = Bridge(self.journal, self.company, self.profiles)
        with self.assertRaisesRegex(BridgeError, 'run_not_found'):
            bridge.dispatch('POST', '/v1/runs/lookup', {**self.control, 'company_id': str(uuid4())})
        changed = copy.deepcopy(self.body)
        changed['spec']['binding']['profile_ref'] = '/old/cem'
        with self.assertRaisesRegex(BridgeError, 'profile_not_found'):
            bridge.dispatch('POST', '/v1/runs', changed)

    def test_unsupported_policy_fails_before_execution_or_reservation(self):
        bridge = Bridge(self.journal, self.company, self.profiles)
        for name in EMPTY_POLICY:
            changed = copy.deepcopy(self.body)
            changed['spec']['permissions'][name] = ['terminal']
            with self.subTest(name=name), self.assertRaisesRegex(BridgeError, 'unsupported_permission_policy'):
                bridge.dispatch('POST', '/v1/runs', changed)
        self.assertIsNone(self.journal.lookup(self.key))

    def test_service_tombstone_and_lookup_without_executor(self):
        bridge = Bridge(self.journal, self.company, self.profiles)
        receipt = bridge.dispatch('POST', '/v1/runs/cancel', {**self.control, 'reason': 'revoked'})
        self.assertEqual(receipt, {'runtime_run_ref': reference(self.key), 'outcome': 'cancelled'})
        self.assertEqual(bridge.dispatch('POST', '/v1/runs', self.body)['status'], 'cancelled')
        self.assertEqual(bridge.dispatch('POST', '/v1/runs/lookup', self.control)['status'], 'cancelled')

    def test_no_terminal_cancel_ack_until_execution_stopped(self):
        class Executor:
            available = True
            stopped = False
            def start(inner, spec, journal):
                journal.begin_execution(spec['idempotency_key'])
            def stop(inner, key):
                return inner.stopped
        executor = Executor()
        bridge = Bridge(self.journal, self.company, self.profiles, executor)
        bridge.dispatch('POST', '/v1/runs', self.body)
        with self.assertRaisesRegex(BridgeError, 'execution_not_stopped'):
            bridge.dispatch('POST', '/v1/runs/cancel', {**self.control, 'reason': 'stop'})
        self.assertEqual(self.journal.lookup(self.key)['status'], 'cancelling')
        self.assertFalse(self.journal.events(self.key)['terminal'])
        executor.stopped = True
        self.assertEqual(bridge.dispatch('POST', '/v1/runs/cancel', {**self.control, 'reason': 'stop'})['outcome'], 'cancelled')

    def test_candidate_tools_deny_at_each_execution_entry(self):
        boundaries = ('_invoke_tool', '_execute_tool_calls', '_execute_tool_calls_sequential', '_execute_tool_calls_concurrent', '_dispatch_delegate_task')
        def unsafe_method(*args, **kwargs):
            self.fail('underlying tool must never execute')
        base = type('Upstream', (), {**{name: unsafe_method for name in boundaries}, '_get_transport': lambda self: None})
        self.journal.reserve(self.spec)
        self.journal.begin_execution(self.key)
        guarded = guarded_agent_class(base, self.journal, self.key)()
        for name in boundaries:
            with self.subTest(name=name), self.assertRaises(ToolDenied):
                getattr(guarded, name)('terminal', {'command': 'should not execute'})
        self.assertEqual(self.journal.lookup(self.key)['status'], 'failed')
        self.assertNotIn('should not execute', json.dumps(self.journal.events(self.key)))

    def test_candidate_invokes_provider_once_and_commits_at_execution_return(self):
        calls = []
        class Agent:
            tools = []
            def _get_transport(inner):
                return None
            def __init__(inner, **kwargs):
                self.assertTrue(kwargs['skip_memory'])
                self.assertTrue(kwargs['skip_context_files'])
                self.assertEqual(kwargs['enabled_toolsets'], [])
                self.assertEqual(kwargs['api_key'], 'selected-fixture-key')
                self.assertEqual(kwargs['base_url'], 'https://api.openai.com/v1')
            def run_conversation(inner, text, **kwargs):
                calls.append(text)
                self.assertEqual(self.journal.lookup(self.key)['status'], 'running')
                return {'completed': True, 'final_response': 'answer'}
        for name in ('_invoke_tool', '_execute_tool_calls', '_execute_tool_calls_sequential', '_execute_tool_calls_concurrent', '_dispatch_delegate_task'):
            setattr(Agent, name, lambda *args: None)
        self.journal.reserve(self.spec)
        execute_candidate(self.spec, self.journal, Agent, 'openai', 'selected-fixture-key')
        execute_candidate(self.spec, self.journal, Agent, 'openai', 'selected-fixture-key')
        self.assertEqual(calls, ['hello'])
        self.assertEqual(self.journal.lookup(self.key)['status'], 'completed')

    def test_work_candidate_retains_complete_text_but_never_requests_office_publication(self):
        self.spec['context']['work_item_id'] = str(uuid4())
        Bridge(self.journal, self.company, self.profiles).validate(self.body)
        calls = []
        class Agent:
            tools = []
            def _get_transport(inner):
                return None
            def __init__(inner, **kwargs):
                pass
            def run_conversation(inner, text, **kwargs):
                calls.append(kwargs['system_message'])
                return {'completed': True, 'final_response': 'Verifiable work deliverable'}
        for name in ('_invoke_tool', '_execute_tool_calls', '_execute_tool_calls_sequential', '_execute_tool_calls_concurrent', '_dispatch_delegate_task'):
            setattr(Agent, name, lambda *args: None)
        self.journal.reserve(self.spec)
        execute_candidate(self.spec, self.journal, Agent, 'openai', 'selected-fixture-key')
        execute_candidate(self.spec, self.journal, Agent, 'openai', 'selected-fixture-key')
        payloads = [event['payload'] for event in Journal(self.path).events(self.key)['events']]
        self.assertEqual(len(calls), 1)
        self.assertIn('human review', calls[0])
        self.assertEqual(payloads[1]['delta']['text'], 'Verifiable work deliverable')
        self.assertEqual(payloads[2]['intent'], 'silent')
        self.assertEqual(payloads[3]['delivery_intent'], 'silent')
        self.assertEqual(self.journal.lookup(self.key)['status'], 'completed')

    def test_candidate_records_only_closed_failure_classes_without_partial_output(self):
        cases = [
            ({'completed': False, 'final_response': 'sensitive unfinished text'}, 'provider_incomplete'),
            (['sensitive malformed response'], 'provider_response_invalid'),
            ({'completed': True, 'final_response': 42}, 'provider_response_invalid'),
            ({'completed': True, 'final_response': 'sensitive oversized text' * 1000}, 'invalid_output'),
            (CredentialDenied('sensitive credential detail'), 'credential_denied'),
            (RuntimeError('sensitive provider exception'), 'provider_failed'),
        ]
        for result, code in cases:
            with self.subTest(code=code):
                spec = copy.deepcopy(self.spec)
                spec['run_id'] = str(uuid4())
                spec['idempotency_key'] = f"ortak-run:{self.company}:{spec['run_id']}"
                key = spec['idempotency_key']
                class Agent:
                    tools = []
                    def _get_transport(inner):
                        return None
                    def __init__(inner, **kwargs):
                        pass
                    def run_conversation(inner, *args, **kwargs):
                        if isinstance(result, BaseException):
                            raise result
                        return result
                for name in ('_invoke_tool', '_execute_tool_calls', '_execute_tool_calls_sequential', '_execute_tool_calls_concurrent', '_dispatch_delegate_task'):
                    setattr(Agent, name, lambda *args: None)
                self.journal.reserve(spec)
                with (nullcontext() if code == 'credential_denied' else self.assertRaisesRegex(BridgeError, f'^{code}$')):
                    execute_candidate(spec, self.journal, Agent, 'openai', 'selected-fixture-key')
                events = self.journal.events(key)['events']
                self.assertEqual([e['payload']['event_type'] for e in events], ['run.started', 'run.failed'])
                self.assertEqual(events[-1]['payload']['code'], code)
                self.assertNotIn('sensitive', json.dumps(events))
                self.assertNotIn('selected-fixture-key', json.dumps(events))

    def test_work_context_cannot_also_supply_an_office_destination(self):
        bridge = Bridge(self.journal, self.company, self.profiles)
        for context in (
            {'work_item_id': 'not-a-work-id'},
            {'work_item_id': '00000000-0000-0000-0000-000000000000'},
            {'work_item_id': str(uuid4()), 'conversation_ref': str(uuid4())},
            {'work_item_id': str(uuid4()), 'reply_to_message_id': 'a' * 64},
        ):
            with self.subTest(context=context), self.assertRaisesRegex(BridgeError, 'invalid_context'):
                body = copy.deepcopy(self.body)
                body['spec']['context'].update(context)
                bridge.validate(body)
        self.assertIsNone(self.journal.lookup(self.key))

    def test_response_tool_intent_denies_before_validation_normalization_or_retry(self):
        boundaries = ('_invoke_tool', '_execute_tool_calls', '_execute_tool_calls_sequential',
                      '_execute_tool_calls_concurrent', '_dispatch_delegate_task')
        cases = [({'output': [{'type': kind}]}, False) for kind in ('function_call', 'custom_tool_call', 'web_search_call')]
        cases += [({'choices': [{'message': {'tool_calls': [{'name': 'terminal'}]}}]}, False),
                  ({'choices': [{'message': {'function_call': {'name': 'terminal'}}}]}, False),
                  ({'output': [{'type': 'message'}]}, True)]
        for response, normalized_tool in cases:
            with self.subTest(response=response):
                run = str(uuid4())
                key = f'ortak-run:{self.company}:{run}'
                self.journal.reserve({**self.spec, 'run_id': run, 'idempotency_key': key})
                self.journal.begin_execution(key)
                def validate(value):
                    self.fail('tool-bearing raw response reached upstream validation')
                def normalize(value, **kwargs):
                    if not normalized_tool:
                        self.fail('tool-bearing raw response reached upstream normalization')
                    return SimpleNamespace(tool_calls=['forged'])
                transport = SimpleNamespace(validate_response=validate, normalize_response=normalize)
                base = type('Upstream', (), {**{name: lambda *args: None for name in boundaries},
                                             '_get_transport': lambda self: transport})
                guarded = guarded_agent_class(base, self.journal, key)()
                with self.assertRaises(ToolDenied):
                    if normalized_tool:
                        guarded._get_transport().normalize_response(response)
                    else:
                        guarded._get_transport().validate_response(response)
                self.assertEqual(self.journal.lookup(key)['status'], 'failed')
                self.assertIn('policy_denied', json.dumps(self.journal.events(key)))

    def test_provider_route_is_explicit_and_oauth_is_not_an_api_key_alias(self):
        for provider, endpoint in (('openai', 'https://api.openai.com/v1'),
                                   ('openrouter', 'https://openrouter.ai/api/v1')):
            kwargs = agent_constructor_kwargs(self.spec, provider, 'selected-fixture-key')
            self.assertEqual(kwargs['api_key'], 'selected-fixture-key')
            self.assertEqual(kwargs['base_url'], endpoint)
        for provider, token in (('openai-codex', 'fixture'), ('custom', 'fixture'),
                                ('openai', None), ('openai', ''), ('openai', 'a b')):
            with self.subTest(provider=provider, token=token), self.assertRaises(BridgeError):
                agent_constructor_kwargs(self.spec, provider, token)

    def test_invalid_key_is_not_a_path_or_alias(self):
        for key in ('../profile', self.key.upper(), reference(self.key), self.key + ':extra'):
            with self.subTest(key=key), self.assertRaises(BridgeError):
                identity(key)

if __name__ == '__main__':
    unittest.main()
