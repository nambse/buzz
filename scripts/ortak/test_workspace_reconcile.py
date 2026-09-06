"""Three real pgschema CHECK seams; PG faults require an explicit disposable DB.

Every PostgreSQL edit is inside an uncommitted transaction and rolled back. This
never creates/drops a database, runs a migrator, or accepts a private-stack URL.
"""
import os
import re
import unittest

import check_schema_parity as parity
import workspace_catalog as workspace

URL_ENV = 'ORTAK_WORKSPACE_RECONCILE_TEST_URL'
RULES = (
    ('workspace_files', 'workspace_files_logical_name_check'),
    ('workspace_reader_executions', 'workspace_reader_executions_check1'),
    ('workspace_reader_executions', 'workspace_reader_executions_check2'),
)
# Independently observed pgschema 1.7.4 catalog trees, not normalized originals.
FLATTENED = (
    "CHECK (((octet_length(logical_name) >= 1) AND (octet_length(logical_name) <= 256) AND (logical_name ~ '^[A-Za-z0-9][A-Za-z0-9._/-]*$'::text) AND (logical_name !~ '(^|/)(\\.|\\.\\.|)(/|$)'::text)))",
    "CHECK (((executable IS NULL) OR ((octet_length(executable) >= 1) AND (octet_length(executable) <= 4096) AND (\"left\"(executable, 1) = '/'::text) AND (octet_length(executable_hash) = 32) AND (operating_uid >= 0) AND (operating_uid <= '4294967295'::bigint))))",
)
EXPECTED = sorted(row for row in workspace.CHECKS if tuple(row[:2]) in RULES)


def block():
    """Read the same bounded release block that real pgschema callers execute."""
    source = (parity.REPO / 'scripts/reconcile-schema-after-pgschema.sql').read_text()
    matches = re.findall(r'DO \$ortak_74_checks\$[\s\S]*?\$ortak_74_checks\$;', source)
    if len(matches) != 1:
        raise AssertionError('one workspace CHECK convergence block required')
    return matches[0]


def checks(source):
    """Extract balanced CHECK expressions while preserving quoted SQL text."""
    found = []
    for start in re.finditer(r'\bCHECK\(', source):
        index, depth, quoted = start.end() - 1, 0, False
        while index < len(source):
            char = source[index]
            if char == "'":
                if quoted and source[index:index + 2] == "''":
                    index += 2
                    continue
                quoted = not quoted
            elif not quoted:
                if char == '(':
                    depth += 1
                elif char == ')':
                    depth -= 1
                    if depth == 0:
                        found.append(source[start.start():index + 1])
                        break
            index += 1
    return found


def source_tokens(value):
    # Source formatting may differ. Catalog SQL is never passed through this.
    return ''.join(part if part.startswith("'") else re.sub(r'\s+', '', part)
                   for part in re.split(r"('(?:''|[^'])*')", value))


class WorkspaceReconcileSourceTests(unittest.TestCase):
    def test_three_repairs_match_immutable_checks_and_accept_only_known_catalog_trees(self):
        source = block()
        immutable = (parity.REPO / 'migrations/0074_ortak_workspace_text_tools.sql').read_text()
        additions = re.findall(r'ALTER TABLE (\w+) ADD CONSTRAINT (\w+) (CHECK\([\s\S]*?\));', source)
        self.assertEqual([(table, name) for table, name, _ in additions], list(RULES))
        original_checks = {source_tokens(check) for check in checks(immutable)}
        for _, _, check in additions:
            self.assertIn(source_tokens(check), original_checks)
        self.assertEqual(re.findall(r'ALTER TABLE (\w+) DROP CONSTRAINT (\w+);', source), list(RULES[:2]))
        definitions = [value.replace("''", "'") for value in
                       re.findall(r"pg_get_constraintdef\(oid,false\)='((?:''|[^'])*)'", source)]
        self.assertCountEqual(definitions, list(FLATTENED) + [row[3] for row in EXPECTED])
        self.assertEqual(source.count("contype='c' AND convalidated AND NOT condeferrable AND NOT condeferred"), 5)
        self.assertEqual(source.count("RAISE EXCEPTION 'ortak: workspace desired-state check mismatch'"), 3)


@unittest.skipUnless(os.environ.get(URL_ENV), 'explicit disposable parity DB required')
class WorkspaceReconcilePostgresTests(unittest.TestCase):
    def setUp(self):
        import psycopg2
        self.psycopg2 = psycopg2
        selected = parity.selected_url(os.environ.get(URL_ENV))
        # Only root's completed, isolated parity databases are eligible. In
        # particular the retained C2 fixture and every live database are refused.
        parity.database_name(selected['dbname'])
        self.connection = psycopg2.connect(**selected, connect_timeout=5,
            options='-c statement_timeout=5000 -c lock_timeout=2000 -c idle_in_transaction_session_timeout=30000')
        self.addCleanup(self.connection.close)
        self.addCleanup(self.connection.rollback)
        self.cursor = self.connection.cursor()
        self.addCleanup(self.cursor.close)
        self.assertEqual(self.snapshot(), EXPECTED)
        self.original_catalog = self.catalog()
        self.addCleanup(self.rollback_and_verify_catalog)

    def catalog(self):
        self.cursor.execute(parity.CATALOG, (parity.TABLES, parity.FUNCTIONS, parity.TABLES))
        return parity.checked_catalog(self.cursor.fetchone()[0])

    def rollback_and_verify_catalog(self):
        self.connection.rollback()
        self.assertEqual(self.catalog(), self.original_catalog,
                         'all catalog components must remain identical after rollback')

    def snapshot(self):
        self.cursor.execute("""SELECT c.relname,k.conname,k.contype,pg_get_constraintdef(k.oid,false),
            k.convalidated,k.condeferrable,k.condeferred FROM pg_constraint k
            JOIN pg_class c ON c.oid=k.conrelid JOIN pg_namespace n ON n.oid=c.relnamespace
            WHERE n.nspname='public' AND k.conname=ANY(%s) ORDER BY c.relname,k.conname""",
            ([name for _, name in RULES],))
        return [list(row) for row in self.cursor.fetchall()]

    def drop(self, table, name):
        from psycopg2 import sql
        self.cursor.execute(sql.SQL('ALTER TABLE {} DROP CONSTRAINT {}').format(sql.Identifier(table), sql.Identifier(name)))

    def add(self, table, name, definition):
        from psycopg2 import sql
        self.cursor.execute(sql.SQL('ALTER TABLE {} ADD CONSTRAINT {} ').format(sql.Identifier(table), sql.Identifier(name))
                            + sql.SQL(definition))

    def reset(self):
        self.connection.rollback()
        self.assertEqual(self.snapshot(), EXPECTED)

    def test_real_known_flattened_and_omitted_checks_repair_exactly_and_idempotently(self):
        for index, (table, name) in enumerate(RULES):
            self.drop(table, name)
            if index < 2:
                self.add(table, name, FLATTENED[index])
        self.assertNotEqual(self.snapshot(), EXPECTED)
        self.cursor.execute(block())
        self.assertEqual(self.snapshot(), EXPECTED)
        self.cursor.execute(block())
        self.assertEqual(self.snapshot(), EXPECTED)
        self.reset()

    def test_real_each_missing_check_is_restored_without_catalog_normalization(self):
        for table, name in RULES:
            with self.subTest(missing=name):
                self.drop(table, name)
                self.cursor.execute(block())
                self.assertEqual(self.snapshot(), EXPECTED)
                self.reset()

    def refused_unchanged(self):
        fault = self.snapshot()
        self.cursor.execute('SAVEPOINT rejected_repair')
        with self.assertRaises(self.psycopg2.Error) as raised:
            self.cursor.execute(block())
        self.assertEqual(raised.exception.pgcode, 'P0001')
        self.assertEqual(raised.exception.diag.message_primary, 'ortak: workspace desired-state check mismatch')
        self.cursor.execute('ROLLBACK TO SAVEPOINT rejected_repair')
        self.assertEqual(self.snapshot(), fault, 'unknown input must remain intact after refusal')
        self.reset()

    def test_real_unknown_substitutions_are_refused_without_silent_replacement(self):
        for table, name in RULES:
            with self.subTest(unknown=name):
                self.drop(table, name)
                self.add(table, name, 'CHECK (true)')
                self.refused_unchanged()

    def test_real_unvalidated_original_checks_are_refused_not_silently_validated(self):
        for row in EXPECTED:
            with self.subTest(unvalidated=row[1]):
                self.drop(*row[:2])
                self.add(*row[:2], row[3] + ' NOT VALID')
                self.refused_unchanged()


if __name__ == '__main__':
    unittest.main()
