"""The populated PG rehearsal cannot select a live port, existing DB, or ambient auth."""

import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import rehearse_private_recovery_obligations as subject
import private_recovery_obligations as obligations


class FixtureGuardTests(unittest.TestCase):
    def setUp(self):
        temp = tempfile.TemporaryDirectory(); self.addCleanup(temp.cleanup)
        self.root = Path(temp.name).resolve()
        self.database = 'ortak_g_obligations_' + 'a' * 32
        selected = subject.bounded.selected_url('postgres://fixture:synthetic@127.0.0.1:55432/postgres')
        self.fixture = subject.Fixture(self.root, selected, self.database)

    def test_only_generated_database_and_explicit_disposable_port_are_accepted(self):
        subject.generated_database(self.database)
        for name in ['ortak', 'postgres', 'ortak_g_obligations_old', 'ortak_reviewed69_' + 'a' * 32]:
            with self.assertRaises(subject.bounded.Refused): subject.generated_database(name)
        for url in ['postgres://fixture:synthetic@127.0.0.1:55433/postgres',
            'postgres://fixture:synthetic@remote:55432/postgres',
            'postgres://fixture:synthetic@127.0.0.1:55432/postgres?options=unsafe', None]:
            with self.assertRaises(subject.bounded.Refused): subject.bounded.selected_url(url)

    def test_child_environment_has_no_ambient_provider_oauth_or_connection_inheritance(self):
        with patch.dict(os.environ, {'OPENAI_API_KEY': 'do-not-inherit', 'PGSERVICE': 'unrelated',
            'DATABASE_URL': 'unrelated', 'HERMES_HOME': '/unrelated'}):
            env = self.fixture.environment(self.database)
        self.assertEqual((env['PGHOST'], env['PGPORT'], env['PGDATABASE']), ('127.0.0.1', '55432', self.database))
        self.assertTrue({'OPENAI_API_KEY', 'PGSERVICE', 'DATABASE_URL', 'HERMES_HOME'}.isdisjoint(env))
        with self.assertRaises(subject.bounded.Refused): self.fixture.environment('unrelated')

    def test_faults_wrap_the_unchanged_production_query_and_always_reach_rollback(self):
        company, fact, other = ['11111111-1111-1111-1111-111111111111',
            '22222222-2222-2222-2222-222222222222', '33333333-3333-3333-3333-333333333333']
        sql = obligations.query(69, company)
        for name, fault in subject.faults(company, fact, other).items():
            with self.subTest(name=name), patch.object(self.fixture, 'sql', return_value=b'{}') as query:
                commands = subject.WitnessCommands(self.fixture, fault)
                commands.run('recovery-obligations', commands.psql(self.database), sql=sql, ceiling=123)
                actual = query.call_args.args[0]
                self.assertTrue(actual.startswith('BEGIN ISOLATION LEVEL REPEATABLE READ; SET LOCAL session_replication_role=replica;'))
                self.assertTrue(actual.endswith(sql))
                self.assertTrue(actual.endswith('ROLLBACK;'))
                self.assertNotIn('COMMIT', actual)
                self.assertEqual(query.call_args.kwargs['ceiling'], 123)
        with self.assertRaises(subject.bounded.Refused): subject.WitnessCommands(self.fixture).psql('ortak')

    def test_skipped_or_mismatched_binary_test_cannot_claim_fixture_seed_success(self):
        for output in [b'test result: ok. 0 passed; 1 ignored;', b'test result: ok. 2 passed;']:
            def run(label, args, env): (self.root / (label + '.log')).write_bytes(output)
            with patch.object(self.fixture.commands, 'run', side_effect=run), self.assertRaisesRegex(subject.bounded.Refused, 'exact_one'):
                self.fixture.run_test('fixture', Path('/fixture/test'), 'exact::test', ignored=True)


if __name__ == '__main__': unittest.main()
