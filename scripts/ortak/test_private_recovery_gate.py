"""Real file-lock/SQLite fixtures and production precondition tests; no live pause or Docker."""

import fcntl
import json
from pathlib import Path
import shutil
import sqlite3
import tempfile
import unittest
from unittest.mock import patch

import check_private_recovery_gate as subject
import recovery_lock_holder as holder


class LeaseTests(unittest.TestCase):
    def setUp(self):
        for name in ('DEPLOYMENT76_SELECTION','SCORER_SELECTION'):
            context=patch.object(subject.inventory,name,None);context.start();self.addCleanup(context.stop)
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name).resolve()
        self.root.chmod(0o700)
        for relative in holder.LOCKS:
            path = self.root / relative
            path.parent.mkdir(parents=True, exist_ok=True, mode=0o700)
            for parent in path.parents:
                if parent == self.root: break
                parent.chmod(0o700)
            path.touch(mode=0o600)

    def journal(self, status='completed', sequence=1):
        path = self.root / 'state/journal.sqlite'
        with sqlite3.connect(path) as db:
            db.executescript('CREATE TABLE runs (start_key TEXT, status TEXT,sequence INTEGER); CREATE TABLE events(start_key TEXT,sequence INTEGER);')
            db.execute('INSERT INTO runs VALUES (?,?,?)', ('fixture', status, sequence))
            db.execute('INSERT INTO events VALUES (?,?)', ('fixture', 1))
        path.chmod(0o600)
        return path

    def test_both_existing_locks_held_without_reading_credentials_and_released(self):
        secret = self.root / 'oauth/ada-private/oauth-state.json'
        secret.write_text('fixture-never-read'); secret.chmod(0o600)
        with holder.held_locks(self.root) as locks:
            self.assertEqual(len(locks), 2)
            for relative in holder.LOCKS:
                with (self.root / relative).open('rb') as other:
                    with self.assertRaises(BlockingIOError): fcntl.flock(other, fcntl.LOCK_EX | fcntl.LOCK_NB)
        for relative in holder.LOCKS:
            with (self.root / relative).open('rb') as other:
                fcntl.flock(other, fcntl.LOCK_EX | fcntl.LOCK_NB)
        self.assertEqual(secret.read_text(), 'fixture-never-read')

    def test_busy_or_replaced_lock_is_refused(self):
        path = self.root / holder.LOCKS[-1]
        with path.open('rb') as other:
            fcntl.flock(other, fcntl.LOCK_EX | fcntl.LOCK_NB)
            with self.assertRaises(BlockingIOError), holder.held_locks(self.root): pass
        with self.assertRaises(ValueError):
            with holder.held_locks(self.root):
                path.unlink(); path.touch(mode=0o600)

    def test_journal_read_does_not_initialize_mutate_or_accept_pending_work(self):
        path = self.journal()
        before = path.read_bytes()
        self.assertEqual(holder.journal_status(self.root), {'runs': 1, 'nonterminal': 0, 'invalid_cursors': 0})
        self.assertEqual(path.read_bytes(), before)
        with sqlite3.connect(path) as db: db.execute("UPDATE runs SET status='cancelling'")
        with self.assertRaises(ValueError): holder.journal_status(self.root)

    def test_cold_wal_without_shm_is_read_from_working_copy_without_source_sidecars(self):
        source = self.root / 'writer.sqlite'
        writer = sqlite3.connect(source)
        self.addCleanup(writer.close)
        writer.execute('PRAGMA journal_mode=WAL')
        writer.executescript("CREATE TABLE runs(start_key TEXT,status TEXT,sequence INTEGER);CREATE TABLE events(start_key TEXT,sequence INTEGER);INSERT INTO runs VALUES('fixture','completed',1);INSERT INTO events VALUES('fixture',1);")
        writer.commit()
        path = self.root / 'state/journal.sqlite'
        for old, new in [(source, path), (Path(str(source) + '-wal'), Path(str(path) + '-wal'))]:
            shutil.copyfile(old, new); new.chmod(0o600)
        before = {p.name: p.read_bytes() for p in path.parent.iterdir()}
        self.assertEqual(holder.journal_status(self.root), {'runs': 1, 'nonterminal': 0, 'invalid_cursors': 0})
        self.assertFalse(Path(str(path) + '-shm').exists())
        self.assertEqual({p.name: p.read_bytes() for p in path.parent.iterdir()}, before)
        with sqlite3.connect(path) as db: db.execute("UPDATE runs SET status='completed',sequence=2")
        with self.assertRaises(ValueError): holder.journal_status(self.root)

    def test_running_source_refuses_before_creating_any_lease_helper(self):
        class LiveGate:
            def __init__(self, output, registry): pass
            def stopped_owners(self): raise subject.Refused('private_native_writer_still_running')
        with patch.object(subject.subprocess, 'Popen') as launch, patch.object(subject, 'root_pause_receipt', return_value={}):
            with self.assertRaises(subject.Refused), subject.held_barrier(self.root, {}, pause_receipt=self.root / 'unused', gate_type=LiveGate): pass
            launch.assert_not_called()

    def test_running_management_is_a_writer_even_if_all_other_owners_are_stopped(self):
        names = subject.inventory.NATIVE_WRITERS
        gate = subject.Gate.__new__(subject.Gate)
        gate.registry = {'owners': {name: {} for name in names}}
        gate.preparation = {'observation': {'native_processes': {name: {} for name in names}, 'files': {}, 'native_ingress': {}}}
        calls = []
        class Inspector:
            def run(self, args, **kwargs):
                calls.append(args)
                if args[0] == '/usr/sbin/lsof': return ('n' + str(subject.inventory.STATE)).encode()
                return b'["123"]' if args[-1] == 'ortak-management' else b'[]'
        gate.inspector = Inspector()
        with patch.object(subject, 'files', return_value={}), patch.object(subject.native_ingress, 'require_stopped'):
            with self.assertRaisesRegex(subject.Refused, 'writer_still_running'): gate.stopped_owners()
        self.assertTrue(any(args[-1] == 'ortak-management' for args in calls))

    def test_old_registry_cannot_omit_new_writer_and_management_work_queues_are_drained(self):
        operation = self.root / 'recovery-operations' / ('a' * 32)
        row = {'format': subject.FORMAT, 'status': 'registered', 'operation_id': operation.name,
            'owners': {name: {} for name in subject.inventory.NATIVE_WRITERS[:-1]},
            'resume_recipes': {}, 'source_code': {}}
        row['registry_sha256'] = subject.sha(row)
        with patch.object(subject.inventory, 'STATE', self.root), \
            patch.object(subject.inventory, 'public_json', return_value=(row, {})):
            with self.assertRaisesRegex(subject.Refused, 'inventory_incomplete'):
                subject.load_registry(operation / 'owners.json')
        self.assertIn("FROM employee_management_commands WHERE company_id=", subject.MAIN_DRAIN_SQL)
        self.assertIn("status IN ('pending','running')", subject.MAIN_DRAIN_SQL)
        self.assertIn("FROM runtime_work_outputs WHERE company_id=", subject.MAIN_DRAIN_SQL)

    def test_lease_is_same_linux_image_but_no_network_socket_executor_or_secret_arguments(self):
        class Command:
            def docker(self, *args): return list(args)
        args = subject.lease_args(Command(), 'ortak-recovery-lease-fixture', 'sha256:' + 'a' * 64, 'print("fixture")')
        self.assertEqual(args[args.index('--network') + 1], 'none')
        self.assertIn('--read-only', args)
        self.assertEqual(args[args.index('--entrypoint') + 1], '/usr/local/bin/python')
        self.assertNotIn('/var/run/docker.sock', str(args))
        self.assertNotIn('oauth-state.json', str(args))
        self.assertNotIn('--rm', args)
        self.assertEqual(sum(value == '--mount' for value in args), 2)

    def test_root_pause_attestation_bound_to_exact_registry_not_an_automatic_boolean(self):
        operation = self.root / 'recovery-operations' / ('a' * 32)
        operation.mkdir(parents=True, mode=0o700)
        registry = {'operation_id': operation.name, 'registry_sha256': 'b' * 64}
        value = {'format': 'ortak-private-recovery-pause/1', 'owners_sha256': 'b' * 64,
            'host_oauth_enrollment_fenced': True, 'root_coordinated_pause': True, 'resume_under_root_control': True}
        with patch.object(subject.inventory, 'STATE', self.root), patch.object(subject.inventory, 'public_json', return_value=(value, {})):
            self.assertEqual(subject.root_pause_receipt(operation / 'pause.json', registry), {})
            value['owners_sha256'] = 'old'
            with self.assertRaises(subject.Refused): subject.root_pause_receipt(operation / 'pause.json', registry)
            value['owners_sha256'] = 'b' * 64; value['host_oauth_enrollment_fenced'] = False
            with self.assertRaises(subject.Refused): subject.root_pause_receipt(operation / 'pause.json', registry)


if __name__ == '__main__':
    unittest.main()
