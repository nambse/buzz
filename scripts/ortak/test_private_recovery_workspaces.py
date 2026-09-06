"""Production74 database witness seams, without live inputs or provider calls."""
import copy
import json
from pathlib import Path
import unittest

import private_recovery_inventory as inventory
import private_recovery_obligations as subject
import private_recovery_workspaces as workspace
import register_private_recovery as register
from test_private_recovery_obligations import COMPANY, Commands, metadata, witness


class WorkspaceObligationTests(unittest.TestCase):
    def test_same_snapshot_projection_uses_one_bounded_production_query(self):
        value = witness(74)
        value['workspace_layout'] = {'company_id':COMPANY,'bindings':[],'runs':[],'readers':[]}
        commands = Commands(value)
        observed = subject.observe_workspace_layout(commands,'fixture',metadata(74),COMPANY)
        self.assertEqual(observed['database_evidence'],value['evidence'])
        self.assertEqual(len(commands.calls),1)
        sql=commands.calls[0][2]['sql']
        self.assertEqual(sql.count('BEGIN ISOLATION LEVEL REPEATABLE READ READ ONLY'),1)
        self.assertEqual(sql.count("SELECT jsonb_build_object('counters'"),1)
        self.assertIn("'workspace_layout',jsonb_build_object",sql)
        self.assertIn('convert_from(b.grant_bytes',sql)
        self.assertIn('UNION SELECT run_id,workspace_id FROM workspace_reader_executions',sql)
        self.assertEqual(commands.calls[0][2]['ceiling'],1024**2)
        with self.assertRaises(subject.Refused): subject.query(73,COMPANY,workspace_layout=True)

    def test_raw_projection_cannot_hide_readers_uses_or_company_and_is_bounded(self):
        value=witness(74)
        value['workspace_layout']={'company_id':COMPANY,'bindings':[],'runs':[],'readers':[]}
        for change in ('wrong_company','missing_reader','missing_use','overflow','unknown_field','undrained'):
            broken=copy.deepcopy(value)
            if change=='wrong_company': broken['workspace_layout']['company_id']='other'
            elif change in ('missing_reader','missing_use'):
                table='workspace_reader_executions' if change=='missing_reader' else 'run_workspace_uses'
                broken['evidence']['tables'][table]=[{'key':[COMPANY,'22222222-2222-2222-2222-222222222222'],'row_sha256':'e'*64}]
            elif change=='overflow': broken['workspace_layout']['bindings']=[{}]*33
            elif change=='unknown_field': broken['workspace_layout']['ambient_root']='/unselected'
            else: broken['counters']['uncontained_workspace_readers']=1
            with self.subTest(change=change),self.assertRaises(subject.Refused):
                subject.observe_workspace_layout(Commands(broken),'fixture',metadata(74),COMPANY)

    def test_exact_six_table_ledger_never_advances_current_selection(self):
        self.assertEqual(inventory.MAIN_SCHEMA_VERSION, 74)
        self.assertEqual(inventory.WORKSPACE_SELECTION['company_id'],inventory.COMPANY)
        self.assertEqual(len(workspace.TABLE_KEYS), 6)
        contract = subject.main_contract(metadata(74))
        self.assertFalse(contract['automatic_activation'])
        for table in workspace.TABLE_KEYS:
            self.assertEqual(contract['retained_table_ownership'][table], 'company_and_community')
            partial = metadata(74); del partial['tables']['public.' + table]
            with self.assertRaises(subject.Refused): subject.main_contract(partial)
            for version in (69, 73):
                forged = metadata(version); forged['tables']['public.' + table] = 0
                with self.assertRaises(subject.Refused): subject.main_contract(forged)
        unknown = metadata(74)
        unknown['migration_checksums'].append([75, 'a' * 96, True])
        with self.assertRaises(subject.Refused): subject.main_contract(unknown)
        unknown = metadata(74); unknown['tables']['public.workspace_future_effects'] = 0
        with self.assertRaisesRegex(subject.Refused, 'workspace_recovery_schema_review_required'):
            subject.main_contract(unknown)
        for flag in workspace.ACTIVATION_GATES:
            self.assertIn(flag, contract['activation_requires'])
            self.assertNotIn(flag, subject.main_contract(metadata(73))['activation_requires'])
        self.assertIn(Path(workspace.__file__).name, register.OPERATOR_FILES)

    def test_every_real_counter_is_required_and_blocks_capture(self):
        empty = witness(74)
        self.assertEqual(subject.observe(Commands(empty), 'fixture', metadata(74), COMPANY, drained=True),
                         empty['evidence'])
        for name in workspace.COUNTERS:
            busy = copy.deepcopy(empty); busy['counters'][name] = 1
            with self.subTest(name=name), self.assertRaisesRegex(subject.Refused, 'not_drained'):
                subject.observe(Commands(busy), 'fixture', metadata(74), COMPANY, drained=True)
            missing = copy.deepcopy(empty); del missing['counters'][name]
            with self.assertRaisesRegex(subject.Refused, 'counters_refused'):
                subject.observe(Commands(missing), 'fixture', metadata(74), COMPANY, drained=True)

    def test_terminal_history_is_exact_and_offline_restore_never_renews_it(self):
        value = witness(74)
        for table, keys in workspace.TABLE_KEYS.items():
            value['evidence']['tables'][table] = [{'key': [COMPANY] + ['11111111-1111-1111-1111-111111111112'] * (len(keys)-1),
                                                  'row_sha256': 'e' * 64}]
        expected = copy.deepcopy(value['evidence'])
        observed = subject.observe(Commands(value), 'fixture', metadata(74), COMPANY, drained=True)
        self.assertEqual(observed, expected)
        for table in workspace.TABLE_KEYS:
            changed = copy.deepcopy(value)
            changed['evidence']['tables'][table][0]['row_sha256'] = 'f' * 64
            with self.assertRaisesRegex(subject.Refused, 'obligations_changed'):
                subject.verify_restore(Commands(changed), 'fixture', metadata(74), COMPANY, expected)
        # Restore's only claim is exact archive equality; it does not claim new
        # eligibility, execute expired work, or replace a stop ACK with a timer.
        value['counters']['active_workspace_runs'] = 1
        proof = subject.verify_restore(Commands(value), 'fixture', metadata(74), COMPANY, expected)
        self.assertFalse(proof['automatic_activation'])
        self.assertIn('workspace_reader_containment_confirmed', proof['activation_requires'])

    def test_query_binds_full_bytes_original_attempt_and_failed_preparation_history(self):
        sql = subject.query(74, COMPANY)
        self.assertIn('REPEATABLE READ READ ONLY', sql)
        self.assertIn('to_jsonb(t)::text', sql)
        self.assertIn('b.grant_bytes<>convert_to(ortak_workspace_canonical', sql)
        self.assertIn('x.result_hash<>sha256(x.result_bytes)', sql)
        self.assertIn('e.owner_lease=x.lease_token', sql)
        self.assertIn('x.attempt_count<=a.attempt_count', sql)
        self.assertNotIn('a.lease_token=x.lease_token', sql)
        self.assertNotIn('ortak_run_workspace_current', sql)
        self.assertNotIn('ortak_workspace_profile_available', sql)
        self.assertNotIn('b.expires_at>clock_timestamp()', sql)
        self.assertNotIn('b.revoked_at IS NULL', sql)
        readers = workspace.COUNTERS['uncontained_workspace_readers']
        self.assertIn("e.state<>'stopped'", readers)
        self.assertNotIn('clock_timestamp', readers)
        parents = workspace.COUNTERS['invalid_workspace_parents']
        self.assertIn("e.request_key='prepare' AND EXISTS(SELECT 1 FROM outbox", parents)
        self.assertIn("u.workspace_id=e.workspace_id", parents)
        self.assertIn("a.state NOT IN ('delivered','interrupted')", workspace.COUNTERS['unsettled_workspace_actions'])
        for table in workspace.TABLE_KEYS:
            self.assertIn('FROM public.' + table + ' t', sql)


if __name__ == '__main__':
    unittest.main()
