"""Containment-owner tests bind DockerExecutor/Engine with a fake engine seam."""
import json
import tempfile
import sys
import unittest
from pathlib import Path
from uuid import uuid4
from unittest.mock import patch

from ortak_hermes_bridge.docker_executor import DockerEngine, DockerExecutor, container_name
from ortak_hermes_bridge.journal import BridgeError, Journal
from ortak_hermes_bridge.service import Bridge, EMPTY_POLICY

IMAGE = 'example.invalid/ortak-hermes@sha256:' + 'a' * 64

class Process:
    def __init__(self):
        self.returncode = None
    def poll(self):
        return self.returncode
    def wait(self, timeout):
        self.returncode = 0
        return 0
    def kill(self):
        self.returncode = -9

class Engine:
    def __init__(self):
        self.calls = []
        self.can_stop = True
        self.keys = []
    def validated_image(self, image):
        return image == IMAGE
    def owned_keys(self, company):
        return self.keys
    def launch(self, args, payload):
        self.calls.append(('launch', args, payload))
        return Process()
    def stop(self, key, image):
        self.calls.append(('stop', key, image))
        return self.can_stop

class Containment(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name).resolve()
        self.journal = Journal(self.root / 'state' / 'journal.sqlite')
        self.company, self.run = str(uuid4()), str(uuid4())
        self.key = f'ortak-run:{self.company}:{self.run}'
        self.profile_dir = self.root / 'profile'
        self.profile_dir.mkdir()
        self.binding = {'adapter': 'hermes', 'profile_ref': 'disposable', 'model': 'fixture',
                        'workspace_ref': 'none', 'credential_refs': ['fixture-ref'], 'options': {}}
        self.profile = {'employee_id': 'fixture', 'binding': self.binding, 'directory': str(self.profile_dir)}
        data = {'company_id': self.company, 'employee_id': 'fixture', 'profile_ref': 'disposable'}
        (self.profile_dir / 'ORTAK_DISPOSABLE_PROFILE.json').write_text(json.dumps(data))
        (self.profile_dir / 'ORTAK_RUNTIME_BINDING.json').write_text(json.dumps(self.binding))
        (self.profile_dir / 'ORTAK_PROVIDER.json').write_text(json.dumps({'provider': 'openai', 'credential_ref': 'fixture-ref'}))
        (self.profile_dir / 'provider-token').write_text('fixture-only-not-a-real-key')
        self.spec = {'run_id': self.run, 'employee_id': 'fixture', 'revision_id': str(uuid4()),
                     'binding': self.binding, 'permissions': EMPTY_POLICY, 'input': 'private input',
                     'context': {}, 'idempotency_key': self.key}
        self.engine = Engine()

    def executor(self):
        executor = DockerExecutor(self.journal, self.company, [self.profile], IMAGE,
                                  'ortak-private-test', self.engine, validated_digest=IMAGE)
        self.addCleanup(executor.close)
        return executor

    def test_no_floating_image_or_unvalidated_executor(self):
        for image, digest in [('example:latest', None), (IMAGE, None)]:
            with self.subTest(image=image), self.assertRaises(BridgeError):
                DockerExecutor(self.journal, self.company, [self.profile], image, 'ortak-private', self.engine, validated_digest=digest)

    def test_launch_is_contained_no_gateway_entrypoint_no_secret_argv(self):
        executor = self.executor()
        self.journal.reserve(self.spec)
        executor.start(self.spec, self.journal)
        _, args, payload = self.engine.calls[-1]
        self.assertEqual(args[args.index('--entrypoint') + 1], 'python')
        self.assertIn('--read-only', args)
        self.assertIn('--init', args)
        self.assertIn('ALL', args)
        self.assertIn('no-new-privileges', args)
        self.assertIn('--pids-limit', args)
        self.assertIn('--memory', args)
        self.assertIn('--cpus', args)
        self.assertIn('--log-driver', args)
        self.assertIn('none', args)
        self.assertIn('HOME=/tmp/hermes-home', args)
        self.assertIn('HERMES_HOME=/tmp/hermes-home', args)
        self.assertIn(f'type=bind,src={self.profile_dir},dst=/profile,readonly', args)
        self.assertNotIn(self.spec['input'], repr(args))
        self.assertNotIn('fixture-only-not-a-real-key', repr(args))
        self.assertEqual(json.loads(payload)['spec'], self.spec)

    def test_profile_rejects_old_config_and_symlink(self):
        for name in ('.env', 'config.yaml', 'mcp.json'):
            with self.subTest(name=name):
                p = self.profile_dir / name
                p.write_text('fixture')
                with self.assertRaisesRegex(BridgeError, 'unexpected_profile_contents'):
                    self.executor()
                p.unlink()
        token = self.profile_dir / 'provider-token'
        token.unlink()
        token.symlink_to(self.profile_dir / 'ORTAK_PROVIDER.json')
        with self.assertRaisesRegex(BridgeError, 'unexpected_profile_contents'):
            self.executor()

    def test_profile_health_validates_binding_provider_and_token(self):
        cases = [('ORTAK_RUNTIME_BINDING.json', '{}'),
                 ('ORTAK_PROVIDER.json', '{"provider":"openai","credential_ref":"wrong"}'),
                 ('provider-token', ''), ('provider-token', 'x' * 4097),
                 ('ORTAK_DISPOSABLE_PROFILE.json', 'x' * 8193)]
        for name, invalid in cases:
            with self.subTest(name=name, bytes=len(invalid)):
                path = self.profile_dir / name
                original = path.read_bytes()
                path.write_text(invalid)
                with self.assertRaises(BridgeError):
                    self.executor()
                path.write_bytes(original)

    def test_engine_read_ceiling_applies_before_eof(self):
        engine = DockerEngine(binary=sys.executable)
        with self.assertRaisesRegex(BridgeError, 'container_engine_invalid_response'):
            engine.command(['-c', "import sys,time; sys.stdout.write('x'*2048); sys.stdout.flush(); time.sleep(60)"])
        self.assertEqual(engine.command(['-c', "print('bounded')"]), (0, 'bounded'))

    def test_exclusive_executor_owner(self):
        self.executor()
        with self.assertRaisesRegex(BridgeError, 'executor_already_owned'):
            self.executor()

    def test_restart_stops_even_terminal_container_and_never_launches(self):
        self.journal.reserve(self.spec)
        self.journal.fail(self.key, 'executor_unavailable')
        self.engine.keys = [self.key]
        self.executor()
        self.assertIn(('stop', self.key, IMAGE), self.engine.calls)
        self.assertFalse(any(c[0] == 'launch' for c in self.engine.calls))
        self.assertEqual(self.journal.lookup(self.key)['status'], 'failed')

    def test_failed_start_still_requires_stop_before_cancel_ack(self):
        executor = self.executor()
        bridge = Bridge(self.journal, self.company, [self.profile], executor)
        self.journal.reserve(self.spec)
        self.journal.fail(self.key, 'executor_unavailable')
        self.engine.can_stop = False
        request = {'company_id': self.company, 'run_id': self.run, 'idempotency_key': self.key, 'reason': 'stop'}
        with self.assertRaisesRegex(BridgeError, 'execution_not_stopped'):
            bridge.dispatch('POST', '/v1/runs/cancel', request)
        self.engine.can_stop = True
        self.assertEqual(bridge.dispatch('POST', '/v1/runs/cancel', request)['outcome'], 'already_terminal')

    def test_unknown_labeled_container_is_not_deleted(self):
        self.engine.keys = [self.key]
        with self.assertRaisesRegex(BridgeError, 'orphan_container_without_registry'):
            self.executor()
        self.assertFalse(any(c[0] == 'stop' for c in self.engine.calls))

    def test_inventory_excludes_company_services_without_execution_keys(self):
        engine = DockerEngine()
        with patch.object(engine, 'command', return_value=(0, self.key)) as command:
            self.assertEqual(engine.owned_keys(self.company), [self.key])
            self.assertIn('label=org.ortak.start_key', command.call_args.args[0])

    def test_daemon_error_does_not_prove_absence(self):
        engine = DockerEngine()
        with patch.object(engine, 'command', return_value=(1, '')):
            self.assertFalse(engine.stop(self.key, IMAGE))
            self.assertFalse(engine.stopped(container_name(self.key)))

    def test_wrong_image_or_owner_is_never_removed(self):
        engine = DockerEngine()
        name = container_name(self.key)
        with patch.object(engine, 'command', side_effect=[(0, name), (0, f'/{name}|wrong-company|{self.key}|{IMAGE}')]) as command:
            self.assertFalse(engine.stop(self.key, IMAGE))
            self.assertFalse(any(call.args[0][:2] == ['container', 'rm'] for call in command.call_args_list))

    def test_exact_owner_removed_then_absence_verified(self):
        engine = DockerEngine()
        name = container_name(self.key)
        replies = [(0, name), (0, f'/{name}|{self.company}|{self.key}|{IMAGE}'), (0, name), (0, '')]
        with patch.object(engine, 'command', side_effect=replies) as command:
            self.assertTrue(engine.stop(self.key, IMAGE))
            self.assertEqual(command.call_args_list[2].args[0], ['container', 'rm', '--force', name])

if __name__ == '__main__':
    unittest.main()
