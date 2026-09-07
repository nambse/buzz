"""Source-only parser regressions; root owns execution and actual PG parity."""
from pathlib import Path
import unittest

import schema_source as subject

REPO = Path(__file__).resolve().parents[2]


class SchemaSourceTests(unittest.TestCase):
    def test_named_body_and_trailing_language_preserve_exact_statement(self):
        sql = """-- CREATE FUNCTION decoy() RETURNS void AS $$bad$$;
CREATE OR REPLACE FUNCTION actual(value TEXT DEFAULT ';') RETURNS TEXT AS $body$
BEGIN
  -- Other dollar delimiters and CREATE FUNCTION are body bytes.
  RETURN $$literal; CREATE FUNCTION decoy()$$ || value;
END;
$body$ LANGUAGE plpgsql SET search_path=pg_catalog,public,pg_temp;
"""
        parsed = list(subject.functions(sql))
        self.assertEqual(len(parsed), 1)
        self.assertEqual(parsed[0].name, "actual")
        self.assertEqual(parsed[0].arguments, "value TEXT DEFAULT ';'")
        self.assertEqual(parsed[0].statement, sql[sql.index("CREATE OR REPLACE"):].rstrip())
        self.assertEqual(parsed[0].body,
                         "\nBEGIN\n  -- Other dollar delimiters and CREATE FUNCTION are body bytes.\n"
                         "  RETURN $$literal; CREATE FUNCTION decoy()$$ || value;\nEND;\n")
        self.assertEqual(parsed[0].with_body("SELECT false"),
                         "CREATE OR REPLACE FUNCTION actual(value TEXT DEFAULT ';') RETURNS TEXT "
                         "AS $body$SELECT false$body$ LANGUAGE plpgsql "
                         "SET search_path=pg_catalog,public,pg_temp;")

    def test_top_level_comments_quotes_and_replacements_never_hide_statements(self):
        sql = """/* outer ; /* nested ; */ remains */ SELECT E'escaped\\';still';
SELECT "semi;column", 'quote'';value';
DO $do$ BEGIN EXECUTE 'CREATE FUNCTION hidden() RETURNS void AS $$x$$;'; END $do$;
CREATE FUNCTION visible() RETURNS boolean LANGUAGE sql AS $$SELECT false$$;
CREATE OR REPLACE FUNCTION visible() RETURNS boolean AS $v$SELECT true$v$ LANGUAGE sql;
"""
        self.assertEqual(len(list(subject.statements(sql))), 5)
        parsed = list(subject.functions(sql))
        self.assertEqual([entry.name for entry in parsed], ["visible", "visible"])
        self.assertEqual([entry.body for entry in parsed], ["SELECT false", "SELECT true"])

    def test_incomplete_or_unsupported_source_refuses_without_partial_success(self):
        for sql in ("SELECT 1", "SELECT 'open;", "/* open", "DO $tag$open$$;",
                    "CREATE FUNCTION public.qualified() RETURNS text AS $$x$$;",
                    "CREATE FUNCTION unsupported() RETURNS text AS 'x';"):
            with self.subTest(sql=sql), self.assertRaises(subject.SourceError):
                list(subject.functions(sql))
        with self.assertRaisesRegex(subject.SourceError, "schema_source_bound"):
            list(subject.functions(" " * (subject.MAX_SOURCE_BYTES + 1)))

    def test_actual_routing_and_inherited_completion_suffixes_are_not_omitted(self):
        routing = (REPO / "crates/ortak-server/src/routing_stream_schema.sql").read_text()
        parsed = list(subject.functions(routing))
        self.assertEqual([entry.name for entry in parsed], ["ortak_routing_notify"])
        self.assertTrue(parsed[0].statement.endswith("$$ LANGUAGE plpgsql;"))
        self.assertIn("pg_notify('ortak_routing_v1'", parsed[0].body)
        execution = (REPO / "docs/ortak/sql/encrypted_dm_execution.sql").read_text()
        functions = {entry.name: entry for entry in subject.functions(execution)}
        completion = functions["ortak_schedule_completed_office_output"]
        self.assertIn("IF NEW.payload_mode='confidential_dm_v1' THEN RETURN NEW; END IF;", completion.body)
        self.assertTrue(completion.statement.endswith("$$ LANGUAGE plpgsql;"))
        self.assertIn("ortak_confidential_reply_lease_guard", functions)


if __name__ == "__main__":
    unittest.main()
