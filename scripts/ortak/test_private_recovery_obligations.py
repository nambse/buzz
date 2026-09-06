"""Production recovery witness boundaries; no live database, process or provider access."""

import copy
import json
from pathlib import Path
import unittest
from unittest.mock import patch

import private_recovery_obligations as subject
import check_private_recovery_gate as gate
import prepare_private_recovery as prepare
import register_private_recovery as register
from test_prepare_private_recovery import observation

COMPANY = '11111111-1111-1111-1111-111111111111'


def metadata(version=69):
    return {'migration_checksums': [[v, 'a' * 96, True] for v in range(1, version + 1)],
            'tables': {'public.' + table: 0 for table in subject.table_keys(version)}}


def witness(version=69):
    return {'counters': dict.fromkeys(subject.counters(version), 0),
            'evidence': {'schema_version': version, 'company_id': COMPANY,
                         'tables': {name: [] for name in subject.table_keys(version)}}}


class Commands:
    def __init__(self, value):
        self.value, self.calls = value, []

    def psql(self, database):
        return [database]

    def run(self, label, args, **kwargs):
        self.calls.append((label, args, kwargs))
        return json.dumps(self.value).encode()


class ObligationTests(unittest.TestCase):
    def observe(self, value, *, drained=True):
        command = Commands(value)
        result = subject.observe(command, 'fixture', metadata(), COMPANY, drained=drained)
        self.assertIn('REPEATABLE READ READ ONLY', command.calls[0][2]['sql'])
        self.assertEqual(command.calls[0][2]['ceiling'], subject.MAX_BYTES)
        return result

    def test_versions_never_adopt_new_tables_or_partial_inventory(self):
        self.assertEqual(subject.main_contract(metadata(66))['retained_tables'], [])
        self.assertEqual(subject.main_contract(metadata(68))['retained_tables'], [subject.PROBE])
        for version in (60, 79, True, 73.0, '73', None):
            with self.assertRaises(subject.Refused): subject.table_keys(version)
        for table in subject.table_keys(69):
            current = metadata(); del current['tables']['public.' + table]
            with self.assertRaises(subject.Refused): subject.main_contract(current)
        old = metadata(66); old['tables']['public.' + subject.PROBE] = 0
        with self.assertRaises(subject.Refused): subject.main_contract(old)
        for table in ('conversation_memory_authorities', 'reviewed_memory_conversation_audiences'):
            old = metadata(74); old['tables']['public.' + table] = 0
            with self.subTest(table=table), self.assertRaises(subject.Refused):
                subject.main_contract(old)

    def test_reviewed_69_through_73_versions_have_exact_retained_ownership(self):
        old = subject.main_contract(metadata(69))
        self.assertEqual(set(old), {'schema_version', 'retained_tables', 'automatic_activation', 'activation_requires'})
        for version in range(70, 74):
            contract = subject.main_contract(metadata(version))
            ownership = contract['retained_table_ownership']
            self.assertEqual(ownership[subject.DECOMPOSITION], 'company')
            self.assertEqual(subject.table_keys(version)[subject.DECOMPOSITION], ('company_id', 'child_id'))
            self.assertEqual(subject.REVIEWED_USES in ownership, version >= 71)
            if version >= 71:
                self.assertEqual(ownership[subject.REVIEWED_USES], 'company_and_community')
                self.assertEqual(subject.table_keys(version)[subject.REVIEWED_USES], ('company_id', 'run_id', 'ordinal'))
            self.assertEqual(contract['historical_evidence'], 'preserve_exact_rows_without_renewing_authority')
        self.assertEqual(subject.table_keys(71), subject.table_keys(72))
        self.assertEqual(subject.table_keys(72), subject.table_keys(73))
        for version in range(69, 74):
            for missing in subject.table_keys(version):
                broken = metadata(version); del broken['tables']['public.' + missing]
                with self.subTest(version=version, missing=missing), self.assertRaises(subject.Refused):
                    subject.main_contract(broken)
            for future in subject.UNREVIEWED_WORKSPACE_TABLES:
                broken = metadata(version); broken['tables']['public.' + future] = 0
                with self.assertRaises(subject.Refused): subject.main_contract(broken)
        for version, future in ((69, subject.DECOMPOSITION), (70, subject.REVIEWED_USES)):
            broken = metadata(version); broken['tables']['public.' + future] = 0
            with self.assertRaises(subject.Refused): subject.main_contract(broken)

    def test_malformed_ledger_or_aliased_table_inventory_refuses(self):
        for ledger in (None, [], {}, [[73]], [[73, 'x', False]], [[True, 'x', True]],
                       [['73', 'x', True]], [[73, None, True]], [[73, '', True]],
                       [[74, 'x', True]], [[72, 'x', True], [71, 'x', True]],
                       [[73, 'x', True], [73, 'x', True]], [[73, 'x', 1]]):
            broken = metadata(73); broken['migration_checksums'] = ledger
            with self.subTest(ledger=ledger), self.assertRaises(subject.Refused): subject.main_contract(broken)
        for tables in (None, [], {'public.work_decomposition': True}, {None: 0}):
            broken = metadata(73); broken['tables'] = tables
            with self.assertRaises(subject.Refused): subject.main_contract(broken)
        broken = metadata(73); broken['tables'][subject.REVIEWED_USES] = 0
        with self.assertRaises(subject.Refused): subject.main_contract(broken)
        for change in ('gap', 'checksum', 'uppercase_checksum', 'incomplete'):
            broken = metadata(73)
            if change == 'gap': del broken['migration_checksums'][20]
            elif change == 'checksum': broken['migration_checksums'][20][1] = 'a' * 64
            elif change == 'uppercase_checksum': broken['migration_checksums'][20][1] = 'A' * 96
            else: broken['migration_checksums'] = broken['migration_checksums'][-1:]
            with self.subTest(change=change), self.assertRaises(subject.Refused): subject.main_contract(broken)

    def test_historical_reviewed_bytes_survive_current_authority_loss_and_offline_expiry(self):
        value = witness(73)
        value['evidence']['tables'][subject.REVIEWED_USES] = [
            {'key': [COMPANY, '22222222-2222-2222-2222-222222222222', 0], 'row_sha256': 'e' * 64}]
        for table in (subject.REVIEWED_USES, subject.DECOMPOSITION):
            if not value['evidence']['tables'][table]:
                value['evidence']['tables'][table] = [
                    {'key': [COMPANY, '33333333-3333-3333-3333-333333333333'], 'row_sha256': 'd' * 64}]
        expected = copy.deepcopy(value['evidence'])
        observed = subject.observe(Commands(value), 'fixture', metadata(73), COMPANY, drained=True)
        self.assertEqual(observed, expected)
        # Current fact opt-in/expiry is intentionally absent from this retained
        # evidence query. A missing or nonterminal parent run still refuses.
        sql = subject.query(73, COMPANY)
        self.assertIn("r.status NOT IN ('completed','failed','cancelled')", sql)
        self.assertIn('r.id IS NULL', sql)
        self.assertNotIn('ortak_run_reviewed_memory_current', sql)
        self.assertNotIn('ortak_reviewed_runtime_eligible', sql)
        self.assertNotIn('ortak_snapshot_scratch_jsonb', sql)
        value['counters']['active_reviewed_memory_runs'] = 1
        with self.assertRaisesRegex(subject.Refused, 'not_drained'):
            subject.observe(Commands(value), 'fixture', metadata(73), COMPANY, drained=True)
        restored = subject.verify_restore(Commands(value), 'fixture', metadata(73), COMPANY, expected)
        self.assertFalse(restored['automatic_activation'])
        for table in (subject.REVIEWED_USES, subject.DECOMPOSITION):
            changed = copy.deepcopy(value); changed['evidence']['tables'][table][0]['row_sha256'] = 'f' * 64
            with self.assertRaisesRegex(subject.Refused, 'obligations_changed'):
                subject.verify_restore(Commands(changed), 'fixture', metadata(73), COMPANY, expected)

    def test_any_uncontained_due_leased_failed_or_uncertain_counter_refuses(self):
        self.assertEqual(self.observe(witness()), witness()['evidence'])
        for name in subject.counters(69):
            current = witness(); current['counters'][name] = 1
            with self.subTest(counter=name), self.assertRaisesRegex(subject.Refused, 'not_drained'):
                self.observe(current)

    def test_inventory_requires_exact_company_keys_hashes_and_no_duplicate_rows(self):
        value = witness()
        row = {'key': [COMPANY, '22222222-2222-2222-2222-222222222222', 'withdraw'], 'row_sha256': 'a' * 64}
        value['evidence']['tables']['reviewed_memory_export_jobs'] = [row]
        self.assertEqual(self.observe(value), value['evidence'])
        for update in ({'key': ['other', 'fact', 'withdraw']}, {'row_sha256': 'invalid'}, {'key': [COMPANY]}):
            changed = copy.deepcopy(value)
            changed['evidence']['tables']['reviewed_memory_export_jobs'][0].update(update)
            with self.assertRaises(subject.Refused): self.observe(changed)
        value['evidence']['tables']['reviewed_memory_export_jobs'].append(row)
        with self.assertRaisesRegex(subject.Refused, 'duplicate'): self.observe(value)

    def test_missing_or_boolean_counters_do_not_turn_into_zero(self):
        for counter in ({}, dict.fromkeys(subject.counters(69), False), dict.fromkeys(subject.counters(69), -1)):
            value = witness(); value['counters'] = counter
            with self.assertRaises(subject.Refused): self.observe(value)

    def test_offline_expiry_never_runs_jobs_but_requires_identical_full_row_hash(self):
        value = witness()
        value['evidence']['tables']['reviewed_memory_export_jobs'] = [
            {'key': [COMPANY, 'fact', 'withdraw'], 'row_sha256': 'a' * 64}]
        expected = copy.deepcopy(value['evidence'])
        value['counters']['uncertain_or_due_export_jobs'] = 1  # Time advanced after a safe capture.
        result = subject.verify_restore(Commands(value), 'offline', metadata(), COMPANY, expected)
        self.assertFalse(result['automatic_activation'])
        self.assertIn('retained_withdrawal_expiry_catch_up', result['activation_requires'])
        value['evidence']['tables']['reviewed_memory_export_jobs'][0]['row_sha256'] = 'b' * 64
        with self.assertRaisesRegex(subject.Refused, 'obligations_changed'):
            subject.verify_restore(Commands(value), 'offline', metadata(), COMPANY, expected)

    def test_sql_fences_pending_lease_only_and_preserves_future_cleanup(self):
        sql = subject.query(69, COMPANY)
        self.assertIn("j.state='pending' AND (j.lease_token IS NOT NULL", sql)
        self.assertIn("j.action='publish' OR j.next_attempt_at<=clock_timestamp()", sql)
        self.assertIn("w.state='pending' AND w.lease_token IS NULL AND w.total_attempts=0", sql)
        self.assertIn("w.last_error_code IS NULL AND w.next_attempt_at>clock_timestamp()", sql)
        self.assertIn('j.total_attempts>0 OR j.last_error_code IS NOT NULL', sql)
        self.assertIn("state NOT IN ('succeeded','failed') OR contained_at IS NULL", sql)
        self.assertNotIn('deadline<', sql)
        self.assertIn('r.lease_token=j.lease_token AND r.total_attempts=j.total_attempts', sql)
        self.assertIn('r.community_id=j.community_id', sql)
        self.assertIn('r.erased_from_reviewed_store AND r.tombstone_at IS NOT NULL', sql)
        self.assertIn('to_jsonb(t)', sql)
        self.assertIn('t.actor_pubkey', sql)
        self.assertIn('recovery obligation bound', sql)
        self.assertNotIn('UPDATE ', sql)
        self.assertNotIn('DELETE ', sql)
        with self.assertRaises(subject.Refused): subject.query(69, COMPANY + "' OR true")

    def test_old_approved_preparation_cannot_gain_new_recovery_contract(self):
        old = observation()
        old_plan = prepare.plan_steps('fixture', old)
        current = copy.deepcopy(old); current['main_database'] = {**metadata(69), 'schema_sha256': 'new'}
        with self.assertRaisesRegex(subject.Refused, 'honcho_generation_missing'):
            prepare.plan_steps('fixture', current)
        current['honcho']['catalog']['tables'].update(dict.fromkeys(subject.HONCHO_REVIEWED, 0))
        new_plan = prepare.plan_steps('fixture', current)
        self.assertNotEqual(old_plan, new_plan)
        self.assertEqual(len(new_plan['recovery_contract']['main']['retained_tables']), 6)
        self.assertIn('private_recovery_obligations.py', register.OPERATOR_FILES)
        self.assertIn('obligations.observe', Path(gate.__file__).read_text())

    def test_main_gate_calls_semantic_guard_before_accepting_a_pause(self):
        current = gate.Gate.__new__(gate.Gate)
        current.output = Path('/fixture')
        current.command = type('Bound',(),{'deadline':float('inf')})()
        meta = {**metadata(), 'schema_sha256': 'frozen'}
        current.preparation = {'observation': {'main_database': meta}}
        class Main(Commands):
            def inspect(self): pass
            def metadata(self, *args): return meta
            def run(self, label, args, **kwargs):
                return b'{"application_clients":0}' if label == 'drain' else super().run(label, args, **kwargs)
        value = witness(); value['counters']['uncontained_runtime_probes'] = 1
        value['evidence']['company_id'] = gate.inventory.COMPANY
        with patch.object(gate, 'private_directory', side_effect=lambda path, **kwargs: path), \
            patch.object(gate, 'Commands', return_value=Main(value)), patch.object(gate, 'HonchoCommands') as honcho:
            with self.assertRaisesRegex(subject.Refused, 'not_drained'): current.drained_databases()
            honcho.assert_not_called()

    def test_honcho_four_tables_are_atomic_and_unknown_extensions_refuse(self):
        base = {'tables': dict.fromkeys(subject.HONCHO_BASE, 0)}
        self.assertIsNone(subject.honcho_contract(base)['reviewed_wire_family'])
        for missing in subject.HONCHO_REVIEWED:
            partial = {'tables': dict.fromkeys((*subject.HONCHO_BASE, *[t for t in subject.HONCHO_REVIEWED if t != missing]), 0)}
            with self.assertRaises(subject.Refused): subject.honcho_contract(partial)
        current = {'tables': dict.fromkeys((*subject.HONCHO_BASE, *subject.HONCHO_REVIEWED), 0)}
        self.assertEqual(subject.honcho_contract(current)['reviewed_wire_family'], 'reviewed-project/1')
        current['tables']['ortak_unreviewed'] = 0
        with self.assertRaises(subject.Refused): subject.honcho_contract(current)

    def test_honcho_semantics_refuse_text_hash_mismatch_or_lost_tombstone_receipts(self):
        meta = {'tables': dict.fromkeys((*subject.HONCHO_BASE, *subject.HONCHO_REVIEWED), 0)}
        value = dict.fromkeys(subject.HONCHO_COUNTERS, 0)
        subject.verify_honcho(Commands(value), 'fixture', meta)
        for name in value:
            bad = {**value, name: 1}
            with self.assertRaisesRegex(subject.Refused, 'lifecycle_inconsistent'):
                subject.verify_honcho(Commands(bad), 'fixture', meta)
        command = Commands(value)
        subject.verify_honcho(command, 'fixture', meta)
        sql = command.calls[0][2]['sql']
        self.assertIn("sha256(convert_to(c.content,'UTF8'))", sql)
        self.assertIn('USING(workspace_id,project_id,record_id)', sql)
        self.assertNotIn('expires_at', sql)  # Expired retained text is not an invented erasure ACK.


if __name__ == '__main__':
    unittest.main()
