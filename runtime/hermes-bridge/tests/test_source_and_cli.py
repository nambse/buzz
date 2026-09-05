"""Source-byte verification and explicit CLI composition regressions."""
import hashlib
import importlib
import json
import os
import signal
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch, Mock
from uuid import uuid4
from ortak_hermes_bridge import HERMES_REVISION
from ortak_hermes_bridge.__main__ import configured_bridge
from ortak_hermes_bridge.journal import BridgeError, Journal
from ortak_hermes_bridge.service import serve
from ortak_hermes_bridge.worker import isolate_environment, prepare_home, arm_deadline

source_module = importlib.import_module('ortak_hermes_bridge.verify_source')

class SourceVerification(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.source = self.root / 'source'
        self.source.mkdir()
        self.file = self.source / 'run_agent.py'
        self.file.write_text('fixture source')
        self.package = self.root / 'bridge' / 'ortak_hermes_bridge'
        self.package.mkdir(parents=True)
        self.lock = self.package.parent / 'hermes-source-lock.json'
        self.lock.write_text(json.dumps({'revision': HERMES_REVISION,
                                        'source_files': {'run_agent.py': hashlib.sha256(self.file.read_bytes()).hexdigest()}}))
        patcher = patch.object(source_module, '__file__', str(self.package / 'verify_source.py'))
        patcher.start()
        self.addCleanup(patcher.stop)

    def test_exact_bytes_pass_but_version_marker_alone_does_not(self):
        self.assertEqual(source_module.verify_source(self.source)['revision'], HERMES_REVISION)
        (self.source / 'ORTAK_SOURCE_REVISION').write_text(HERMES_REVISION)
        self.file.write_text('tampered source')
        with self.assertRaisesRegex(BridgeError, 'image_source_mismatch'):
            source_module.verify_source(self.source)

    def test_source_environment_and_symlink_are_refused(self):
        environment = self.source / '.env'
        environment.write_text('fixture')
        with self.assertRaises(BridgeError):
            source_module.verify_source(self.source)
        environment.unlink()
        target = self.root / 'outside.py'
        target.write_text(self.file.read_text())
        self.file.unlink()
        self.file.symlink_to(target)
        with self.assertRaises(BridgeError):
            source_module.verify_source(self.source)

    def test_wrong_revision_is_refused(self):
        data = json.loads(self.lock.read_text())
        data['revision'] = 'main'
        self.lock.write_text(json.dumps(data))
        with self.assertRaises(BridgeError):
            source_module.verify_source(self.source)

class CliComposition(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.journal = Journal(Path(self.temporary.name) / 'journal.sqlite')
        self.config = {'company_id': str(uuid4()), 'profiles': [],
                       'executor': {'image': 'sha256:' + 'a'*64, 'network': 'ortak-private',
                                    'validated_digest': 'sha256:' + 'a'*64}}

    def test_config_alone_does_not_enable_execution(self):
        with patch('ortak_hermes_bridge.docker_executor.DockerExecutor') as constructor:
            bridge = configured_bridge(self.config, self.journal)
            self.assertFalse(bridge.executor.available)
            constructor.assert_not_called()

    def test_explicit_opt_in_passes_exact_validation_identity(self):
        executor = Mock(available=True)
        with patch('ortak_hermes_bridge.docker_executor.DockerExecutor', return_value=executor) as constructor:
            bridge = configured_bridge(self.config, self.journal, True)
            self.assertIs(bridge.executor, executor)
            self.assertEqual(constructor.call_args.kwargs['validated_digest'], self.config['executor']['image'])

    def test_invalid_company_does_not_construct_or_leak_executor(self):
        self.config['company_id'] = 'invalid'
        with patch('ortak_hermes_bridge.docker_executor.DockerExecutor') as constructor:
            with self.assertRaises((BridgeError, ValueError)):
                configured_bridge(self.config, self.journal, True)
            constructor.assert_not_called()

    def test_missing_validation_is_not_silently_enabled(self):
        del self.config['executor']['validated_digest']
        with self.assertRaisesRegex(BridgeError, 'executor_validation_required'):
            configured_bridge(self.config, self.journal, True)

class WorkerEnvironment(unittest.TestCase):
    def test_ambient_credentials_and_lazy_install_exception_are_removed(self):
        ambient = {'OPENAI_API_KEY': 'owner-fixture', 'HERMES_LAZY_INSTALL_TARGET': '/old/profile',
                   'HERMES_DISABLE_LAZY_INSTALLS': '0', 'HTTP_PROXY': 'http://fixture.invalid'}
        with patch.dict(os.environ, ambient, clear=True):
            isolate_environment('/tmp/fresh-fixture')
            self.assertEqual(os.environ['HERMES_DISABLE_LAZY_INSTALLS'], '1')
            self.assertNotIn('HERMES_LAZY_INSTALL_TARGET', os.environ)
            self.assertNotIn('OPENAI_API_KEY', os.environ)
            self.assertNotIn('HTTP_PROXY', os.environ)
            self.assertEqual(os.environ['HOME'], os.environ['HERMES_HOME'])
            self.assertEqual(os.environ['LD_LIBRARY_PATH'], '/opt/sqlite-fixed/lib')

    def test_fresh_home_writes_only_fixed_probe_and_install_opt_outs(self):
        with tempfile.TemporaryDirectory() as temporary, patch.dict(os.environ):
            home = Path(temporary) / 'fresh-home'
            prepare_home(home)
            text = (home / 'config.yaml').read_text()
            self.assertIn('environment_probe: false', text)
            self.assertIn('allow_lazy_installs: false', text)
            self.assertEqual((home / 'config.yaml').stat().st_mode & 0o777, 0o600)
            with self.assertRaises(FileExistsError):
                prepare_home(home)

    def test_child_deadline_uses_kernel_signal_and_refuses_namespace_init(self):
        with patch('ortak_hermes_bridge.worker.os.getpid', return_value=1):
            with self.assertRaisesRegex(BridgeError, 'worker_init_required'):
                arm_deadline()
        # The actual production timer kills this separate harmless process. No
        # model, Hermes import, signal handler callback or owner monitor exists.
        result = subprocess.run([sys.executable, '-c',
            'from ortak_hermes_bridge.worker import arm_deadline; import time; arm_deadline(1); time.sleep(10)'],
            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=5)
        self.assertEqual(result.returncode, -signal.SIGALRM)

class ListenerConfiguration(unittest.TestCase):
    def test_loopback_default_and_explicit_container_bind_use_authenticated_handler(self):
        for address in (None, '0.0.0.0'):
            with self.subTest(address=address), patch('ortak_hermes_bridge.service.HTTPServer') as server:
                args = (Mock(), 'fixture-' * 8, 8650)
                serve(*args) if address is None else serve(*args, listen_address=address)
                self.assertEqual(server.call_args.args[0], (address or '127.0.0.1', 8650))
                server.return_value.serve_forever.assert_called_once()
        with patch('ortak_hermes_bridge.service.HTTPServer') as server:
            with self.assertRaisesRegex(BridgeError, 'invalid_service_credential'):
                serve(Mock(), '', 8650, '0.0.0.0')
            server.assert_not_called()

    def test_other_bind_addresses_are_refused(self):
        with patch('ortak_hermes_bridge.service.HTTPServer') as server:
            with self.assertRaisesRegex(BridgeError, 'invalid_listen_address'):
                serve(Mock(), 'fixture-' * 8, 8650, '192.0.2.1')
            server.assert_not_called()

if __name__ == '__main__':
    unittest.main()
