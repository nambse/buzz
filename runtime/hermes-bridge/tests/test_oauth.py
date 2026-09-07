"""Production store/worker/constructor seams with disposable OAuth fixtures only."""
import base64
from concurrent.futures import ThreadPoolExecutor
import json
import os
from pathlib import Path
import tempfile
import time
import unittest
from unittest.mock import Mock, patch
from uuid import uuid4

from ortak_hermes_bridge.hermes_candidate import agent_constructor_kwargs, ToollessTransport, runtime_reasoning
from ortak_hermes_bridge.journal import BridgeError
from ortak_hermes_bridge.oauth_credentials import OAuthStore, STATE, MARKER, atomic_write, oauth_identity
from ortak_hermes_bridge.oauth_flow import flow
from ortak_hermes_bridge.worker import selected_provider_token


def access(expires=3600):
    """Create visibly fake JWT metadata, never an actual provider credential."""
    payload = {'exp': int(time.time()) + expires,
               'https://api.openai.com/auth': {'chatgpt_account_id': 'disposable-test-account'}}
    middle = base64.urlsafe_b64encode(json.dumps(payload).encode()).decode().rstrip('=')
    return 'fixture-header.' + middle + '.fixture-signature'


class OAuthOwnership(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name).resolve()
        self.binding = {'profile_ref': 'fresh', 'credential_refs': ['opaque-fresh'],
                        'model': 'gpt-6-astra', 'options': {'reasoning_effort': 'max'}}
        self.identity = oauth_identity(str(uuid4()), 'fresh', self.binding)
        self.store = OAuthStore.create(self.root / 'oauth', self.identity)
        self.tokens = {'access_token': access(), 'refresh_token': 'fixture-refresh-token-original'}
        self.store.enroll(lambda: {'tokens': self.tokens})

    def expiring(self):
        state = self.store.read()
        state['tokens']['access_token'] = access(20)
        atomic_write(self.store.directory / STATE, state)

    def test_fresh_only_identity_and_private_modes(self):
        self.assertEqual(self.store.directory.stat().st_mode & 0o777, 0o700)
        for path in self.store.directory.iterdir():
            self.assertEqual(path.stat().st_mode & 0o777, 0o600)
        other = dict(self.identity, employee_id='other')
        with self.assertRaisesRegex(BridgeError, 'oauth_identity_mismatch'):
            OAuthStore(self.store.directory, other)
        unmarked = self.root / 'old'; unmarked.mkdir(mode=0o700)
        with self.assertRaisesRegex(BridgeError, 'oauth_state_unavailable'):
            OAuthStore.create(unmarked, self.identity)
        self.assertEqual(list(unmarked.iterdir()), [])

    def test_symlink_hardlink_and_readable_secret_rejected(self):
        state = self.store.directory / STATE
        state.chmod(0o644)
        with self.assertRaisesRegex(BridgeError, 'oauth_file_permissions'):
            self.store.access_token()
        state.chmod(0o600)
        outside = self.root / 'outside'
        os.link(state, outside)
        with self.assertRaisesRegex(BridgeError, 'oauth_file_permissions'):
            self.store.access_token()
        outside.unlink()
        state.rename(outside)
        state.symlink_to(outside)
        with self.assertRaisesRegex(BridgeError, 'oauth_state_unavailable'):
            self.store.access_token()

    def test_concurrent_resolution_refreshes_exactly_once_and_persists_rotation(self):
        self.expiring()
        rotated = {'access_token': access(7200), 'refresh_token': 'fixture-refresh-token-rotated'}
        driver = Mock()
        def refresh(action, payload):
            self.assertEqual(action, 'refresh')
            self.assertEqual(self.store.read()['phase'], 'refreshing')
            time.sleep(0.06)
            return rotated
        driver.call.side_effect = refresh
        stores = [OAuthStore(self.store.directory, self.identity, driver) for _ in range(4)]
        with ThreadPoolExecutor(max_workers=4) as pool:
            values = list(pool.map(lambda store: store.access_token(), stores))
        self.assertEqual(values, [rotated['access_token']] * 4)
        driver.call.assert_called_once()
        self.assertEqual(self.store.read()['generation'], 2)
        self.assertEqual(self.store.read()['tokens'], rotated)

    def test_process_death_after_refresh_fence_never_reuses_token(self):
        class ProcessDeath(BaseException):
            pass
        self.expiring()
        driver = Mock(); driver.call.side_effect = ProcessDeath()
        self.store.driver = driver
        with self.assertRaises(ProcessDeath):
            self.store.access_token()
        self.assertEqual(self.store.read()['phase'], 'refreshing')
        with self.assertRaisesRegex(BridgeError, 'oauth_relogin_required'):
            OAuthStore(self.store.directory, self.identity, driver).access_token()
        driver.call.assert_called_once()

    def test_lost_response_and_failed_persist_do_not_repeat_remote_rotation(self):
        for failure in ('response', 'persist'):
            with self.subTest(failure=failure):
                self.store.enroll(lambda: {'tokens': self.tokens})
                self.expiring()
                driver = Mock(); self.store.driver = driver
                if failure == 'response':
                    driver.call.side_effect = RuntimeError('fixture-secret-must-not-escape')
                    with self.assertRaisesRegex(BridgeError, '^oauth_request_uncertain$'):
                        self.store.access_token()
                else:
                    driver.call.return_value = {'access_token': access(7200), 'refresh_token': 'fixture-next-refresh'}
                    original = atomic_write
                    def publish(path, value):
                        if value['phase'] == 'ready':
                            raise OSError('simulated disk failure')
                        original(path, value)
                    with patch('ortak_hermes_bridge.oauth_credentials.atomic_write', side_effect=publish):
                        with self.assertRaises(OSError):
                            self.store.access_token()
                with self.assertRaisesRegex(BridgeError, 'oauth_relogin_required'):
                    OAuthStore(self.store.directory, self.identity, driver).access_token()
                driver.call.assert_called_once()

    def test_retry_later_has_durable_backoff_and_keeps_selected_owner(self):
        self.expiring()
        self.store.driver = Mock()
        self.store.driver.call.side_effect = BridgeError('oauth_retry_later', 503)
        for _ in range(2):
            with self.assertRaisesRegex(BridgeError, 'oauth_retry_later'):
                self.store.access_token()
        self.store.driver.call.assert_called_once()
        self.assertEqual(self.store.read()['phase'], 'ready')
        self.assertGreater(self.store.read()['retry_at'], time.time())

    def test_local_unexpired_token_is_not_provider_health(self):
        self.store.driver = Mock()
        self.assertEqual(self.store.access_token(), self.tokens['access_token'])
        self.store.driver.call.assert_not_called()
        snapshot = self.store.snapshot()
        self.assertEqual(snapshot['oauth_generation'], 1)
        self.assertNotIn('healthy', snapshot)
        self.assertNotIn(self.tokens['access_token'], json.dumps(snapshot))
        self.store.driver.call.assert_not_called()
        self.expiring()
        with self.assertRaisesRegex(BridgeError, 'oauth_probe_required'):
            self.store.snapshot()
        self.store.driver.call.assert_not_called()

    def test_refresh_errors_are_closed_and_never_include_provider_payloads(self):
        error = RuntimeError('fixture-provider-secret')
        with patch('ortak_hermes_bridge.oauth_flow.load_oauth_helpers', return_value=(Mock(), Mock(side_effect=error))):
            self.assertEqual(flow('refresh', self.tokens), {'error': 'oauth_request_uncertain'})


class RuntimeSelection(unittest.TestCase):
    def setUp(self):
        self.binding = {'model': 'gpt-6-astra', 'options': {'reasoning_effort': 'max'},
                        'credential_refs': ['selected-ref']}
        self.spec = {'binding': self.binding, 'run_id': str(uuid4())}

    def test_constructor_receives_exact_codex_model_effort_and_fixed_endpoint(self):
        kwargs = agent_constructor_kwargs(self.spec, 'openai-codex', access())
        self.assertEqual(kwargs['model'], 'gpt-6-astra')
        self.assertEqual(kwargs['provider'], 'openai-codex')
        self.assertEqual(kwargs['api_mode'], 'codex_responses')
        self.assertEqual(kwargs['base_url'], 'https://chatgpt.com/backend-api/codex')
        self.assertEqual(kwargs['reasoning_config'], {'enabled': True, 'effort': 'max'})

    def test_ultra_missing_and_unknown_options_refused_before_execution(self):
        for options in ({'reasoning_effort': 'ultra'}, {}, {'reasoning_effort': 'max', 'base_url': 'https://other.invalid'}):
            with self.subTest(options=options), self.assertRaises(BridgeError):
                runtime_reasoning(dict(self.binding, options=options), 'openai-codex')

    def test_astra_max_compatibility_corrects_legacy_clamp_and_refuses_model_change(self):
        underlying = Mock(api_mode='codex_responses')
        underlying.build_kwargs.return_value = {'model': 'gpt-6-astra', 'reasoning': {'effort': 'xhigh'}, 'temperature': 1}
        wrapped = ToollessTransport(underlying, Mock(), ('openai-codex', 'gpt-6-astra', 'max'))
        actual = wrapped.build_kwargs('gpt-6-astra', [])
        self.assertEqual(actual['reasoning']['effort'], 'max')
        self.assertNotIn('temperature', actual)
        underlying.build_kwargs.return_value = {'model': 'other-model'}
        with self.assertRaisesRegex(BridgeError, 'runtime_model_changed'):
            wrapped.build_kwargs('gpt-6-astra', [])

    def test_worker_cannot_consult_auth_files_or_use_wrong_credential_ref(self):
        provider = {'provider': 'openai-codex', 'credential_ref': 'selected-ref'}
        token = access()
        with patch('builtins.open', side_effect=AssertionError('worker must not read auth state')):
            self.assertEqual(selected_provider_token(self.spec, provider, token), token)
            with self.assertRaisesRegex(BridgeError, 'credential_binding_mismatch'):
                selected_provider_token(self.spec, dict(provider, credential_ref='other'), token)
            with self.assertRaisesRegex(BridgeError, 'oauth_relogin_required'):
                selected_provider_token(self.spec, provider, access(10))


if __name__ == '__main__':
    unittest.main()
