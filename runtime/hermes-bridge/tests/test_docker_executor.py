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

    def executor(self, *, workspace=False):
        executor = DockerExecutor(self.journal, self.company, [self.profile], IMAGE,
                                  'ortak-private-test', self.engine, validated_digest=IMAGE,
                                  workspace_validated_digest=IMAGE if workspace else None)
        self.addCleanup(executor.close)
        return executor

    def test_no_floating_image_or_unvalidated_executor(self):
        for image, digest in [('example:latest', None), (IMAGE, None)]:
            with self.subTest(image=image), self.assertRaises(BridgeError):
                DockerExecutor(self.journal, self.company, [self.profile], image, 'ortak-private', self.engine, validated_digest=digest)

    def test_workspace_grant_enters_stdin_without_any_new_mount_or_credential_path(self):
        from test_workspace_tools import selected
        grant, _ = selected(self.spec, self.company)
        executor = self.executor(workspace=True)
        self.journal.reserve(self.spec, workspace=grant)
        executor.start(self.spec, self.journal, workspace=grant)
        _, args, payload = self.engine.calls[-1]
        request = json.loads(payload)
        self.assertEqual(request, {'company_id': self.company, 'spec': self.spec, 'workspace': grant})
        mounts = [args[index + 1] for index, arg in enumerate(args) if arg == '--mount']
        self.assertEqual(mounts, [f'type=bind,src={self.profile_dir},dst=/profile,readonly',
                                  f'type=bind,src={self.root / "state"},dst=/ortak-state'])
        self.assertNotIn(grant['files'][0]['file_id'], repr(args))
        with patch.object(executor, 'validate_profile', side_effect=AssertionError('credential/profile I/O before policy gate')) as validate:
            with self.assertRaisesRegex(BridgeError, 'invalid_workspace'):
                executor.start(self.spec, self.journal, workspace={**grant, 'manifest_hash': '0' * 64})
            validate.assert_not_called()

    def test_workspace_capability_requires_second_exact_image_validation(self):
        from test_workspace_tools import selected
        grant, _ = selected(self.spec, self.company)
        executor = self.executor()
        bridge = Bridge(self.journal, self.company, [self.profile], executor)
        self.assertTrue(executor.available)
        self.assertFalse(executor.workspace_text_read)
        self.assertNotIn('workspace_text_read', bridge.dispatch('GET', '/v1/capabilities')['capabilities'])
        with self.assertRaisesRegex(BridgeError, 'unsupported_permission_policy'):
            bridge.dispatch('POST', '/v1/runs', {'company_id': self.company, 'spec': self.spec, 'workspace': grant})
        self.assertIsNone(self.journal.lookup(self.key))
        self.assertEqual(self.engine.calls, [])
        with patch.object(executor, 'validate_profile', side_effect=AssertionError('profile access before capability gate')) as validate:
            with self.assertRaisesRegex(BridgeError, 'unsupported_permission_policy'):
                executor.start(self.spec, self.journal, workspace=grant)
            validate.assert_not_called()
        with self.assertRaisesRegex(BridgeError, 'workspace_executor_validation_required'):
            DockerExecutor(self.journal, self.company, [self.profile], IMAGE, 'ortak-private-test',
                           self.engine, validated_digest=IMAGE, workspace_validated_digest='sha256:' + 'b' * 64)

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

    def test_stop_proof_survives_auto_remove_between_list_and_inspect(self):
        engine = DockerEngine()
        name = container_name(self.key)
        # The deadline loop observes exited, then the separate whole-container
        # proof races --rm. Both proofs need successful daemon evidence.
        replies = [(0, name), (0, 'false'), (0, name), (1, ''), (0, '')]
        with patch.object(engine, 'command', side_effect=replies) as command:
            self.assertTrue(engine.stopped(name))
            self.assertTrue(engine.stopped(name))
            self.assertEqual(command.call_count, 5)
            self.assertEqual(command.call_args_list[4].args[0],
                             command.call_args_list[2].args[0])

    def test_failed_inspect_requires_fresh_successful_empty_list(self):
        engine = DockerEngine()
        name = container_name(self.key)
        for final_list in ((1, ''), (0, name), (0, 'unexpected-name')):
            with self.subTest(final_list=final_list), patch.object(
                    engine, 'command', side_effect=[(0, name), (1, ''), final_list]) as command:
                self.assertFalse(engine.stopped(name))
                self.assertEqual(command.call_count, 3)
        with patch.object(engine, 'command', side_effect=[
                (0, name), (1, ''), BridgeError('container_engine_unavailable', 503)]):
            with self.assertRaisesRegex(BridgeError, 'container_engine_unavailable'):
                engine.stopped(name)

    def test_running_or_invalid_inspect_does_not_retry_into_absence(self):
        engine = DockerEngine()
        name = container_name(self.key)
        for state in ('true', '', 'False', 'false\ntrue'):
            with self.subTest(state=state), patch.object(
                    engine, 'command', side_effect=[(0, name), (0, state)]) as command:
                self.assertFalse(engine.stopped(name))
                self.assertEqual(command.call_count, 2)

    def test_auto_remove_proof_uses_bounded_real_cli_commands(self):
        name = container_name(self.key)
        binary = self.root / 'fixture-docker'
        trace = self.root / 'fixture-docker-calls.json'
        for last_code, expected in ((0, True), (1, False)):
            with self.subTest(last_code=last_code):
                trace.write_text('[]')
                replies = [(0, name), (1, ''), (last_code, '')]
                binary.write_text(f'#!{sys.executable}\n' +
                    'import json, sys\nfrom pathlib import Path\n' +
                    f'trace = Path({str(trace)!r})\n' +
                    'calls = json.loads(trace.read_text())\n' +
                    f'code, value = {replies!r}[len(calls)]\n' +
                    'calls.append(sys.argv[1:])\ntrace.write_text(json.dumps(calls))\n' +
                    'print(value)\nsys.exit(code)\n')
                binary.chmod(0o700)
                self.assertEqual(DockerEngine(str(binary)).stopped(name), expected)
                calls = json.loads(trace.read_text())
                self.assertEqual(len(calls), 3)
                self.assertEqual(calls[0], calls[2])
                self.assertEqual(calls[0], ['container', 'ls', '--all', '--filter',
                    f'name=^/{name}$', '--format', '{{.Names}}'])
                self.assertEqual(calls[1], ['container', 'inspect', '--format',
                    '{{.State.Running}}', name])

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
