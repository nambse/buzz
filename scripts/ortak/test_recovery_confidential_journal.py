"""Real cold-copy query seam plus production journal DDL/wire; no Docker or keys.

All deliberate corruption changes occur only in disposable fixture copies after
dropping the corresponding immutable guard. They never change production SQL.
"""

import ast
import hashlib
import importlib.util
import json
from pathlib import Path
import sqlite3
import tempfile
import unittest
from unittest.mock import patch

import recovery_confidential_journal as subject
import recovery_lock_holder as holder
import private_recovery_journal as selected
import restore_private_recovery as restore
import private_recovery_payloads as payload

REPO = Path(__file__).resolve().parents[2]
BRIDGE = REPO / 'runtime/hermes-bridge/ortak_hermes_bridge'
VECTOR = REPO / 'crates/ortak-control/src/confidential/vector.json'


def production_base_schema():
    """Extract literal production DDL, without constructing the application Journal."""
    tree = ast.parse((BRIDGE / 'journal.py').read_text())
    roots = [node.value for node in ast.walk(tree) if isinstance(node, ast.Constant)
        and isinstance(node.value, str) and 'CREATE TABLE IF NOT EXISTS runs (' in node.value]
    assert len(roots) == 1
    tree = ast.parse((BRIDGE / 'journal_tools.py').read_text())
    tools = [ast.literal_eval(node.value) for node in tree.body if isinstance(node, ast.Assign)
        and any(isinstance(target, ast.Name) and target.id == 'SCHEMA' for target in node.targets)]
    assert len(tools) == 1
    return roots[0] + tools[0]


class ConfidentialJournalTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name).resolve()
        self.path = self.root / 'journal.sqlite'
        self.vector = json.loads(VECTOR.read_bytes())
        self.identity = self.vector['identity']
        self.key = 'ortak-run:' + self.identity['company_id'] + ':' + self.identity['run_id']
        spec = importlib.util.spec_from_file_location('fixture_confidential_wire', BRIDGE / 'confidential/wire.py')
        self.wire = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(self.wire)
        with sqlite3.connect(self.path) as db:
            db.executescript(production_base_schema())
            db.execute("INSERT INTO runs VALUES('ordinary','old-fingerprint','completed','2026-01-01T00:00:00Z',1)")
            db.execute("INSERT INTO events VALUES('ordinary',1,'2026-01-01T00:00:00Z','{}')")

    def populate(self, *, failed=False):
        snapshot = self.vector['expected']['envelope_utf8'].encode()
        identity_bytes = self.vector['expected']['identity_utf8'].encode()
        # Shared externally pinned snapshot vector and actual production public
        # event framing. The fixture does not claim to authenticate event tags.
        identity = self.wire.ValidatedIdentity(identity_bytes)
        with sqlite3.connect(self.path) as db:
            db.executescript((BRIDGE / 'confidential/schema.sql').read_text())
            db.execute('INSERT INTO runs VALUES(?,?,?,?,0)',
                (self.key, hashlib.sha256(snapshot).hexdigest(), 'accepted', '2026-01-01T00:00:00Z'))
            db.execute('INSERT INTO confidential_runs VALUES(?,?,?,?,?,?)',
                (self.key, 'ortak-confidential-journal/1', identity_bytes, snapshot,
                 self.identity['key_id'], bytes.fromhex(self.vector['nonce_hex'])))
            if failed:
                db.execute('INSERT INTO confidential_status VALUES(?,?,?)',
                    (self.key, 'executor_interrupted', '2026-01-01T00:00:01Z'))
                db.execute("UPDATE runs SET status='failed' WHERE start_key=?", (self.key,))
                return
            db.execute("UPDATE runs SET status='running' WHERE start_key=?", (self.key,))
            for ordinal in (1, 2):
                header = self.wire.PayloadHeader(identity, 'runtime_event', ordinal, 0)
                nonce = bytes([ordinal]) * 12
                event = self.wire.ConfidentialEnvelope(header, nonce, bytes(16))
                db.execute('INSERT INTO confidential_events VALUES(?,?,?,?,?,?)',
                    (self.key, ordinal, '2026-01-01T00:00:01Z', event.canonical_bytes,
                     self.identity['key_id'], nonce))
            db.execute("UPDATE runs SET status='completed' WHERE start_key=?", (self.key,))

    def proof(self, path=None):
        return holder.staged_journal_status(path or self.path, confidential_reviewed=True)

    def copy(self, name):
        target = self.root / (name + '.sqlite')
        with sqlite3.connect(self.path) as source, sqlite3.connect(target) as destination:
            source.backup(destination)
        return target

    def test_extension_is_explicit_all_three_or_none_and_legacy_result_is_exact(self):
        legacy = holder.staged_journal_status(self.path)
        self.assertEqual(legacy['runs'], 1)
        self.assertEqual(legacy['invalid_cursors'], 0)
        self.assertNotIn('confidential', legacy)
        with self.assertRaises(ValueError):
            self.proof()
        self.populate()
        with self.assertRaises(ValueError):
            holder.staged_journal_status(self.path)
        with self.assertRaises(ValueError):
            holder.staged_journal_status(self.path, confidential_reviewed=1)
        partial = self.copy('partial')
        with sqlite3.connect(partial) as db:
            db.execute('DROP TABLE confidential_status')
        with self.assertRaises(ValueError):
            self.proof(partial)
        unknown = self.copy('unknown')
        with sqlite3.connect(unknown) as db:
            db.execute('CREATE TABLE confidential_future(secret BLOB)')
        with self.assertRaises(ValueError):
            self.proof(unknown)

    def test_mixed_modes_and_cold_reopen_preserve_full_ciphertext_row_hashes(self):
        legacy = holder.staged_journal_status(self.path)
        self.populate()
        before = self.path.read_bytes()
        observed = self.proof()
        self.assertEqual(self.path.read_bytes(), before)
        self.assertEqual(observed, self.proof(self.copy('restored')))
        without_extension = {k: v for k, v in observed.items() if k != 'confidential'}
        self.assertEqual(without_extension, {**legacy, 'runs': 2})
        proof = observed['confidential']
        self.assertEqual(proof['tables'], {'confidential_runs': 1, 'confidential_events': 2, 'confidential_status': 0})
        self.assertEqual(proof['cryptographic_authentication'], 'not_performed')
        self.assertFalse(proof['automatic_activation'])
        serialized = json.dumps(observed)
        for private in (self.key, self.identity['employee_id'], 'ICEiIyQlJicoKSor', 'vnlmneyVn/'):
            self.assertNotIn(private, serialized)
        changed = self.copy('changed')
        with sqlite3.connect(changed) as db:
            db.execute('DROP TRIGGER confidential_event_immutable')
            db.execute("UPDATE confidential_events SET occurred_at='2026-01-01T00:00:02Z' WHERE sequence=2")
        self.assertNotEqual(proof['logical_rows_sha256'], self.proof(changed)['confidential']['logical_rows_sha256'])

    def test_corrupt_cursors_mixed_plaintext_nonce_and_parent_fingerprint_refuse(self):
        self.populate()
        mutations = {
            'cursor': "DROP TRIGGER confidential_registry_guard; UPDATE runs SET sequence=3 WHERE start_key<>'ordinary';",
            'gap': 'DROP TRIGGER confidential_event_retained; DELETE FROM confidential_events WHERE sequence=1;',
            'nonce': 'DROP TRIGGER confidential_event_immutable; UPDATE confidential_events SET nonce=zeroblob(12) WHERE sequence=1;',
            'fingerprint': "DROP TRIGGER confidential_registry_guard; UPDATE runs SET fingerprint='changed' WHERE start_key<>'ordinary';",
            'plaintext': "DROP TRIGGER ordinary_event_mode_guard; INSERT INTO events SELECT start_key,1,'2026-01-01T00:00:00Z','private canary' FROM confidential_runs;",
            'blob_type': "DROP TRIGGER confidential_event_immutable; PRAGMA ignore_check_constraints=ON; UPDATE confidential_events SET envelope='private canary' WHERE sequence=1;",
        }
        for name, sql in mutations.items():
            with self.subTest(name=name):
                path = self.copy(name)
                with sqlite3.connect(path) as db:
                    db.executescript(sql)
                with self.assertRaisesRegex(ValueError, 'recovery_') as caught:
                    self.proof(path)
                self.assertNotIn('private canary', str(caught.exception))

    def test_keyless_failed_zero_cursor_requires_closed_status_and_terminal_run(self):
        self.populate(failed=True)
        self.assertEqual(self.proof()['confidential']['tables']['confidential_events'], 0)
        missing = self.copy('missing-status')
        with sqlite3.connect(missing) as db:
            db.execute('DROP TRIGGER confidential_status_retained')
            db.execute('DELETE FROM confidential_status')
        with self.assertRaises(ValueError):
            self.proof(missing)
        active = self.copy('active')
        with sqlite3.connect(active) as db:
            db.execute('DROP TRIGGER confidential_registry_guard')
            db.execute("UPDATE runs SET status='cancelling' WHERE start_key<>'ordinary'")
        with self.assertRaises(ValueError):
            self.proof(active)

    def test_aggregate_ciphertext_bound_precedes_json_decoding(self):
        self.populate()
        with patch.object(subject, 'MAX_BYTES', 1), patch.object(subject, 'object_bytes', side_effect=AssertionError('decoded before bound')):
            with self.assertRaisesRegex(ValueError, 'recovery_confidential_journal_refused'):
                self.proof()

    def test_source_bound_selection_is_carried_into_embedded_lease_and_refuses_wrong_ledger(self):
        import sys
        choice={'format':'ortak-confidential-journal-recovery/1',
            'validator_sha256':hashlib.sha256(Path(subject.__file__).read_bytes()).hexdigest()}
        self.assertEqual(selected.require_confidential_schema(choice,77),choice)
        self.assertEqual(selected.require_confidential_schema(choice,78),choice)
        for value,ledger in [(choice,76),(None,77),(None,78),(choice,79),(choice,78.0),(choice,True),({**choice,'validator_sha256':'0'*64},77)]:
            with self.assertRaises(selected.Refused):selected.require_confidential_schema(value,ledger)
        self.populate()
        namespace={'__name__':'reviewed_lease_fixture'}
        with patch.dict(sys.modules):
            sys.modules.pop('recovery_confidential_journal',None)
            exec(compile(selected.lease_script(holder,confidential_reviewed=choice),'<lease>','exec'),namespace)
            self.assertTrue(namespace['RECOVERY_CONFIDENTIAL_REVIEWED'])
            self.assertEqual(namespace['staged_journal_status'](self.path,confidential_reviewed=True),self.proof())

    def test_sealed_selection_and_full_ciphertext_proof_bind_actual_offline_component_restore(self):
        self.populate()
        choice={'format':'ortak-confidential-journal-recovery/1',
            'validator_sha256':hashlib.sha256(Path(subject.__file__).read_bytes()).hexdigest()}
        self.path.chmod(0o600)
        backup=self.root/'backup';backup.mkdir(mode=0o700)
        component=payload.copy_file(self.path,backup/'journal.sqlite',64*1024**2)
        expected=restore.journal(backup/'journal.sqlite',confidential_reviewed=choice)
        component.update(confidential_selection=choice,confidential_proof=expected,cold_companions=[])
        target=self.root/'restored';target.mkdir(mode=0o700)
        proof=restore.restore_journal_component(backup,component,target,confidential_reviewed=choice)
        self.assertEqual({k:v for k,v in proof.items() if k!='cold_companions'},expected)
        self.assertEqual((target/'journal.sqlite').read_bytes(),(backup/'journal.sqlite').read_bytes())
        for mutation in ({**component,'confidential_selection':None},
                         {**component,'confidential_proof':{}}):
            with self.assertRaises(selected.Refused):
                restore.restore_journal_component(backup,mutation,self.root/'unused',confidential_reviewed=choice)


if __name__ == '__main__':
    unittest.main()
