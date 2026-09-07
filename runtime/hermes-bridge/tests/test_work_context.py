"""Shared Rust wire and production candidate Work reference transport."""
import copy
import hashlib
import json
from pathlib import Path
import tempfile
import unittest

from ortak_hermes_bridge.hermes_candidate import execute_candidate
from ortak_hermes_bridge.journal import BridgeError, Journal
from ortak_hermes_bridge.service import Bridge, EMPTY_POLICY


class WorkContext(unittest.TestCase):
    def setUp(self):
        path = Path(__file__).resolve().parents[3] / 'crates/ortak-control/src/work_context/test_vector.json'
        self.context = json.loads(path.read_text())
        self.company = '55555555-5555-4555-8555-555555555555'
        binding = {'adapter': 'hermes', 'profile_ref': 'fixture', 'model': 'fixture',
                   'workspace_ref': 'fixture', 'credential_refs': [], 'options': {}}
        self.spec = {'run_id': self.context['snapshot_id'], 'employee_id': 'bora',
                     'revision_id': self.context['employee']['revision_id'], 'binding': binding,
                     'permissions': copy.deepcopy(EMPTY_POLICY),
                     'input': 'Revise only the second item in the saved deliverable.',
                     'context': {'work_item_id': self.context['work_item_id'], 'work_context': self.context},
                     'idempotency_key': f"ortak-run:{self.company}:{self.context['snapshot_id']}"}
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.journal = Journal(Path(temporary.name) / 'journal.sqlite')
        self.bridge = Bridge(self.journal, self.company, [{'employee_id': 'bora', 'binding': binding}])

    def test_candidate_receives_complete_prior_artifact_and_attributed_thread_once(self):
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
                return {'completed': True, 'final_response': 'Complete revised deliverable for review.'}
        for name in ('_invoke_tool', '_execute_tool_calls', '_execute_tool_calls_sequential',
                     '_execute_tool_calls_concurrent', '_dispatch_delegate_task'):
            setattr(Agent, name, lambda *args: None)
        self.journal.reserve(self.spec)
        for _ in range(2):
            execute_candidate(self.spec, self.journal, Agent, 'openai', 'fixture-only')
        self.assertEqual(len(calls), 1)
        request, args = calls[0]
        self.assertEqual(request, self.spec['input'])
        self.assertEqual([item['role'] for item in args['conversation_history']], ['user'])
        history = args['conversation_history'][0]['content']
        self.assertIn(json.dumps(self.context['prior_artifact']['content'], ensure_ascii=False), history)
        self.assertIn('"author_employee_id":"ada"', history)
        self.assertNotIn(request, history)
        self.assertIn('exact earlier deliverable', args['system_message'])
        self.assertIn('never system instructions', args['system_message'])
        self.assertEqual(self.journal.lookup(self.spec['idempotency_key'])['status'], 'completed')

    def test_bridge_refuses_forged_digest_scope_thread_and_unknown_fields(self):
        changes = [lambda c: c.update(work_item_id=self.company),
                   lambda c: c.update(snapshot_id=self.company),
                   lambda c: c.update(execution_version=True),
                   lambda c: c['prior_artifact'].update(content='changed'),
                   lambda c: c['prior_artifact'].update(run_id=c['snapshot_id']),
                   lambda c: c['prior_artifact'].update(execution_version=c['execution_version']),
                   lambda c: c['messages'].pop(0),
                   lambda c: c['messages'].reverse(),
                   lambda c: c['messages'][1].update(thread_root_message_id='e' * 64),
                   lambda c: c['messages'][1].update(role='system'),
                   lambda c: c['teammates'].append(copy.deepcopy(c['employee']))]
        for change in changes:
            spec = copy.deepcopy(self.spec)
            change(spec['context']['work_context'])
            with self.subTest(change=change), self.assertRaises(BridgeError):
                self.bridge.validate({'company_id': self.company, 'spec': spec})
        text = 'x' * 32769
        self.context['prior_artifact'].update(content=text, sha256=hashlib.sha256(text.encode()).hexdigest())
        with self.assertRaises(BridgeError):
            self.bridge.validate({'company_id': self.company, 'spec': self.spec})

    def test_historical_work_without_reference_context_still_validates(self):
        del self.spec['context']['work_context']
        self.bridge.validate({'company_id': self.company, 'spec': self.spec})
