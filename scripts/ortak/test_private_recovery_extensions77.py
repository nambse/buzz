"""Five production observer/catalog/caller regressions; no runtime activation.

These bind the real SQL-generation and response-validation seams. PostgreSQL
execution of the generated query remains a separate root-owned integration gate.
"""

import copy
from pathlib import Path
import re
import unittest

import private_recovery_obligations as subject
from test_private_recovery_obligations import Commands, COMPANY, metadata, witness

ROOT = Path(__file__).resolve().parents[2]
EXTENSION = subject.extensions77
MAIN = {
    'employee_memory_channel_authorities', 'employee_reviewed_memory_facts',
    'employee_reviewed_memory_operations', 'employee_reviewed_memory_targets',
    'employee_reviewed_memory_exports', 'employee_reviewed_memory_export_jobs',
    'employee_reviewed_memory_export_commands', 'employee_reviewed_memory_export_receipts',
    'run_employee_reviewed_memory_uses', 'encrypted_dm_selections', 'encrypted_dm_decrypt_jobs',
    'confidential_runs', 'confidential_run_payloads', 'confidential_dm_receipts',
    'confidential_run_dispatches', 'confidential_execution_leases', 'confidential_event_receipts',
    'confidential_reply_bundles', 'confidential_reply_outbox',
}
SOURCES = ('employee_reviewed_memory_candidate.sql', 'employee_reviewed_memory_runtime_candidate.sql',
           'encrypted_dm_jobs.sql', 'encrypted_dm_admission.sql', 'encrypted_dm_execution.sql')


class Extension77Tests(unittest.TestCase):
    def test_complete_explicit_inventory_binds_real_primary_keys_and_preserves61_to76(self):
        ddl = '\n'.join((ROOT / 'docs/ortak/sql' / name).read_text() for name in SOURCES)
        self.assertEqual(set(subject.table_keys(77)) - set(subject.table_keys(76)), MAIN)
        self.assertEqual(len(MAIN), 19)
        for table in MAIN:
            body = re.search(r'CREATE TABLE ' + table + r'\s*\((.*?)\n\);', ddl, re.S).group(1)
            compact = re.sub(r'\s+', '', body)
            self.assertIn('PRIMARYKEY(' + ','.join(EXTENSION.TABLE_KEYS[table]) + ')', compact)
            broken = metadata(77)
            del broken['tables']['public.' + table]
            with self.subTest(table=table), self.assertRaises(subject.Refused):
                subject.main_contract(broken)
            for version in range(61, 77):
                old = metadata(version)
                old['tables']['public.' + table] = 0
                with self.subTest(version=version, table=table), self.assertRaises(subject.Refused):
                    subject.main_contract(old)
        for version in range(61, 77):
            old = metadata(version)
            old['tables']['public.employee_memory_bindings'] = 0
            self.assertNotIn('employee_and_protected_memory', subject.main_contract(old))
            self.assertEqual(subject.observe(Commands(witness(version)), 'old', old, COMPANY, drained=True),
                             witness(version)['evidence'])
            self.assertNotIn('confidential_runs', subject.query(version, COMPANY))
        for version in (79, True, 78.0, '78'):
            with self.assertRaises(subject.Refused): subject.table_keys(version)
        #78 changes current event invalidation only: exact archived rows, keys,
        # byte/resource guards, and all history/drain counters remain77.
        self.assertEqual(subject.table_keys(78), subject.table_keys(77))
        self.assertEqual(subject.counters(78), subject.counters(77))
        self.assertEqual(subject.main_contract(metadata(78)),
                         subject.main_contract(metadata(77)) | {'schema_version': 78})
        prior = subject.query(77, COMPANY)
        current = subject.query(78, COMPANY)
        self.assertEqual(current, prior.replace("'schema_version',77,", "'schema_version',78,"))
        current_witness = witness(78)
        current_witness['evidence']['tables']['confidential_runs'] = [
            {'key': [COMPANY, '22222222-2222-2222-2222-222222222222'], 'row_sha256': 'c' * 64}]
        commands = Commands(current_witness)
        self.assertEqual(subject.observe(commands, 'fixture78', metadata(78), COMPANY, drained=True),
                         current_witness['evidence'])
        self.assertEqual(commands.calls[0][2]['sql'], current)
        current_witness['counters']['unsettled_protected_runs77'] = 1
        with self.assertRaisesRegex(subject.Refused, 'not_drained'):
            subject.observe(Commands(current_witness), 'fixture78', metadata(78), COMPANY, drained=True)
        unknown = metadata(77)
        unknown['tables']['public.confidential_future_secret'] = 0
        with self.assertRaises(subject.Refused): subject.main_contract(unknown)

    def test_real_observer_rejects_history_even_offline_but_never_renews_due_cleanup(self):
        for name in EXTENSION.HISTORY:
            for drained in (False, True):
                value = witness(77)
                value['counters'][name] = 1
                with self.subTest(counter=name, drained=drained), self.assertRaisesRegex(
                        subject.Refused, 'recovery77_history_inconsistent'):
                    subject.observe(Commands(value), 'fixture', metadata(77), COMPANY, drained=drained)
        value = witness(77)
        value['evidence']['tables']['confidential_runs'] = [
            {'key': [COMPANY, '22222222-2222-2222-2222-222222222222'], 'row_sha256': 'a' * 64}]
        expected = copy.deepcopy(value['evidence'])
        value['counters']['unsettled_employee_export_jobs77'] = 1
        actual = subject.verify_restore(Commands(value), 'offline', metadata(77), COMPANY, expected)
        self.assertEqual(actual['evidence'], expected)
        self.assertFalse(actual['automatic_activation'])
        with self.assertRaisesRegex(subject.Refused, 'not_drained'):
            subject.observe(Commands(value), 'fixture', metadata(77), COMPANY, drained=True)
        value['evidence']['tables']['confidential_runs'][0]['row_sha256'] = 'b' * 64
        with self.assertRaisesRegex(subject.Refused, 'obligations_changed'):
            subject.verify_restore(Commands(value), 'offline', metadata(77), COMPANY, expected)
        value['evidence']['tables']['confidential_runs'][0]['key'][1] = 'malformed'
        with self.assertRaisesRegex(subject.Refused, 'key_refused'):
            subject.observe(Commands(value), 'fixture', metadata(77), COMPANY, drained=False)

    def test_production_query_bounds_precede_decode_and_pins_are_time_independent(self):
        command = Commands(witness(77))
        subject.observe(command, 'fixture', metadata(77), COMPANY, drained=True)
        sql = command.calls[0][2]['sql']
        self.assertEqual(sql.count('BEGIN ISOLATION LEVEL REPEATABLE READ READ ONLY'), 1)
        self.assertLess(sql.index('protected recovery byte bound'), sql.index("SELECT jsonb_build_object('counters'"))
        self.assertIn('>8388608', sql)
        self.assertIn('NOT BETWEEN 1 AND 262144', sql)
        self.assertIn('>1024 THEN RAISE EXCEPTION', sql)
        self.assertIn("'row_sha256',encode(sha256(convert_to(to_jsonb(t)::text,'UTF8')),'hex')", sql)
        for name, keys in EXTENSION.TABLE_KEYS.items():
            self.assertIn(' FROM public.' + name + " t WHERE t.company_id='" + COMPANY + "'", sql)
            self.assertIn('jsonb_build_array(' + ','.join('t.' + key for key in keys) + ')', sql)
        history = '\n'.join(EXTENSION.HISTORY.values())
        for forbidden in ('clock_timestamp(', 'ortak_employee_memory_run_origin(',
                          'ortak_employee_snapshot_v5(', 'ortak_confidential_dm_current(',
                          'ortak_lock_', 'JOIN events ', 'JOIN office_inbox ', 'channel_members',
                          'active_revision_id', 'UPDATE ', 'DELETE ', 'INSERT '):
            self.assertNotIn(forbidden, history)
        for required in ('c.claim_worker=j.worker_id', 'a.claim_generation=j.claim_generation',
                         "p.envelope_bytes,c.identity_bytes,p.purpose,p.ordinal", "'base64'",
                         "e.state<>'stopped' OR EXISTS", "x.state='acknowledged'",
                         'u.source_authority_epoch<=src.epoch', 'u.destination_authority_epoch<=dst.epoch',
                         'u.consumption_epoch<=t.consumption_epoch',
                         "h.origin_type='human' AND h.origin_id=h.origin->>'requester_public_key'",
                         "encode(h.source_message_id,'hex')", 'h.spec_hash=sha256(h.spec_bytes)',
                         'ortak_snapshot_scratch_jsonb', 'jsonb_array_length(h.records)+selected.n::integer-1'):
            self.assertIn(required, history)
        for name, old_sql in subject.conversations.invariants(76).items():
            if name == 'invalid_conversation_snapshot_history76':
                expected = old_sql.replace('FROM parsed s LEFT JOIN runs',
                    "FROM (SELECT * FROM parsed WHERE wire->'version' IS DISTINCT FROM '5'::jsonb) s LEFT JOIN runs")
                self.assertEqual(subject.counters(77)[name], expected)
            else:
                self.assertEqual(subject.counters(77)[name], old_sql)
        drain = '\n'.join(EXTENSION.DRAIN.values())
        self.assertIn("e.state IN('complete','stopped')", drain)
        self.assertNotIn("e.state IN('complete','stopped','unconfirmed')", drain)
        self.assertNotIn('lease_expires_at<', drain)
        self.assertIn("j.state='verified' AND NOT EXISTS", drain)

    def test_honcho_all_seven_required_and_full_hash_proof_preserves_old_family(self):
        names = set(subject.HONCHO_BASE) | set(subject.HONCHO_REVIEWED)
        old = {'tables': dict.fromkeys(names, 0)}
        original = subject.honcho_contract(old)
        self.assertNotIn('employee_wire_family', original)
        selected = {'tables': dict.fromkeys(names | set(EXTENSION.HONCHO_KEYS), 0)}
        self.assertEqual(subject.stack_contract(metadata(77), selected)['honcho']['employee_wire_family'], 'reviewed-employee/1')
        with self.assertRaises(subject.Refused): subject.stack_contract(metadata(76), selected)
        with self.assertRaises(subject.Refused): subject.stack_contract(metadata(77), old)
        model = (ROOT / 'runtime/honcho-adapter/ortak_honcho/reviewed_employee_models.py').read_text()
        for name in EXTENSION.HONCHO_KEYS:
            self.assertIn('"' + name + '"', model)
            partial = copy.deepcopy(selected)
            del partial['tables'][name]
            with self.assertRaises(subject.Refused): subject.honcho_contract(partial)
        value = {'counters': dict.fromkeys(subject.HONCHO_COUNTERS | EXTENSION.HONCHO_HISTORY, 0),
                 'tables': {name: [] for name in EXTENSION.HONCHO_KEYS}}
        command = Commands(value)
        proof = subject.verify_honcho(command, 'honcho', selected)
        self.assertEqual(proof['employee_retained_evidence'], value['tables'])
        query = command.calls[0][2]['sql']
        self.assertIn('>1024', query)
        self.assertIn('ortak_employee_diagnostic_content', query)
        self.assertNotIn('clock_timestamp', query)
        for name in EXTENSION.HONCHO_HISTORY:
            changed = copy.deepcopy(value)
            changed['counters'][name] = 1
            with self.subTest(counter=name), self.assertRaises(subject.Refused):
                subject.verify_honcho(Commands(changed), 'honcho', selected)

    def test_deletion_manifest_and_actual_three_checkpoints_require_external_settlement(self):
        deletion = (ROOT / 'crates/buzz-db/src/store/deletion.rs').read_text()
        def inventory(name):
            body = re.search(r'pub const ' + name + r':.*?= &\[(.*?)\];', deletion, re.S).group(1)
            return re.findall(r'"([a-z_]+)"', body)
        expected = inventory('EXPECTED_SCOPED_TABLES')
        retained = inventory('RETAINED_SCOPED_TABLES')
        purged = inventory('PURGE_SCOPED_TABLES')
        self.assertTrue(MAIN <= set(expected) and MAIN <= set(retained))
        self.assertTrue(MAIN.isdisjoint(purged))
        self.assertEqual(expected, sorted(set(expected)))
        self.assertEqual(retained, sorted(set(retained)))
        call = 'extensions77::require_settled(&mut tx, token.community_id).await?;'
        self.assertEqual(deletion.count(call), 3)
        self.assertLess(deletion.index(call), deletion.index('quiescing_started_at = now()'))
        purge = deletion[deletion.index('pub async fn purge_postgres'):]
        self.assertLess(purge.index(call), purge.index('set_executor_gucs'))
        guard = (ROOT / 'crates/buzz-db/src/store/deletion/extensions77.rs').read_text()
        for required in ("a.remote_status='withdrawn'", 'a.erased_from_reviewed_store',
                         'a.request_hash=j.request_hash AND a.binding_hash=t.binding_hash',
                         'a.claim_worker=j.worker_id', "stop.state='acknowledged'",
                         "e.state IN('complete','stopped')", "d.state='delivered' OR e.state='stopped'",
                         "o.state='pending'", 'if isolation != "read committed"'):
            self.assertIn(required, guard)
        for forbidden in ('lease_expires_at<', 'clock_timestamp', 'attempts=0', 'UPDATE ', 'DELETE FROM '):
            self.assertNotIn(forbidden, guard)


if __name__ == '__main__':
    unittest.main()
