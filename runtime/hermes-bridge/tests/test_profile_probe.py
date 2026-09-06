"""Explicit probe admission and read-only readiness bind the production executor."""
import json
import unittest
from unittest.mock import patch
from uuid import uuid4

import test_docker_executor as fixtures
from test_oauth import access
from ortak_hermes_bridge.oauth_credentials import OAuthStore, oauth_identity, STATE, atomic_write
from ortak_hermes_bridge.journal import BridgeError
from ortak_hermes_bridge.service import Bridge


class OAuthProbe(unittest.TestCase):
    executor = fixtures.Containment.executor
    # Reuse setup/helpers, not the API-key test cases inherited by discovery.
    def setUp(self):
        fixtures.Containment.setUp(self)
        self.binding.update(model='gpt-6-astra', options={'reasoning_effort': 'max'})
        self.profile['oauth_directory'] = str(self.root / 'oauth')
        (self.profile_dir / 'provider-token').unlink()
        (self.profile_dir / 'ORTAK_RUNTIME_BINDING.json').write_text(json.dumps(self.binding))
        (self.profile_dir / 'ORTAK_PROVIDER.json').write_text(json.dumps(
            {'provider': 'openai-codex', 'credential_ref': 'fixture-ref'}))
        self.store = OAuthStore.create(self.profile['oauth_directory'],
            oauth_identity(self.company, 'fixture', self.binding))
        self.tokens = {'access_token': access(), 'refresh_token': 'fixture-private-refresh-value'}
        self.store.enroll(lambda: {'tokens': self.tokens})
        self.engine.stopped = lambda name: self.engine.can_stop

    def test_explicit_probe_identity_completion_and_readiness_expiry(self):
        executor = self.executor()
        bridge = Bridge(self.journal, self.company, [self.profile], executor)
        request = {'company_id': self.company, 'binding': self.binding, 'probe_id': self.run}
        with patch('ortak_hermes_bridge.oauth_credentials.OAuthProcess.call',
                   side_effect=AssertionError('unexpired read must never call provider')):
            self.assertFalse(executor.inspect(self.binding))
            inspected = bridge.dispatch('POST', '/v1/profiles/inspect',
                                        {'company_id': self.company, 'binding': self.binding})
            self.assertEqual(inspected['credential_references'], ['fixture-ref'])
            self.assertFalse(inspected['healthy'])
            first = bridge.dispatch('POST', '/v1/profiles/probe', request)
            self.assertEqual(bridge.dispatch('POST', '/v1/profiles/probe', request), first)
            self.assertEqual(len([c for c in self.engine.calls if c[0] == 'launch']), 1)
            args, payload = self.engine.calls[-1][1:]
            self.assertEqual(json.loads(payload)['oauth_access_token'], self.tokens['access_token'])
            for secret in self.tokens.values():
                self.assertNotIn(secret, repr(args))
            self.assertNotIn(self.tokens['refresh_token'], payload.decode())
            self.assertNotIn(self.profile['oauth_directory'], repr(args))
            self.assertFalse(executor.inspect(self.binding))
            self.assertTrue(self.journal.begin_execution(self.key))
            self.journal.complete(self.key, 'OK')
            self.assertFalse(executor.inspect(self.binding), 'completion alone is not containment')
            self.assertTrue(executor.stop(self.key))
            self.assertTrue(executor.inspect(self.binding))
            with patch('ortak_hermes_bridge.journal.time.time', return_value=10**11):
                self.assertFalse(executor.inspect(self.binding))
            self.engine.can_stop = False
            self.assertFalse(executor.inspect(self.binding))
            self.engine.can_stop = True
            previous_image = executor.image
            executor.image = 'sha256:' + 'b' * 64
            self.assertFalse(executor.inspect(self.binding))
            executor.image = previous_image
            state = self.store.read(); state['generation'] += 1
            atomic_write(self.store.directory / STATE, state)
            self.assertFalse(executor.inspect(self.binding))
            with self.assertRaisesRegex(BridgeError, 'probe_conflict'):
                bridge.dispatch('POST', '/v1/profiles/probe', request)
            with self.journal.connection() as db:
                encoded = json.dumps([dict(row) for row in db.execute('SELECT * FROM profile_probes')])
            for secret in self.tokens.values():
                self.assertNotIn(secret, encoded)

    def test_probe_scope_and_failure_are_durable_without_launch_replay(self):
        executor = self.executor()
        bridge = Bridge(self.journal, self.company, [self.profile], executor)
        request = {'company_id': self.company, 'binding': self.binding, 'probe_id': self.run}
        with self.assertRaisesRegex(BridgeError, 'profile_not_found'):
            bridge.dispatch('POST', '/v1/profiles/probe', dict(request, company_id=str(uuid4())))
        with patch.object(self.engine, 'launch', side_effect=OSError('fixture engine failure')) as launch:
            with self.assertRaisesRegex(BridgeError, 'executor_unavailable'):
                bridge.dispatch('POST', '/v1/profiles/probe', request)
            self.assertEqual(self.journal.lookup(self.key)['status'], 'failed')
            self.assertEqual(bridge.dispatch('POST', '/v1/profiles/probe', request)['status'], 'failed')
            launch.assert_called_once()
        self.assertFalse(executor.inspect(self.binding))

    def test_probe_metadata_and_run_reservation_are_atomic(self):
        with self.journal.connection() as db:
            db.execute("CREATE TRIGGER fail_probe BEFORE INSERT ON profile_probes BEGIN SELECT RAISE(ABORT,'fixture'); END")
        with self.assertRaises(Exception):
            self.journal.reserve(self.spec, probe_selection={'fixture': True})
        self.assertIsNone(self.journal.lookup(self.key))

    def test_ordinary_run_cannot_be_adopted_as_health_probe(self):
        self.journal.reserve(self.spec)
        with self.assertRaisesRegex(BridgeError, 'probe_conflict'):
            self.journal.reserve(self.spec, probe_selection={'fixture': True})
