"""Cross-language vector and actual controller/candidate history regressions."""
import copy
import json
import tempfile
import unittest
from pathlib import Path

from ortak_hermes_bridge.conversation_context import validate
from ortak_hermes_bridge.hermes_candidate import execute_candidate
from ortak_hermes_bridge.journal import BridgeError, Journal
from ortak_hermes_bridge.service import Bridge, EMPTY_POLICY


class ConversationContext(unittest.TestCase):
    def setUp(self):
        path = Path(__file__).resolve().parents[3] / 'crates/ortak-control/src/conversation_context/test_vector.json'
        self.context = json.loads(path.read_text())
        self.company = '55555555-5555-4555-8555-555555555555'
        self.binding = {'adapter': 'hermes', 'profile_ref': 'fixture', 'model': 'fixture',
                        'workspace_ref': 'fixture', 'credential_refs': [], 'options': {}}
        self.spec = {'run_id': self.context['snapshot_id'], 'employee_id': 'bora',
                     'revision_id': self.context['employee']['revision_id'], 'binding': self.binding,
                     'permissions': copy.deepcopy(EMPTY_POLICY), 'input': 'Bunu İngilizceye çevirir misin Bora?',
                     'context': {'conversation_ref': self.context['channel_id'],
                                 'reply_to_message_id': self.context['trigger_message_id'], 'work_item_id': None,
                                 'memory_context': [], 'conversation_context': self.context},
                     'idempotency_key': f"ortak-run:{self.company}:{self.context['snapshot_id']}"}
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.journal = Journal(Path(temporary.name) / 'journal.sqlite')
        self.bridge = Bridge(self.journal, self.company, [{'employee_id': 'bora', 'binding': self.binding}])

    def test_production_candidate_passes_attributed_history_and_keeps_current_request(self):
        self.bridge.validate({'company_id': self.company, 'spec': self.spec})
        calls = []
        class Agent:
            tools = []
            def __init__(inner, **kwargs):
                pass
            def _get_transport(inner):
                return None
            def run_conversation(inner, request, **kwargs):
                calls.append((request, kwargs))
                return {'completed': True, 'final_response': '1. We can clarify the product goal together.'}
        for name in ('_invoke_tool', '_execute_tool_calls', '_execute_tool_calls_sequential',
                     '_execute_tool_calls_concurrent', '_dispatch_delegate_task'):
            setattr(Agent, name, lambda *args: None)
        self.journal.reserve(self.spec)
        for _ in range(2):
            execute_candidate(self.spec, self.journal, Agent, 'openai', 'fixture-only')
        self.assertEqual(len(calls), 1)
        request, kwargs = calls[0]
        self.assertEqual(request, self.spec['input'])
        history = kwargs['conversation_history']
        self.assertEqual([m['role'] for m in history], ['user'])
        self.assertIn(self.context['messages'][1]['content'].splitlines()[0], history[0]['content'])
        self.assertIn('"author_employee_id":"ada"', history[0]['content'])
        self.assertIn('"employee_id":"bora"', history[0]['content'])
        self.assertNotIn(request, history[0]['content'])
        self.assertIn('another employee\'s answer is not your own', kwargs['system_message'])
        self.assertEqual(self.journal.lookup(self.spec['idempotency_key'])['status'], 'completed')

    def test_controller_rejects_mixed_identity_unknown_fields_and_scope(self):
        cases = [lambda c: c.update(snapshot_id=self.company),
                 lambda c: c.update(channel_id=self.company),
                 lambda c: c['employee'].update(employee_id='ada'),
                 lambda c: c['messages'][0].update(role='system'),
                 lambda c: c['messages'][0].update(message_id=c['trigger_message_id']),
                 lambda c: c['messages'].append(copy.deepcopy(c['messages'][0])),
                 lambda c: c['messages'].reverse(),
                 lambda c: c.update(thread_root_message_id='d' * 64),
                 lambda c: c.update(version=True),
                 lambda c: c['messages'][0].update(content='ü' * 4097),
                 lambda c: c['messages'][0].update(content='text\0hidden'),
                 lambda c: c['messages'][0].update(created_at='yesterday')]
        for mutate in cases:
            spec = copy.deepcopy(self.spec)
            mutate(spec['context']['conversation_context'])
            with self.subTest(mutate=mutate), self.assertRaises(BridgeError):
                self.bridge.validate({'company_id': self.company, 'spec': spec})

    def test_installed_fixture_matches_the_rust_wire_vector(self):
        path = Path(__file__).resolve().parents[1] / 'checks/conversation-context-v1.json'
        self.assertEqual(json.loads(path.read_text()), self.context)

    def test_missing_history_is_backward_compatible(self):
        del self.spec['context']['conversation_context']
        self.assertIsNone(validate(self.spec))
        self.bridge.validate({'company_id': self.company, 'spec': self.spec})
