"""Falsifiable pause dispatch tests: no real process signal, container or secret access."""

from contextlib import contextmanager
import ast
import hashlib
import json
from pathlib import Path
import signal
import tempfile
import time
from types import SimpleNamespace
import unittest
from unittest.mock import Mock, patch

import pause_private_recovery as subject
import private_recovery_obligations as obligations
import private_recovery_journal as selected_journal
import recovery_lock_holder


class PauseTests(unittest.TestCase):
    def test_preflight_records_actual_reviewed_schema_without_assuming_69(self):
        for version in (69, 73, 74):
            value = self.bare(); value.output = Path('/unused/output')
            current = {'main_database': {'migration_checksums':
                [[v, 'a' * 96, True] for v in range(1, version + 1)]}, 'contained_children': []}
            value.expected = current
            value.api = SimpleNamespace(private_directory=lambda path, **_: path, obligations=obligations)
            prepare = SimpleNamespace(observe=lambda _: current, authority=lambda row: row)
            with patch.object(subject.importlib, 'import_module', return_value=prepare):
                value.preflight()
            self.assertEqual(value.event.call_args.args[1]['schema'], version)
            current['main_database']['migration_checksums'].append([75, 'a' * 96, True])
            value.event.reset_mock()
            with patch.object(subject.importlib, 'import_module', return_value=prepare), self.assertRaises(obligations.Refused):
                value.preflight()
            value.event.assert_not_called()

    def test_overall_timer_interrupts_nested_work_and_restores_signal_policy(self):
        with patch.object(subject.signal, 'getitimer', return_value=(0.0, 0.0)), \
                patch.object(subject.signal, 'getsignal', return_value=signal.SIG_DFL), \
                patch.object(subject.signal, 'signal') as install, \
                patch.object(subject.signal, 'setitimer') as timer:
            with self.assertRaises(subject.Refused), subject.overall_deadline():
                callback = install.call_args.args[1]
                callback(signal.SIGALRM, None)
            self.assertEqual(timer.call_args_list[0].args, (signal.ITIMER_REAL, 900))
            self.assertEqual(timer.call_args_list[-1].args, (signal.ITIMER_REAL, 0))
            self.assertEqual(install.call_args.args, (signal.SIGALRM, signal.SIG_DFL))
        with patch.object(subject.signal, 'getitimer', return_value=(5.0, 0.0)), \
                patch.object(subject.signal, 'signal') as install:
            with self.assertRaises(subject.Refused), subject.overall_deadline(): pass
            install.assert_not_called()

    def bare(self):
        value = subject.Pause.__new__(subject.Pause)
        value.effects, value.sequence, value.deadline = [], 0, time.monotonic() + 60
        value.operation = Path('/unused/selected-operation')
        value.registry = {'registry_sha256':'a'*64}
        value.event = Mock()
        value.remaining = Mock(return_value=3)
        value.command = SimpleNamespace(deadline=time.monotonic() + 60,
            docker=lambda *args: ['docker', '--host', 'selected-socket', *args])
        value.inspector = SimpleNamespace(run=Mock(), children=Mock(return_value=[]), container=Mock())
        return value

    def identity(self):
        return {'pid': 123, 'uid': 501, 'started_at': 'Sun Sep 6 00:40:11 2026',
            'executable': '/owned/binary with spaces', 'cwd': '/owned', 'inode': 987,
            'sha256': 'a' * 64}

    def test_each_production_drain_counter_refuses_pending_and_shape_drift(self):
        baseline = dict.fromkeys(subject.COUNTERS, 0)
        baseline['application_clients'] = 7
        subject.drained_counts(baseline, connected_writers=True)
        with self.assertRaises(subject.Refused): subject.drained_counts(baseline, connected_writers=False)
        for name in subject.COUNTERS - {'application_clients'}:
            row = {**baseline, name: 1}
            with self.subTest(name=name), self.assertRaises(subject.Refused):
                subject.drained_counts(row, connected_writers=True)
        for row in ({k: v for k, v in baseline.items() if k != 'active_runs'},
                    {**baseline, 'unknown': 0}, {**baseline, 'active_runs': False}):
            with self.assertRaises(subject.Refused): subject.drained_counts(row, connected_writers=True)

    def test_process_signal_requires_second_current_identity_including_inode(self):
        for key, changed in (('pid', 124), ('inode', 999), ('sha256', 'b' * 64),
                             ('cwd', '/elsewhere'), ('started_at', 'changed')):
            value = self.bare()
            original = self.identity()
            value.process = Mock(side_effect=[original, {**original, key: changed}])
            with self.subTest(key=key), patch.object(subject.os, 'kill') as kill:
                with self.assertRaises(subject.Refused): value.stop_process('ortak-worker')
                kill.assert_not_called()

    def test_process_uses_only_term_then_requires_observed_absence(self):
        value = self.bare()
        current = self.identity()
        value.process = Mock(return_value=current)
        value.inspector.run.return_value = b'null'
        with patch.object(subject.os, 'kill') as kill:
            value.stop_process('ortak-worker')
        kill.assert_called_once_with(current['pid'], signal.SIGTERM)
        self.assertEqual(value.process.call_count, 2)
        self.assertEqual(value.effects[0]['outcome'], 'verified_pid_absent')
        self.assertFalse(value.event.call_args.args[1]['exit_status_available'])

    def test_live_or_reused_pid_never_gets_force_kill_or_success(self):
        for state in ('124 501 Sun Sep 6 00:40:11 2026 /owned/binary with spaces',
                      '123 501 Sun Sep 6 00:40:11 2026 /owned/binary with spaces'):
            value = self.bare()
            value.process = Mock(return_value=self.identity())
            value.remaining.return_value = -1
            value.inspector.run.return_value = json.dumps(state).encode()
            with patch.object(subject.os, 'kill') as kill, self.assertRaises(subject.Refused):
                value.stop_process('ortak-worker')
            kill.assert_called_once_with(123, signal.SIGTERM)
            self.assertNotEqual(value.effects[0]['outcome'], 'verified_pid_absent')

    def container(self, value, name='redis', exit_code=0):
        old = {'id': 'a' * 64, 'image': 'sha256:' + 'b' * 64, 'started_at': 'selected', 'running': True}
        value.expected = {'containers': {name: old}}
        value.inspector.container.return_value = old
        value.container_state = Mock(return_value={'exit_code': exit_code, 'running': False,
            'oom': False, 'pid': 0, 'restarting': False})
        return old

    def test_docker_stop_has_no_daemon_force_kill_and_checks_exact_exit(self):
        value = self.bare()
        old = self.container(value)
        value.stop_container('redis')
        self.assertEqual(value.inspector.run.call_args.args[0], ['docker', '--host', 'selected-socket',
            'stop', '--signal', 'SIGTERM', '--timeout', '-1', old['id']])
        self.assertEqual(value.inspector.container.call_count, 2)
        for name, code in (('redis', 143), ('minio', 137), ('honcho_api', 137), ('controller', 1)):
            value = self.bare(); self.container(value, name, code)
            with self.subTest(name=name), self.assertRaises(subject.Refused): value.stop_container(name)

    def test_container_drift_or_runtime_children_refuse_before_stop(self):
        value = self.bare(); old = self.container(value)
        value.inspector.container.side_effect = [old, {**old, 'started_at': 'replacement'}]
        with self.assertRaises(subject.Refused): value.stop_container('redis')
        value.inspector.run.assert_not_called()
        value = self.bare(); self.container(value, 'controller')
        value.inspector.children.return_value = [{'running': True}]
        with self.assertRaises(subject.Refused): value.stop_container('controller')
        value.inspector.run.assert_not_called()

    def test_transport_failure_retains_uncertain_stop_and_never_retries(self):
        value = self.bare(); self.container(value)
        value.inspector.run.side_effect = subject.Refused('command_deadline_exceeded')
        with self.assertRaises(subject.Refused): value.stop_container('redis')
        self.assertEqual(value.inspector.run.call_count, 1)
        self.assertEqual(value.effects[0]['outcome'], 'stop_not_yet_acknowledged')
        value.container_state.assert_not_called()

    def machine(self, fail=None):
        value = self.bare()
        value.api = SimpleNamespace(Refused=subject.Refused)
        value.resume_plan = Mock(return_value={'automatic_resume': False})
        calls = []
        def action(label):
            calls.append(label)
            if label == fail: raise subject.Refused('fixture_refusal')
        value.preflight = lambda: action('preflight')
        value.drain = lambda label: action('drain:' + label)
        value.stop_process = lambda name: action(name)
        value.stop_container = lambda name: action(name)
        value.publish_held_pause = lambda: action('publish-held')
        return value, calls

    def test_production_machine_orders_ingress_writers_apps_cold_stores_and_held_proof(self):
        value, calls = self.machine()
        value.run()
        self.assertEqual(calls, ['preflight', 'drain:before-native', 'native', 'drain:after-native',
            'ortak-server', 'buzz-relay', 'drain:after-buzz-relay', 'ortak-management', 'ortak-worker',
            'drain:after-ortak-worker', 'controller', 'honcho_api', 'redis', 'minio', 'publish-held'])
        self.assertEqual(value.event.call_args.args[1]['status'], 'paused_verified')

    def test_every_failure_prefix_records_resume_and_does_not_publish_pause(self):
        for name in ['preflight', 'drain:before-native', *subject.PROCESS_ORDER,
                     'drain:after-buzz-relay', *subject.CONTAINER_ORDER]:
            value, calls = self.machine(name)
            with self.subTest(name=name), self.assertRaises(subject.Refused): value.run()
            self.assertNotIn('publish-held', calls)
            receipt = value.event.call_args.args[1]
            self.assertFalse(receipt['capture_authorized_by_this_attempt'])
            self.assertFalse(receipt['resume_plan']['automatic_resume'])

    def test_real_pause_not_written_if_final_held_drain_changes(self):
        @contextmanager
        def schema(_): yield {'backend_pid': 11}
        protected = {'format': 'ortak-confidential-journal-recovery/1', 'validator_sha256':
            hashlib.sha256(Path(selected_journal.confidential.__file__).read_bytes()).hexdigest()}
        for version, selection, admitted in ((76, None, True), (78, protected, True),
                                             (76, protected, False), (78, None, False)):
            with self.subTest(version=version, selected=selection is not None):
                value = self.bare()
                value.expected = {'containers': {'controller': {'image': 'frozen'}},
                    'journal_confidential': selection}
                value.output = Path('/unused')
                value.gate = SimpleNamespace()
                value.final_stopped = Mock(side_effect=[{}, {'generation': 1}, {'generation': 2}])
                saves = Mock()
                process = SimpleNamespace(stdin=Mock(), poll=Mock(return_value=None), wait=Mock(return_value=0))
                value.command.stop = Mock()
                validator = Mock(wraps=selected_journal.require_confidential_schema)
                bundle = Mock(wraps=selected_journal.lease_script)
                value.api = SimpleNamespace(recovery_lock_holder=recovery_lock_holder,
                    inventory=SimpleNamespace(MAIN_SCHEMA_VERSION=version),
                    selected_journal=SimpleNamespace(require_confidential_schema=validator, lease_script=bundle),
                    lease_args=Mock(return_value=['frozen-lease']), environment=lambda: {},
                    response=Mock(return_value={'status': 'held'}), held_schema=schema,
                    private_directory=lambda path, **_: path, save=saves)
                with patch.object(subject.subprocess, 'Popen', return_value=process) as launch:
                    if not admitted:
                        with self.assertRaisesRegex(selected_journal.Refused, 'journal_storage_selection_refused'):
                            value.publish_held_pause()
                        launch.assert_not_called()
                        bundle.assert_not_called()
                    else:
                        with self.assertRaisesRegex(subject.Refused, 'drain_generation_changed'):
                            value.publish_held_pause()
                        value.command.stop.assert_called_once_with(process)
                        bundle.assert_called_once_with(recovery_lock_holder, confidential_reviewed=selection)
                        # Inspect the actual generated Linux program without executing it:
                        # omitting the caller's opt-in must fail for the protected journal.
                        program = ast.parse(value.api.lease_args.call_args.args[-1])
                        flags = [node.value.value for node in program.body if isinstance(node, ast.Assign)
                            and any(isinstance(target, ast.Name) and target.id == 'RECOVERY_CONFIDENTIAL_REVIEWED'
                                    for target in node.targets)]
                        self.assertEqual(flags, [version == 78])
                validator.assert_called_once_with(selection, version)
                saves.assert_not_called()

    def test_default_cli_never_loads_or_mutates_selection(self):
        for argv in (['pause'], ['pause', '--execute-root-pause'], ['pause', '--host-oauth-enrollment-fenced']):
            with patch.object(subject.sys, 'argv', argv), patch.object(subject, 'load_selected') as load, \
                    patch.object(subject.argparse.ArgumentParser, 'error', side_effect=SystemExit(2)):
                with self.assertRaises(SystemExit): subject.main()
                load.assert_not_called()

    def test_public_source_scope_rejects_symlink_mode_drift_and_oversize(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'code.py'
            path.write_bytes(b'public'); path.chmod(0o500)
            self.assertEqual(subject.public_bytes(path, 0o500, 10), b'public')
            with self.assertRaises(subject.Refused): subject.public_bytes(path, 0o600, 10)
            with self.assertRaises(subject.Refused): subject.public_bytes(path, 0o500, 3)
            link = Path(directory) / 'link'; link.symlink_to(path)
            with self.assertRaises(subject.Refused): subject.public_bytes(link, 0o500, 10)


if __name__ == '__main__':
    unittest.main()
