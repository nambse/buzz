"""Falsifiable target-only populated-credential restore compatibility regressions."""

import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

from backup_private_database import Refused
import private_restore_credential_functions as subject


class RestoreTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(); self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name).resolve(); self.root.chmod(0o700)
        self.calls = []
        self.original = subject.expected(); self.final = subject.expected()
        owner = self
        class Command:
            root = owner.root
            def command(self, *args): return args
            def psql(self, database): return ('psql', database)
            def run(self, label, args, **kwargs):
                owner.calls.append((label, args, kwargs))
                if label == 'restore-original-functions': return json.dumps(owner.original).encode()
                if label == 'restore-final-functions': return json.dumps(owner.final).encode()
                return b''
        self.command = Command()

    def restore(self):
        return subject.restore_sections(self.command, 'ortak_verify_' + 'a' * 32, self.root / 'dump')

    def test_sections_configure_before_populated_data_and_reset_before_verification(self):
        result = self.restore()
        self.assertTrue(result['exact_function_catalog_restored'])
        self.assertEqual([c[0] for c in self.calls], ['restore-pre-data', 'restore-original-functions',
            'restore-configure', 'restore-data', 'restore-post-data', 'restore-reset', 'restore-final-functions'])
        for label, args, options in self.calls:
            if args[0] == 'pg_restore':
                self.assertIn('--single-transaction', args); self.assertIn('--exit-on-error', args)
                self.assertIn('--section=' + label.removeprefix('restore-'), args)
                self.assertNotIn('--clean', args); self.assertNotIn('--create', args)
            if label == 'restore-configure':
                sql = options['sql']
                self.assertIn(subject.guard_sql(), sql); self.assertIn(subject.guard_sql(True), sql)
                self.assertIn('ALTER FUNCTION public.ortak_all_credential_refs(jsonb) SET search_path TO pg_catalog, public;', sql)
                self.assertTrue(sql.startswith('BEGIN;') and sql.endswith('COMMIT;\n'))
            if label == 'restore-reset':
                self.assertIn(subject.guard_sql(True), options['sql']); self.assertIn(subject.guard_sql(), options['sql'])
                self.assertIn('RESET ALL;', options['sql'])

    def test_unknown_body_overload_or_original_configuration_refuses_before_alter_and_data(self):
        for changed in [{**subject.EXPECTED[0], 'body_sha256': '0' * 64},
            {**subject.EXPECTED[0], 'config': ['search_path=unsafe']},
            {**subject.EXPECTED[0], 'security_definer': True}]:
            with self.subTest(changed=changed), tempfile.TemporaryDirectory() as directory:
                self.command.root = Path(directory); self.calls.clear()
                self.original = [changed, subject.EXPECTED[1]]
                with self.assertRaisesRegex(Refused, 'allowlist'): self.restore()
                self.assertEqual([c[0] for c in self.calls], ['restore-pre-data', 'restore-original-functions'])

    def test_failure_retains_phase_intent_and_never_claims_reset_or_schema_success(self):
        original_run = self.command.run
        def run(label, *args, **kwargs):
            if label == 'restore-data': raise Refused('populated_check_failed')
            return original_run(label, *args, **kwargs)
        with patch.object(self.command, 'run', side_effect=run), self.assertRaises(Refused): self.restore()
        self.assertTrue((self.root / 'restore-data-intent.json').exists())
        self.assertFalse((self.root / 'restore-data-complete.json').exists())
        self.assertFalse((self.root / 'restore-reset-complete.json').exists())
        self.assertNotIn('restore-post-data', [c[0] for c in self.calls])

    def test_any_unrestored_function_setting_prevents_success(self):
        self.final = subject.expected(True)
        with self.assertRaisesRegex(Refused, 'not_restored'): self.restore()
        self.assertFalse((self.root / 'restore-verify-complete.json').exists())


if __name__ == '__main__': unittest.main()
