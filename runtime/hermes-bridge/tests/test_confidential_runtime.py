"""Real AEAD/request/journal/executor seam; synthetic provider, no credentials."""
import base64
import copy
import json
import os
from pathlib import Path
import sqlite3
import sys
import tempfile
import threading
import http.client
from http.server import HTTPServer
import unittest
from unittest.mock import patch
from uuid import uuid4

from ortak_hermes_bridge.confidential import codec, memfd, wire
from ortak_hermes_bridge.confidential.request import ConfidentialRequest, canonical_inner
from ortak_hermes_bridge.confidential.journal import ExecutionJournal, reserve, events
from ortak_hermes_bridge.hermes_candidate import execute_candidate
from ortak_hermes_bridge.journal import BridgeError, Journal, reference
from ortak_hermes_bridge.service import Bridge, EMPTY_POLICY

VECTOR = Path(__file__).resolve().parents[3] / 'crates/ortak-control/src/confidential/vector.json'


class ConfidentialRuntime(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.path = Path(self.temp.name) / 'journal.sqlite'
        self.journal = Journal(self.path)
        claims = json.loads(VECTOR.read_bytes())['identity']
        for key in ('company_id', 'run_id', 'key_id', 'employee_revision_id'):
            claims[key] = str(uuid4())
        self.claims = claims
        self.identity = wire.ValidatedIdentity(wire._dump(claims, 2048))
        self.master = codec.MasterKey(os.urandom(32))
        self.addCleanup(self.master.close)
        self.key = f"ortak-run:{claims['company_id']}:{claims['run_id']}"
        self.canary = 'gizli-kumru-74293-private-runtime'
        self.binding = {'adapter': 'hermes', 'profile_ref': 'fixture', 'model': 'fixture',
                        'workspace_ref': 'none', 'credential_refs': [], 'options': {}}
        self.spec = {'run_id': claims['run_id'], 'revision_id': claims['employee_revision_id'],
                     'employee_id': claims['employee_id'], 'idempotency_key': self.key,
                     'input': self.canary, 'permissions': copy.deepcopy(EMPTY_POLICY),
                     'context': {'conversation_ref': claims['conversation_id'], 'reply_to_message_id': None},
                     'binding': self.binding}
        self.profiles = [{'employee_id': claims['employee_id'], 'binding': self.binding}]
        self.bridge = Bridge(self.journal, claims['company_id'], self.profiles)
        self.body = self.body_for(self.spec)

    def body_for(self, spec):
        inner = {'format': 'ortak-confidential-run/1', 'identity': self.claims, 'spec': spec}
        snapshot = codec.seal(self.master, self.identity, 'snapshot', 0, canonical_inner(inner, 48 * 1024))
        keys = {}
        for purpose in ('snapshot', 'runtime_event'):
            key = codec._derive(self.master, self.identity, purpose)
            keys[purpose] = base64.b64encode(key).decode('ascii')
            key[:] = bytes(len(key))
        return {'company_id': self.claims['company_id'], 'snapshot': json.loads(snapshot.canonical_bytes), 'keys': keys}

    def request(self):
        request = ConfidentialRequest(self.body, self.bridge)
        self.addCleanup(request.close)
        return request

    def test_real_candidate_protects_output_and_restart_replays_identical_envelopes(self):
        calls = []
        reply = self.canary + '-yanıt'
        class Agent:
            tools = []
            def __init__(self, **kwargs): pass
            def _get_transport(self): return None
            def run_conversation(self, text, **kwargs):
                calls.append(text)
                return {'completed': True, 'final_response': reply}
        for name in ('_invoke_tool', '_execute_tool_calls', '_execute_tool_calls_sequential',
                     '_execute_tool_calls_concurrent', '_dispatch_delegate_task'):
            setattr(Agent, name, lambda *args: None)
        request = self.request()
        reserve(self.journal, request)
        sink = ExecutionJournal(self.journal, request)
        execute_candidate(request.spec, sink, Agent, 'openai', 'synthetic-provider-token')
        execute_candidate(request.spec, sink, Agent, 'openai', 'synthetic-provider-token')
        self.assertEqual(calls, [self.canary])
        result = events(self.journal, self.key)
        self.assertTrue(result['terminal'])
        self.assertEqual([x['cursor'] for x in result['events']], ['1', '2', '3', '4'])
        envelope = wire.ConfidentialEnvelope.parse(wire._dump(result['events'][1]['envelope'], 96 * 1024))
        opened = codec.open_payload(self.master, self.identity, 'runtime_event', 2, envelope)
        self.addCleanup(opened.close)
        self.assertEqual(json.loads(bytes(opened.view()))['payload']['delta']['text'], reply)
        request.close()
        restored = Journal(self.path)
        self.assertEqual(events(restored, self.key), result)
        restarted = Bridge(restored, self.claims['company_id'], self.profiles)
        replay = restarted.dispatch('POST', '/v1/confidential/runs/replay',
                                    {key: self.body[key] for key in ('company_id', 'snapshot')})
        self.assertEqual(replay['status'], 'completed')
        with self.assertRaisesRegex(BridgeError, 'run_not_found'):
            restored.events(self.key)
        for path in self.path.parent.glob('journal.sqlite*'):
            raw = path.read_bytes()
            for value in (self.canary, 'synthetic-provider-token', *self.body['keys'].values()):
                self.assertNotIn(value.encode(), raw)

    def test_cancel_without_keys_refuses_late_text_and_recovers_closed_status(self):
        request = self.request()
        reserve(self.journal, request)
        sink = ExecutionJournal(self.journal, request)
        self.assertTrue(sink.begin_execution(self.key))
        self.journal.request_cancel(self.key)
        self.assertFalse(sink.complete(self.key, self.canary))
        request.close()
        other = Journal(self.path)
        with self.assertRaisesRegex(BridgeError, 'execution_owner_not_stopped'):
            other.recover(lambda _: False)
        other.recover(lambda _: True)
        result = events(other, self.key)
        self.assertEqual(result['failure']['code'], 'cancelled')
        self.assertTrue(result['terminal'])
        self.assertEqual(len(result['events']), 1)
        with other.connection() as db:
            self.assertEqual(db.execute('SELECT count(*) FROM events').fetchone()[0], 0)

    def test_retained_mode_nonces_and_failure_status_cannot_be_erased(self):
        request = self.request()
        reserve(self.journal, request)
        sink = ExecutionJournal(self.journal, request)
        sink.begin_execution(self.key)
        sink.fail(self.key, 'provider_failed')
        with self.journal.connection() as db:
            for table in ('confidential_runs', 'confidential_events', 'confidential_status'):
                for sql in (f'DELETE FROM {table}', f'UPDATE {table} SET start_key=start_key'):
                    with self.subTest(sql=sql), self.assertRaises(sqlite3.IntegrityError): db.execute(sql)
            with self.assertRaises(sqlite3.IntegrityError):
                db.execute('INSERT INTO events VALUES (?,?,?,?)', (self.key, 2, 'fixture', self.canary))
            for sql in ("UPDATE runs SET fingerprint='changed'", "UPDATE runs SET sequence=0",
                        "UPDATE runs SET status='accepted'", "UPDATE runs SET started_at='changed'"):
                with self.subTest(sql=sql), self.assertRaises(sqlite3.IntegrityError): db.execute(sql)
        with self.assertRaisesRegex(BridgeError, 'start_conflict'):
            self.journal.reserve(self.spec)

    def test_atomic_output_rollback_and_nonce_collision_preserve_original_envelope(self):
        request = self.request()
        reserve(self.journal, request)
        sink = ExecutionJournal(self.journal, request)
        sink.begin_execution(self.key)
        first = events(self.journal, self.key)
        nonce = base64.b64decode(first['events'][0]['envelope']['nonce'])
        with patch('ortak_hermes_bridge.confidential.runtime_keys.os.urandom', return_value=nonce):
            with self.assertRaises(sqlite3.IntegrityError): sink.complete(self.key, self.canary)
        self.assertEqual(events(self.journal, self.key), first)
        with self.journal.connection() as db:
            db.execute("CREATE TRIGGER refuse_completion BEFORE UPDATE OF status ON runs WHEN NEW.status='completed' BEGIN SELECT RAISE(ABORT,'fixture'); END")
        with self.assertRaises(sqlite3.IntegrityError): sink.complete(self.key, self.canary)
        self.assertEqual(events(self.journal, self.key), first)

    def test_authentication_context_and_tool_policy_fail_before_any_reservation(self):
        invalid = []
        changed = copy.deepcopy(self.body)
        changed['keys']['snapshot'] = base64.b64encode(os.urandom(32)).decode()
        invalid.append(changed)
        for context in ({'conversation_ref': str(uuid4())}, {**self.spec['context'], 'memory_context': []},
                        {**self.spec['context'], 'work_item_id': str(uuid4())}):
            invalid.append(self.body_for({**self.spec, 'context': context}))
        invalid.append(self.body_for({**self.spec, 'permissions': {**EMPTY_POLICY, 'allowed_tools': ['terminal']}}))
        invalid.append(self.body_for({**self.spec, 'revision_id': str(uuid4())}))
        invalid.append(self.body_for({**self.spec, 'input': 'x' * 8193}))
        for body in invalid:
            with self.assertRaises((wire.ConfidentialError, BridgeError)):
                ConfidentialRequest(body, self.bridge)
        self.assertIsNone(self.journal.lookup(self.key))

    def test_distinct_service_capability_and_changed_snapshot_conflict(self):
        with self.assertRaisesRegex(BridgeError, 'confidential_executor_unavailable'):
            self.bridge.dispatch('POST', '/v1/confidential/runs', self.body)
        self.assertIsNone(self.journal.lookup(self.key))
        request = self.request()
        reserve(self.journal, request)
        other = ConfidentialRequest(self.body_for({**self.spec, 'input': 'different'}), self.bridge)
        self.addCleanup(other.close)
        with self.assertRaisesRegex(BridgeError, 'start_conflict'): reserve(self.journal, other)
        self.assertFalse(reserve(self.journal, request)[1])

    def test_sealed_memfd_or_explicit_refusal_never_uses_disk_fallback(self):
        with patch('tempfile.TemporaryFile', side_effect=AssertionError('disk fallback')):
            if not memfd.supported():
                with self.assertRaisesRegex(BridgeError, 'confidential_memfd_unavailable'):
                    memfd.launch(sys.executable, ['-c', 'pass'], b'fixture')
                return
            program = ('import fcntl,sys; s=fcntl.F_SEAL_WRITE|fcntl.F_SEAL_GROW|fcntl.F_SEAL_SHRINK|fcntl.F_SEAL_SEAL; '
                       'assert fcntl.fcntl(0,fcntl.F_GET_SEALS)&s==s; assert sys.stdin.buffer.read()==b"fixture"')
            process = memfd.launch(sys.executable, ['-c', program], b'fixture')
            try:
                self.assertEqual(process.wait(timeout=3), 0)
            finally:
                if process.poll() is None:
                    process.kill(); process.wait(timeout=2)

    def test_encoded_http_path_uses_strict_duplicate_parser_and_raw_body_budget(self):
        from ortak_hermes_bridge.service import handler
        token = 'fixture-http-token-' + 'a' * 32
        server = HTTPServer(('127.0.0.1', 0), handler(self.bridge, token))
        server.timeout = 2
        try:
            for body, status in ((b'{"company_id":"one","company_id":"two"}', 422),
                                 (b' ' * (112 * 1024 + 1), 413)):
                thread = threading.Thread(target=server.handle_request, daemon=True)
                thread.start()
                connection = http.client.HTTPConnection(*server.server_address, timeout=2)
                try:
                    connection.request('POST', '/v1/%63onfidential/runs', body,
                                       {'Authorization': 'Bearer ' + token, 'Content-Type': 'application/json'})
                    response = connection.getresponse()
                    self.assertEqual(response.status, status)
                    response.read(1024)
                finally:
                    connection.close()
                    thread.join(timeout=3)
                self.assertFalse(thread.is_alive())
        finally:
            server.server_close()
        self.assertIsNone(self.journal.lookup(self.key))


if __name__ == '__main__': unittest.main()
