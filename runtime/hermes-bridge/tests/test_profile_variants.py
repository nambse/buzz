"""Model selection binds the production registry, journal and OAuth executor."""
import copy
import hashlib
import json
import unittest
from types import SimpleNamespace
from unittest.mock import patch
from uuid import uuid4

import test_bridge
import test_profile_probe
from test_docker_executor import DockerExecutor, IMAGE
from ortak_hermes_bridge.journal import BridgeError
from ortak_hermes_bridge.oauth_credentials import STATE
from ortak_hermes_bridge.profile_probe import selected_profile
from ortak_hermes_bridge.service import Bridge


class RegistryVariants(unittest.TestCase):
    setUp = test_bridge.Fixture.setUp

    def test_new_model_keeps_existing_run_binding_and_exact_selection(self):
        starts = []
        executor = SimpleNamespace(available=True, start=lambda spec, journal: starts.append(spec))
        original = Bridge(self.journal, self.company, self.profiles, executor)
        receipt = original.dispatch('POST', '/v1/runs', self.body)
        variant = copy.deepcopy(self.profiles[0])
        variant['binding'].update(model='another-model', options={'reasoning_effort': 'high'})
        configured = self.profiles + [variant]
        bridge = Bridge(self.journal, self.company, configured, executor)
        variant['binding']['model'] = 'changed-after-construction'
        self.assertEqual(bridge.dispatch('POST', '/v1/runs', self.body), receipt)
        new_spec = copy.deepcopy(self.spec)
        new_spec.update(run_id=str(uuid4()), revision_id=str(uuid4()), binding=bridge.profiles[1]['binding'])
        new_spec['idempotency_key'] = f"ortak-run:{self.company}:{new_spec['run_id']}"
        bridge.dispatch('POST', '/v1/runs', {'company_id': self.company, 'spec': new_spec})
        self.assertEqual([s['binding']['model'] for s in starts], ['test-model', 'another-model'])
        with self.assertRaisesRegex(BridgeError, 'start_conflict'):
            bridge.dispatch('POST', '/v1/runs', {'company_id': self.company,
                'spec': dict(self.spec, binding=new_spec['binding'])})
        for binding in [dict(new_spec['binding'], model='unregistered'),
                        dict(new_spec['binding'], options={'reasoning_effort': 'max'})]:
            with self.subTest(binding=binding), self.assertRaisesRegex(BridgeError, 'profile_not_found'):
                bridge.validate({'company_id': self.company, 'spec': dict(new_spec, binding=binding)})

    def test_duplicate_and_changed_profile_ownership_are_rejected(self):
        first = dict(self.profiles[0], oauth_directory='/private/fixture/oauth')
        second = copy.deepcopy(first)
        second['binding']['model'] = 'second-model'
        cases = [copy.deepcopy(first)]
        for field, value in [('credential_refs', ['different']), ('workspace_ref', 'different'),
                             ('adapter', 'different')]:
            changed = copy.deepcopy(second)
            changed['binding'][field] = value
            cases.append(changed)
        cases.extend([dict(second, employee_id='other'),
                      dict(second, oauth_directory='/private/other/oauth'), self.profiles[0]])
        for variant in cases:
            with self.subTest(variant=variant), self.assertRaisesRegex(BridgeError, 'invalid_profile_registry'):
                Bridge(self.journal, self.company, [first, variant])
        variants = [dict(first, binding=dict(first['binding'], model=f'model-{i}')) for i in range(65)]
        self.assertEqual(len(Bridge(self.journal, self.company, variants[:64]).profiles), 64)
        with self.assertRaisesRegex(BridgeError, 'invalid_profile_registry'):
            Bridge(self.journal, self.company, variants)

    def test_operator_probe_requires_full_variant_fingerprint(self):
        first = dict(self.profiles[0], oauth_directory='/private/fixture/oauth')
        second = copy.deepcopy(first)
        second['binding']['options'] = {'reasoning_effort': 'high'}
        config = {'profiles': [first, second]}
        with self.assertRaisesRegex(BridgeError, 'oauth_profile_required'):
            selected_profile(config, 'disposable')
        digest = hashlib.sha256(json.dumps(second['binding'], sort_keys=True,
                                          separators=(',', ':')).encode()).hexdigest()
        self.assertEqual(selected_profile(config, 'disposable', digest), second)
        self.assertEqual(selected_profile({'profiles': [first]}, 'disposable'), first)
        for employee, fingerprint, error in [('other', digest, 'oauth_profile_required'),
                ('disposable', '0' * 64, 'oauth_profile_required'),
                ('disposable', 'bad', 'invalid_binding_fingerprint')]:
            with self.subTest(employee=employee, fingerprint=fingerprint), self.assertRaisesRegex(BridgeError, error):
                selected_profile(config, employee, fingerprint)


class OAuthVariants(unittest.TestCase):
    setUp = test_profile_probe.OAuthProbe.setUp

    def test_variants_share_one_enrollment_and_launch_exact_immutable_profiles(self):
        variant = copy.deepcopy(self.profile)
        variant['binding'].update(model='gpt-5.6-sol', options={'reasoning_effort': 'high'})
        directory = self.root / 'second-profile'
        directory.mkdir()
        variant['directory'] = str(directory)
        for source in self.profile_dir.iterdir():
            (directory / source.name).write_bytes(source.read_bytes())
        (directory / 'ORTAK_RUNTIME_BINDING.json').write_text(json.dumps(variant['binding']))
        before = {p: p.read_bytes() for root in (self.profile_dir, directory, self.store.directory)
                  for p in root.iterdir() if p.is_file()}
        profiles = [self.profile, variant]
        executor = DockerExecutor(self.journal, self.company, profiles, IMAGE,
                                  'ortak-private-test', self.engine, validated_digest=IMAGE)
        self.addCleanup(executor.close)
        bridge = Bridge(self.journal, self.company, profiles, executor)
        with patch('ortak_hermes_bridge.oauth_credentials.OAuthProcess.call',
                   side_effect=AssertionError('shared current enrollment must not refresh or enroll')):
            for index, profile in enumerate(profiles):
                probe_id = str(uuid4())
                key = f'ortak-run:{self.company}:{probe_id}'
                self.assertFalse(executor.inspect(profile['binding']))
                self.assertEqual(executor.credential_references(profile['binding']), ['fixture-ref'])
                bridge.dispatch('POST', '/v1/profiles/probe', {'company_id': self.company,
                    'binding': profile['binding'], 'probe_id': probe_id})
                _, args, payload = self.engine.calls[-1]
                self.assertIn(f"type=bind,src={profile['directory']},dst=/profile,readonly", args)
                request = json.loads(payload)
                self.assertEqual(request['spec']['binding'], profile['binding'])
                self.assertEqual(request['oauth_access_token'], self.tokens['access_token'])
                self.assertNotIn(self.tokens['refresh_token'], payload.decode())
                self.assertNotIn(self.profile['oauth_directory'], repr(args))
                self.assertTrue(self.journal.begin_execution(key))
                self.assertTrue(self.journal.complete(key, 'OK'))
                self.assertTrue(executor.stop(key))
                self.assertTrue(executor.inspect(profile['binding']))
                if index == 0:
                    self.assertFalse(executor.inspect(variant['binding']), 'another model needs its own witness')
            self.assertEqual(before, {p: p.read_bytes() for p in before})
            state = self.store.read()
            state['generation'] += 1
            from ortak_hermes_bridge.oauth_credentials import atomic_write
            atomic_write(self.store.directory / STATE, state)
            self.assertTrue(all(not executor.inspect(p['binding']) for p in profiles))
