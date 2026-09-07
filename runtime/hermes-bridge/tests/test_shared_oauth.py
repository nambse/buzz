"""Registered consumers use one real private fixture store through production seams."""
import copy
from concurrent.futures import ThreadPoolExecutor
import json
import time
import unittest
from unittest.mock import patch
from uuid import uuid4

from test_docker_executor import DockerExecutor, IMAGE
from test_oauth import access
import test_profile_probe
from ortak_hermes_bridge.journal import BridgeError
from ortak_hermes_bridge.oauth_credentials import MARKER, STATE, OAuthStore, atomic_write, oauth_identity
from ortak_hermes_bridge.profile_probe import selected_profile
from ortak_hermes_bridge.service import Bridge


class SharedConnection(unittest.TestCase):
    setUp = test_profile_probe.OAuthProbe.setUp

    def consumer(self, employee='bora', owner=None, directory=None):
        profile = copy.deepcopy(self.profile)
        profile['employee_id'] = employee
        profile['binding'].update(profile_ref=f'{employee}-profile', model='gpt-5.6-sol',
                                  options={'reasoning_effort': 'high'})
        root = self.root / f'{employee}-profile'
        root.mkdir()
        profile['directory'] = str(root)
        profile['oauth_directory'] = str(directory or self.store.directory)
        profile['oauth_owner'] = dict(owner or self.store.identity)
        for name, value in {
                'ORTAK_DISPOSABLE_PROFILE.json': {'company_id': self.company, 'employee_id': employee,
                                                 'profile_ref': profile['binding']['profile_ref']},
                'ORTAK_RUNTIME_BINDING.json': profile['binding'],
                'ORTAK_PROVIDER.json': {'provider': 'openai-codex', 'credential_ref': 'fixture-ref'}}.items():
            (root / name).write_text(json.dumps(value))
        return profile

    def executor(self, profiles):
        executor = DockerExecutor(self.journal, self.company, profiles, IMAGE,
                                  'ortak-private-test', self.engine, validated_digest=IMAGE)
        self.addCleanup(executor.close)
        return executor

    def complete_probe(self, bridge, executor, profile):
        run = str(uuid4())
        key = f'ortak-run:{self.company}:{run}'
        request = {'company_id': self.company, 'binding': profile['binding'], 'probe_id': run}
        receipt = bridge.dispatch('POST', '/v1/profiles/probe', request)
        self.assertEqual(bridge.dispatch('POST', '/v1/profiles/probe', request), receipt)
        self.assertTrue(self.journal.begin_execution(key))
        self.assertTrue(self.journal.complete(key, 'OK'))
        self.assertTrue(executor.stop(key))
        self.assertTrue(executor.inspect(profile['binding']))

    def test_two_employees_share_owner_variants_without_copy_or_identity_change(self):
        consumers = [self.consumer(), self.consumer('deniz')]
        variant = copy.deepcopy(self.profile)
        variant['binding'].update(model='gpt-5.6-sol', options={'reasoning_effort': 'low'})
        variant_root = self.root / 'owner-variant'; variant_root.mkdir()
        variant['directory'] = str(variant_root)
        for source in self.profile_dir.iterdir():
            (variant_root / source.name).write_bytes(source.read_bytes())
        (variant_root / 'ORTAK_RUNTIME_BINDING.json').write_text(json.dumps(variant['binding']))
        profiles = [self.profile, variant, *consumers]
        before = {path: path.read_bytes() for path in self.store.directory.iterdir()}
        executor = self.executor(profiles)
        bridge = Bridge(self.journal, self.company, profiles, executor)
        config = {'company_id': self.company, 'profiles': profiles}
        with patch('ortak_hermes_bridge.oauth_credentials.OAuthProcess.call',
                   side_effect=AssertionError('no enrollment or refresh of current fixture token')):
            for profile in consumers:
                self.assertEqual(selected_profile(config, profile['employee_id']), profile)
                self.assertEqual(executor.credential_references(profile['binding']), ['fixture-ref'])
                self.complete_probe(bridge, executor, profile)
                run = str(uuid4())
                spec = dict(self.spec, employee_id=profile['employee_id'], binding=profile['binding'],
                            run_id=run, idempotency_key=f'ortak-run:{self.company}:{run}')
                bridge.dispatch('POST', '/v1/runs', {'company_id': self.company, 'spec': spec})
                _, args, payload = next(call for call in reversed(self.engine.calls) if call[0] == 'launch')
                request = json.loads(payload)
                self.assertEqual(request, {'company_id': self.company, 'spec': spec,
                                           'oauth_access_token': self.tokens['access_token']})
                self.assertIn(f"type=bind,src={profile['directory']},dst=/profile,readonly", args)
                self.assertNotIn(str(self.store.directory), repr(args))
                self.assertNotIn(self.tokens['refresh_token'], payload.decode())
                self.assertTrue(self.journal.begin_execution(spec['idempotency_key']))
                self.assertTrue(self.journal.complete(spec['idempotency_key'], 'fixture result'))
                self.assertTrue(executor.stop(spec['idempotency_key']))
            self.assertEqual(before, {path: path.read_bytes() for path in before})
            self.assertNotIn('oauth_owner_sha256', executor.probe_selection(self.profile))
            for profile in consumers:
                self.assertEqual(len(executor.probe_selection(profile)['oauth_owner_sha256']), 64)

    def test_invalid_grants_refuse_before_secret_or_executor_io(self):
        consumer = self.consumer()
        cases = []
        for field, value in [('company_id', str(uuid4())), ('credential_ref', 'other'),
                             ('employee_id', 'unregistered'), ('profile_ref', 'unknown'),
                             ('format', 'unrecognized')]:
            changed = copy.deepcopy(consumer); changed['oauth_owner'][field] = value
            cases.append([self.profile, changed])
        cases.append([self.profile, dict(consumer, oauth_directory=str(self.root / 'other'))])
        cases.append([self.profile, dict(consumer, oauth_owner=None)])
        cases.append([self.profile, dict(consumer, oauth_owner={**consumer['oauth_owner'], 'extra': True})])
        cases.append([consumer])
        chain_owner = dict(self.profile, oauth_owner=oauth_identity(self.company, 'other', self.binding))
        cases.append([chain_owner, consumer])
        cyclic_owner = dict(self.profile, oauth_owner=oauth_identity(self.company, consumer['employee_id'], consumer['binding']))
        cases.append([cyclic_owner, consumer])
        no_store = {key: value for key, value in consumer.items() if key != 'oauth_directory'}
        cases.append([self.profile, no_store])
        with patch('ortak_hermes_bridge.oauth_credentials.private_read',
                   side_effect=AssertionError('invalid grant opened a secret')) as secret:
            for profiles in cases:
                with self.subTest(profiles=profiles):
                    with self.assertRaises(BridgeError):
                        Bridge(self.journal, self.company, profiles)
                    with self.assertRaises(BridgeError):
                        DockerExecutor(self.journal, self.company, profiles, IMAGE,
                                       'ortak-private-test', self.engine, validated_digest=IMAGE)
            secret.assert_not_called()
        self.assertEqual(self.engine.calls, [])
        self.assertFalse((self.root / 'state' / 'executor.lock').exists())

    def test_frozen_grant_cannot_be_replaced_by_config_or_request(self):
        consumer = self.consumer()
        profiles = [self.profile, consumer]
        executor = self.executor(profiles)
        bridge = Bridge(self.journal, self.company, profiles, executor)
        original = copy.deepcopy(consumer)
        consumer['oauth_owner']['employee_id'] = 'changed-after-construction'
        consumer['binding']['credential_refs'] = ['changed-after-construction']
        self.assertEqual(executor.oauth_store(executor.profiles[1]).identity, self.store.identity)
        spec = dict(self.spec, employee_id=original['employee_id'], binding=original['binding'])
        for body in [{'company_id': self.company, 'spec': spec, 'oauth_owner': original['oauth_owner']},
                     {'company_id': self.company, 'spec': dict(spec, oauth_owner=original['oauth_owner'])},
                     {'company_id': self.company, 'spec': dict(spec, binding=consumer['binding'])}]:
            with self.assertRaises(BridgeError): bridge.validate(body)
        self.assertEqual(self.engine.calls, [])

    def test_absent_grant_and_changed_marker_do_not_fall_back(self):
        consumer = self.consumer()
        ungranted = {key: value for key, value in consumer.items() if key != 'oauth_owner'}
        with self.assertRaisesRegex(BridgeError, 'oauth_identity_mismatch'):
            DockerExecutor(self.journal, self.company, [self.profile, ungranted], IMAGE,
                           'ortak-private-test', self.engine, validated_digest=IMAGE)
        executor = self.executor([self.profile, consumer])
        atomic_write(self.store.directory / MARKER, dict(self.store.identity, employee_id='replaced'))
        self.assertEqual(executor.credential_references(consumer['binding']), [])
        with self.assertRaisesRegex(BridgeError, 'oauth_identity_mismatch'):
            executor.oauth_store(executor.profiles[1]).access_token()
        self.assertEqual(self.engine.calls, [])

    def test_shared_refresh_is_single_rotation_and_invalidates_both_probe_witnesses(self):
        consumers = [self.consumer(), self.consumer('deniz')]
        executor = self.executor([self.profile, *consumers])
        bridge = Bridge(self.journal, self.company, executor.profiles, executor)
        for profile in consumers: self.complete_probe(bridge, executor, profile)
        state = self.store.read(); state['tokens']['access_token'] = access(20)
        atomic_write(self.store.directory / STATE, state)
        rotated = {'access_token': access(7200), 'refresh_token': 'fixture-rotated-once'}
        marker = (self.store.directory / MARKER).read_bytes()
        def refresh(action, payload):
            self.assertEqual(action, 'refresh')
            self.assertEqual(self.store.read()['phase'], 'refreshing')
            time.sleep(0.03)
            return rotated
        with patch('ortak_hermes_bridge.oauth_credentials.OAuthProcess.call', side_effect=refresh) as call:
            stores = [executor.oauth_store(profile) for profile in consumers]
            with ThreadPoolExecutor(max_workers=2) as pool:
                self.assertEqual(list(pool.map(lambda store: store.access_token(), stores)),
                                 [rotated['access_token']] * 2)
            call.assert_called_once()
        self.assertEqual(self.store.read()['generation'], 2)
        self.assertEqual((self.store.directory / MARKER).read_bytes(), marker)
        self.assertTrue(all(not executor.inspect(profile['binding']) for profile in consumers))

    def test_owner_remap_with_identical_token_generation_requires_new_probe(self):
        consumer = self.consumer()
        executor = self.executor([self.profile, consumer])
        bridge = Bridge(self.journal, self.company, executor.profiles, executor)
        self.complete_probe(bridge, executor, consumer)
        previous = executor.probe_selection(consumer)
        alternate = self.consumer('alternate')
        del alternate['oauth_owner']
        alternate['oauth_directory'] = str(self.root / 'alternate-store')
        store = OAuthStore.create(alternate['oauth_directory'],
                                  oauth_identity(self.company, 'alternate', alternate['binding']))
        store.enroll(lambda: {'tokens': self.tokens})
        rebound = copy.deepcopy(consumer)
        rebound.update(oauth_owner=dict(store.identity), oauth_directory=str(store.directory))
        # A real deployment remapping closes the old executor before selection.
        executor.close()
        replacement = self.executor([alternate, rebound])
        current = replacement.probe_selection(rebound)
        self.assertEqual({k: v for k, v in previous.items() if k != 'oauth_owner_sha256'},
                         {k: v for k, v in current.items() if k != 'oauth_owner_sha256'})
        self.assertNotEqual(previous['oauth_owner_sha256'], current['oauth_owner_sha256'])
        self.assertFalse(replacement.inspect(rebound['binding']))
