"""G75/76 production metadata/query boundaries; no database or runtime execution."""

import copy
import hashlib
import json
from pathlib import Path
import re
import unittest

import private_recovery_obligations as subject
from test_private_recovery_obligations import Commands, COMPANY, metadata, witness

AUTHORITY = 'conversation_memory_authorities'
AUDIENCE = 'reviewed_memory_conversation_audiences'
PROJECT = '22222222-2222-2222-2222-222222222222'
CHANNEL = '33333333-3333-3333-3333-333333333333'
FACT = '44444444-4444-4444-4444-444444444444'
EXPECTED_KEYS = {AUTHORITY: ('company_id', 'project_id', 'channel_id'),
                 AUDIENCE: ('company_id', 'fact_id')}


def observed75():
    """Independent transport rows: complete source rows are represented by hashes."""
    value = witness(75)
    value['evidence']['tables'][AUTHORITY] = [
        {'key': [COMPANY, PROJECT, CHANNEL], 'row_sha256': 'a' * 64}]
    value['evidence']['tables'][AUDIENCE] = [
        {'key': [COMPANY, FACT], 'row_sha256': 'b' * 64}]
    return value


class ConversationRecoveryTests(unittest.TestCase):
    def test75_inventory_matches_immutable_physical_keys_and76_adds_no_tables(self):
        self.assertEqual({name: subject.table_keys(75)[name] for name in EXPECTED_KEYS}, EXPECTED_KEYS)
        self.assertEqual(set(subject.table_keys(75)) - set(subject.table_keys(74)), set(EXPECTED_KEYS))
        self.assertEqual(subject.table_keys(76), subject.table_keys(75))
        for version in (78, True, 75.0, '75'):
            with self.subTest(version=version), self.assertRaises(subject.Refused):
                subject.table_keys(version)
        pending = metadata(77)
        pending['migration_checksums'].append([78, 'b' * 96, True])
        with self.assertRaises(subject.Refused): subject.main_contract(pending)
        for table in EXPECTED_KEYS:
            for old_version in range(61, 75):
                old = metadata(old_version)
                old['tables']['public.' + table] = 0
                with self.subTest(version=old_version, table=table), self.assertRaises(subject.Refused):
                    subject.main_contract(old)
            partial = metadata(75)
            del partial['tables']['public.' + table]
            with self.assertRaises(subject.Refused): subject.main_contract(partial)
        unknown = metadata(75)
        unknown['tables']['public.conversation_future_authority'] = 0
        with self.assertRaisesRegex(subject.Refused, 'conversation_recovery_schema_review_required'):
            subject.main_contract(unknown)
        source = (Path(__file__).resolve().parents[2] / 'migrations/0075_ortak_conversation_memory.sql').read_text()
        for table, keys in EXPECTED_KEYS.items():
            body = re.search(r'CREATE TABLE ' + table + r' \((.*?)\n\);', source, re.S).group(1)
            compact = re.sub(r'\s+', '', body)
            self.assertIn('PRIMARYKEY(' + ','.join(keys) + ')', compact)
        for column in ('audience_kind', 'conversation_consumption_enabled', 'conversation_channel_id',
                       'conversation_consumption_epoch', 'conversation_audience_hash', 'conversation_authority_epoch'):
            self.assertIn('ADD COLUMN ' + column, source)

    def test_older_contracts_keep_their_tables_gates_and_read_only_query_shape(self):
        for version in range(61, 75):
            contract = subject.main_contract(metadata(version))
            self.assertNotIn('conversation_memory', contract)
            self.assertNotIn('conversation_current_source_and_epochs_revalidated', contract['activation_requires'])
            self.assertTrue(set(EXPECTED_KEYS).isdisjoint(contract['retained_tables']))
            sql = subject.query(version, COMPANY)
            self.assertNotIn('conversation_', sql)
            self.assertEqual(subject.observe(Commands(witness(version)), 'fixture', metadata(version),
                                             COMPANY, drained=True), witness(version)['evidence'])
        contract = subject.main_contract(metadata(75))
        self.assertFalse(contract['automatic_activation'])
        self.assertEqual(contract['conversation_memory']['runtime_publication'], 'not_admitted_by_schema75')
        self.assertEqual({contract['retained_table_ownership'][name] for name in EXPECTED_KEYS},
                         {'company_and_community'})

    def test_real_query_aggregates_every_full_row_under_one_bound_before_returning_hashes(self):
        value = observed75()
        command = Commands(value)
        expected = subject.observe(command, 'fixture', metadata(75), COMPANY, drained=True)
        self.assertEqual(expected, value['evidence'])
        sql = command.calls[0][2]['sql']
        self.assertIn('BEGIN ISOLATION LEVEL REPEATABLE READ READ ONLY', sql)
        self.assertEqual(command.calls[0][2]['ceiling'], 384 * 1024)
        self.assertIn('>1024 THEN RAISE EXCEPTION', sql)
        self.assertLess(sql.index('DO $$BEGIN IF'), sql.index("SELECT jsonb_build_object('counters'"))
        for table, keys in EXPECTED_KEYS.items():
            self.assertIn(' FROM public.' + table + " t WHERE t.company_id='" + COMPANY + "'", sql)
            self.assertIn('jsonb_build_array(' + ','.join('t.' + key for key in keys) + ')', sql)
        self.assertIn("'row_sha256',encode(sha256(convert_to(to_jsonb(t)::text,'UTF8')),'hex')", sql)
        self.assertNotIn(' LIMIT ', sql)
        self.assertTrue(sql.endswith('ROLLBACK;'))
        self.assertIn('conversation_current_source_and_epochs_revalidated',
                      subject.verify_restore(Commands(value), 'offline', metadata(75), COMPANY, expected)['activation_requires'])

    def test_source_identity_is_historical_and_canonical_bytes_do_not_query_current_authority(self):
        sql = subject.query(75, COMPANY)
        for forbidden in ('ortak_conversation_source_observation(', 'ortak_conversation_scope_current(',
                          'ortak_run_reviewed_memory_current(', 'ortak_reviewed_runtime_eligible(',
                          'JOIN events ', 'JOIN office_inbox ', 'channel_members', 'project_access_grants',
                          'active_revision_id', 'UPDATE ', 'DELETE ', 'INSERT '):
            self.assertNotIn(forbidden, sql)
        for required in ('a.audience_bytes IS DISTINCT FROM convert_to(ortak_conversation_json75(wire.audience)',
                         'a.audience_hash IS DISTINCT FROM sha256(a.audience_bytes)',
                         "'format','ortak-reviewed-conversation-source/1'",
                         "'format','ortak-reviewed-conversation-provenance/1'",
                         'f.source_message_id=a.source_event_id', 'f.source_artifact_id IS NULL',
                         "receipt.action='promote' AND receipt.result_version=1",
                         'u.conversation_authority_epoch<=scope.epoch',
                         'u.conversation_consumption_epoch<=t.conversation_consumption_epoch',
                         'u.consumption_epoch=0', 'u.conversation_audience_hash IS NULL',
                         'u.conversation_authority_epoch IS NULL', 'u.conversation_consumption_epoch IS NULL'):
            self.assertIn(required, sql)
        self.assertNotIn('u.conversation_authority_epoch=scope.epoch', sql)
        self.assertNotIn('u.conversation_consumption_epoch=t.conversation_consumption_epoch', sql)
        # Target opt-in may remain true on75, including after TTL/permission loss.
        target = subject.conversations.INVARIANTS['invalid_conversation_target_pins']
        self.assertNotIn('clock_timestamp', target)
        self.assertNotIn('valid_until', target)
        self.assertNotIn('runtime_consumption_enabled', target)

    def test_structural_failure_cannot_be_ignored_during_offline_restore(self):
        for name in subject.conversations.INVARIANTS:
            for drained in (False, True):
                value = observed75()
                value['counters'][name] = 1
                with self.subTest(counter=name, drained=drained), self.assertRaisesRegex(
                        subject.Refused, 'conversation_recovery_history_inconsistent'):
                    subject.observe(Commands(value), 'fixture', metadata(75), COMPANY, drained=drained)
        self.assertIn('unsupported_conversation_execution75', subject.counters(75))

    def test_offline_expiry_retains_bytes_without_remote_ack_or_epoch_renewal(self):
        value = observed75()
        # Full-row hash includes opaque canonical bytes, epoch, reason and timestamps.
        original = {'epoch': 3, 'last_change_reason': 'membership_changed', 'audience_bytes': 'retained'}
        value['evidence']['tables'][AUTHORITY][0]['row_sha256'] = hashlib.sha256(
            json.dumps(original, sort_keys=True).encode()).hexdigest()
        expected = copy.deepcopy(value['evidence'])
        value['counters']['uncertain_or_due_export_jobs'] = 1
        restored = subject.verify_restore(Commands(value), 'offline', metadata(75), COMPANY, expected)
        self.assertFalse(restored['automatic_activation'])
        self.assertEqual(restored['evidence'], expected)
        with self.assertRaisesRegex(subject.Refused, 'not_drained'):
            subject.observe(Commands(value), 'fixture', metadata(75), COMPANY, drained=True)
        for table in (AUTHORITY, AUDIENCE):
            changed = copy.deepcopy(value)
            changed['evidence']['tables'][table][0]['row_sha256'] = 'c' * 64
            with self.subTest(table=table), self.assertRaisesRegex(subject.Refused, 'obligations_changed'):
                subject.verify_restore(Commands(changed), 'offline', metadata(75), COMPANY, expected)

    def test_new_primary_keys_and_scope_bound_are_not_inferred_from_table_counts(self):
        for table in EXPECTED_KEYS:
            for key in ([COMPANY], ['other', FACT], [COMPANY, 'not-a-uuid'],
                        [COMPANY, '00000000-0000-0000-0000-000000000000']):
                value = observed75()
                value['evidence']['tables'][table][0]['key'] = key
                with self.subTest(table=table, key=key), self.assertRaises(subject.Refused):
                    subject.observe(Commands(value), 'fixture', metadata(75), COMPANY, drained=True)
            value = observed75()
            value['evidence']['tables'][table] *= 2
            with self.assertRaisesRegex(subject.Refused, 'duplicate'):
                subject.observe(Commands(value), 'fixture', metadata(75), COMPANY, drained=True)
        value = observed75()
        value['evidence']['tables'][AUTHORITY] = [
            {'key': [COMPANY, PROJECT, f'33333333-3333-3333-3333-{i:012x}'], 'row_sha256': 'a' * 64}
            for i in range(129)]
        with self.assertRaisesRegex(subject.Refused, 'conversation_recovery_scope_bound'):
            subject.observe(Commands(value), 'fixture', metadata(75), COMPANY, drained=True)

    def test_workspace75_projection_keeps_its_exact_branch_and76_preserves_same_snapshot(self):
        sql = subject.query(75, COMPANY, workspace_layout=True)
        self.assertEqual(sql.count('BEGIN ISOLATION LEVEL'), 1)
        self.assertIn("'workspace_layout',jsonb_build_object", sql)
        self.assertIn('convert_from(b.grant_bytes', sql)
        self.assertIn('conversation_memory_authorities', sql)
        self.assertNotIn('conversation recovery snapshot bound', sql)
        sql76 = subject.query(76, COMPANY, workspace_layout=True)
        self.assertEqual(sql76.count('BEGIN ISOLATION LEVEL'), 1)
        self.assertIn("'workspace_layout',jsonb_build_object", sql76)
        self.assertIn('conversation recovery snapshot bound', sql76)
        with self.assertRaises(subject.Refused): subject.query(78, COMPANY, workspace_layout=True)

    def test76_explicitly_replaces75_execution_refusal_without_changing_other_counters(self):
        old = subject.counters(75)
        current = subject.counters(76)
        self.assertNotIn('unsupported_conversation_execution75', current)
        for name, sql in old.items():
            if name != 'unsupported_conversation_execution75':
                self.assertEqual(current[name], sql)
        self.assertEqual(set(current)-set(old), {
            'invalid_conversation_export_history76', 'invalid_conversation_snapshot_history76'})
        contract = subject.main_contract(metadata(76))
        self.assertEqual(contract['conversation_memory']['storage_version'], 75)
        self.assertEqual(contract['conversation_memory']['snapshot_version'], 4)
        self.assertEqual(contract['conversation_memory']['runtime_publication'],
                         'schema76_retained_exports_and_v4_uses_only')
        self.assertFalse(contract['automatic_activation'])
        self.assertIn('same_key_remote_reconciliation', contract['activation_requires'])
        self.assertIn('retained_withdrawal_expiry_catch_up', contract['activation_requires'])
        self.assertEqual(subject.main_contract(metadata(75))['conversation_memory'],
                         subject.conversations.RECOVERY_CONTRACT)

    def test76_bounds_all_candidate_bytes_before_json_decoding_and_keeps_hashed_output(self):
        commands = Commands(witness(76))
        subject.observe(commands, 'fixture', metadata(76), COMPANY, drained=True)
        sql = commands.calls[0][2]['sql']
        guard = sql.index("RAISE EXCEPTION 'conversation recovery snapshot bound'")
        decode = sql.index("ortak_snapshot_scratch_jsonb(convert_from(s.spec_bytes,'UTF8')::json)")
        self.assertLess(guard, decode)
        self.assertIn("')>8388608", sql[:guard])
        self.assertIn("')>1024", sql[:guard])
        self.assertIn('octet_length(spec_bytes) NOT BETWEEN 1 AND 262144', sql[:guard])
        self.assertEqual(sql.count('BEGIN ISOLATION LEVEL REPEATABLE READ READ ONLY'), 1)
        self.assertIn('s.company_id=\'' + COMPANY + '\'', sql)
        self.assertIn('h.spec_hash=sha256(h.spec_bytes)', sql)
        self.assertEqual(commands.calls[0][2]['ceiling'], 384 * 1024)
        self.assertEqual(witness(76)['evidence']['tables'].keys(), witness(75)['evidence']['tables'].keys())
        self.assertIn("'row_sha256',encode(sha256(convert_to(to_jsonb(t)::text,'UTF8')),'hex')", sql)
        self.assertNotIn("'spec_bytes',", sql)
        self.assertTrue(sql.endswith('ROLLBACK;'))

    def test76_history_uses_retained_v4_identity_without_current_authority(self):
        sql = subject.query(76, COMPANY)
        fragment = (Path(__file__).resolve().parents[2] / 'docs/ortak/sql/conversation_runtime76.sql').read_text()
        runtime = Path(__file__).resolve().parents[2] / 'crates/ortak-runtime/src/memory_context/conversation'
        record = (runtime / 'record.rs').read_text()
        origin = (runtime / 'origin.rs').read_text()
        for field in ('conversation_audience_hash', 'conversation_authority_epoch', 'conversation_consumption_epoch'):
            self.assertIn('pub ' + field + ':', record)
            self.assertIn("'" + field + "'", fragment)
            self.assertIn("'" + field + "'", sql)
        for field in ('requester_public_key', 'provenance'):
            self.assertIn(field + ': String', origin)
            self.assertIn("'" + field + "'", sql)
        for required in ("h.wire->'version'='4'::jsonb", "NOT h.wire ? 'reviewed'",
                         "h.spec_hash=sha256(h.spec_bytes)", "u.ordinal=selected.n-1",
                         "'scope',u.audience_kind,'record',rec.record",
                         "'reviewed_conversation_memory'", "'reviewed_project_memory'", "'run_scratch_memory'",
                         "'trust','untrusted_data'", "h.origin->>'provenance'=ortak_conversation_json75(h.provenance)",
                         "h.origin->>'requester_public_key'=h.requested_by", "h.definition_hash=sha256(h.definition_bytes)",
                         "'source_event_id'=encode(h.message_id,'hex')", 'jsonb_array_length(h.records) BETWEEN 1 AND 8',
                         'jsonb_array_length(h.records)+jsonb_array_length(h.scratch)<=8',
                         'u.conversation_authority_epoch<=scope.epoch', 'u.conversation_consumption_epoch<=t.conversation_consumption_epoch'):
            self.assertIn(required, sql)
        for forbidden in ('ortak_conversation_run_origin(', 'ortak_conversation_source_observation(',
                          'ortak_conversation_runtime_eligible(', 'ortak_run_reviewed_memory_current(',
                          'ortak_conversation_snapshot76(', 'active_revision_id', 'JOIN events ',
                          'JOIN office_inbox ', 'JOIN channel_members ', 'project_access_grants',
                          'u.fact_version=f.version', 'UPDATE ', 'INSERT ', 'DELETE '):
            self.assertNotIn(forbidden, sql)
        # Restored epoch/expiry/status history is never forced back into current use.
        history = subject.conversations.SNAPSHOT_HISTORY76 + subject.conversations.EXPORT_HISTORY76
        self.assertNotIn('clock_timestamp', history)
        self.assertNotIn('valid_until', history)
        self.assertNotIn('f.version=1', history)
        self.assertNotIn("r.status IN", history)

    def test76_export_and_snapshot_corruption_refuse_offline_but_due_cleanup_stays_inert(self):
        value = witness(76)
        value['evidence']['tables'][AUTHORITY] = observed75()['evidence']['tables'][AUTHORITY]
        value['evidence']['tables'][AUDIENCE] = observed75()['evidence']['tables'][AUDIENCE]
        value['evidence']['tables']['run_reviewed_memory_uses'] = [
            {'key': [COMPANY, '55555555-5555-5555-5555-555555555555', 0], 'row_sha256': 'c'*64}]
        for name in ('invalid_conversation_export_history76', 'invalid_conversation_snapshot_history76'):
            for drained in (False, True):
                changed = copy.deepcopy(value)
                changed['counters'][name] = 1
                with self.subTest(counter=name, drained=drained), self.assertRaisesRegex(
                        subject.Refused, 'conversation_recovery_history_inconsistent'):
                    subject.observe(Commands(changed), 'fixture', metadata(76), COMPANY, drained=drained)
        original = copy.deepcopy(value['evidence'])
        value['counters']['uncertain_or_due_export_jobs'] = 1
        result = subject.verify_restore(Commands(value), 'offline', metadata(76), COMPANY, original)
        self.assertEqual(result['evidence'], original)
        self.assertFalse(result['automatic_activation'])
        with self.assertRaisesRegex(subject.Refused, 'not_drained'):
            subject.observe(Commands(value), 'fixture', metadata(76), COMPANY, drained=True)
        value['evidence']['tables']['run_reviewed_memory_uses'][0]['row_sha256'] = 'd'*64
        with self.assertRaisesRegex(subject.Refused, 'obligations_changed'):
            subject.verify_restore(Commands(value), 'offline', metadata(76), COMPANY, original)

    def test76_office_requester_and_work_source_bind_only_retained_company_scoped_parents(self):
        command = Commands(witness(76))
        subject.observe(command, 'fixture', metadata(76), COMPANY, drained=True)
        sql = command.calls[0][2]['sql']
        for predicate in (
            'LEFT JOIN routing_decisions decision ON decision.company_id=r.company_id AND decision.id=r.routing_decision_id',
            "h.decision_origin_type='human' AND h.decision_origin_id=h.origin->>'requester_public_key'",
            'h.decision_message_id=h.message_id AND h.decision_root_message_id=h.root_message_id',
            'LEFT JOIN work_items item ON item.company_id=work.company_id AND item.project_id=work.project_id',
            'AND item.id=work.work_item_id',
            "h.provenance->>'source_event_id'=encode(h.work_source_message_id,'hex')",
        ):
            self.assertIn(predicate, sql)
            self.assertNotIn(predicate, subject.query(75, COMPANY))
        office = sql[sql.index('h.work_item_id IS NULL AND NOT'):sql.index('OR (h.work_item_id IS NOT NULL')]
        self.assertIn('h.decision_origin_type', office)
        self.assertNotIn('h.work_source_message_id', office)
        for forbidden in ('JOIN office_inbox ', 'JOIN events ', 'channel_members', 'project_access_grants'):
            self.assertNotIn(forbidden, sql)
        # The actual SQL counter is structural on both capture and offline
        # restore; a changed requester/source is not a mere drain warning.
        changed = witness(76)
        changed['counters']['invalid_conversation_snapshot_history76'] = 1
        for drained in (False, True):
            with self.subTest(drained=drained), self.assertRaisesRegex(subject.Refused, 'history_inconsistent'):
                subject.observe(Commands(changed), 'fixture', metadata(76), COMPANY, drained=drained)

    def test76_json_path_subtraction_has_explicit_jsonb_left_operand(self):
        command = Commands(witness(76))
        subject.observe(command, 'fixture', metadata(76), COMPANY, drained=True)
        sql = command.calls[0][2]['sql']
        for predicate in ("(h.wire->'spec')-'run_id'",
                          "(h.wire->'spec'->'context')-'conversation_ref'",
                          "(h.provenance->'audience')-'channel_id'"):
            self.assertIn(predicate, sql)
        # PostgreSQL gives arithmetic '-' higher precedence than JSON '->'.
        # Without parentheses it tries the ambiguous unknown-literal subtraction.
        self.assertNotRegex(subject.conversations.SNAPSHOT_HISTORY76, r"->'[^']+'\s*-\s*'")

    def test76_typed_scratch_identity_uniqueness_and_original_byte_bounds_reach_the_real_query(self):
        command = Commands(witness(76))
        subject.observe(command, 'fixture', metadata(76), COMPANY, drained=True)
        sql = command.calls[0][2]['sql']
        typed = (Path(__file__).resolve().parents[2] / 'crates/ortak-runtime/src/memory_context.rs').read_text()
        for predicate in (
            "count(DISTINCT record->>'record_ref')",
            "scratch.record->'scope'->>'scope'='run_scratch'",
            "scratch.record->'scope'->>'run_id'=h.run_id::text",
            "scratch.record->'provenance'->>'employee_id'=h.employee_id",
            "scratch.record->'provenance'->>'run_id'=h.run_id::text",
            "octet_length(scratch.record->>'record_ref') BETWEEN 1 AND 256",
            "octet_length(scratch.record->'provenance'->>'source') BETWEEN 1 AND 128",
            "scratch.record->>'record_ref' !~ U&'[\\0001-\\001F\\007F-\\009F]'",
            "scratch.record->'provenance'->>'source' !~ U&'[\\0001-\\001F\\007F-\\009F]'",
            "isfinite((scratch.record->'provenance'->>'recorded_at')::timestamptz)",
            "btrim(scratch.record->>'content',U&'\\0009\\000A",
            "regexp_replace(scratch.record->>'content',E'\\x01[\\x01\\x02]','','g')))/2<=4096",
        ):
            self.assertIn(predicate, sql)
            self.assertNotIn(predicate, subject.query(75, COMPANY))
        for predicate in ('MemoryScope::RunScratch { run_id }', 'provenance.run_id != Some(run_id)',
                          '!refs.insert(&record.record_ref)', 'record.content.len() > 4096',
                          'provenance.source.len() > 128', 'record.record_ref.len() > 256'):
            self.assertIn(predicate, typed)
        self.assertNotIn('clock_timestamp', subject.conversations.SNAPSHOT_HISTORY76)


if __name__ == '__main__':
    unittest.main()
