#!/usr/bin/env python3
"""Exercise the production PostgreSQL discovery graph without compiling Rust."""

from pathlib import Path
import runpy
import tempfile
import unittest

CHECKER = runpy.run_path(str(Path(__file__).with_name("check-postgres-test-discovery.py")))
PG_TEST = '#[test]\n#[ignore = "requires PostgreSQL"]\nfn database_case() {}\n'


class ModuleDiscoveryTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory(prefix="postgres-module-discovery-")
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)
        self.write("Cargo.toml", '[package]\nname = "discovery-fixture"\nversion = "0.0.0"\n')
        self.write("src/lib.rs", "")

    def write(self, name, source):
        path = self.root / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(source)
        return path

    def errors(self, path):
        # Single-file invocations must find the complete enclosing crate too.
        index = CHECKER["out_of_line_module_index"]([path])
        return CHECKER["validate_file"](path, index)

    def test_binary_context_reaches_two_explicit_path_descendants(self):
        self.write("tests/postgres_work.rs", '#[path = "work/authorized.rs"] mod authorized;')
        self.write("tests/work/authorized.rs", '#[path = "authorized/definition.rs"] mod definition;')
        leaf = self.write("tests/work/authorized/definition.rs", PG_TEST)
        self.assertEqual(self.errors(leaf), [])
        index = CHECKER["out_of_line_module_index"]([leaf])
        self.assertEqual(CHECKER["postgres_packages"]([leaf], index), ["discovery-fixture"])
        # The real nextest binary predicate stops selecting after this rename.
        (self.root / "tests/postgres_work.rs").rename(self.root / "tests/work.rs")
        self.assertIn("not discoverable", " ".join(self.errors(leaf)))

    def test_directory_binary_inherits_its_cargo_target_name(self):
        self.write("tests/postgres_delivery/main.rs", "mod delivery;")
        leaf = self.write("tests/postgres_delivery/delivery.rs", PG_TEST)
        self.assertEqual(self.errors(leaf), [])
        # A nested file's postgres_ prefix alone is never a binary name.
        orphan = self.write("tests/helpers/postgres_orphan.rs", PG_TEST)
        self.assertIn("not discoverable", " ".join(self.errors(orphan)))

    def test_library_namespace_reaches_standard_and_inline_descendants(self):
        self.write("src/lib.rs", "mod postgres_tests;")
        self.write("src/postgres_tests.rs", "mod nested { mod cases; }")
        leaf = self.write("src/postgres_tests/nested/cases.rs", PG_TEST)
        self.assertEqual(self.errors(leaf), [])

    def test_inline_path_is_relative_to_inline_directory(self):
        self.write("src/lib.rs", 'mod postgres_tests { #[path = "real.rs"] mod cases; }')
        leaf = self.write("src/postgres_tests/real.rs", PG_TEST)
        self.assertEqual(self.errors(leaf), [])

    def test_external_namespace_propagates_to_descendants(self):
        self.write("src/lib.rs", "mod postgres_tests;")
        self.write("src/postgres_tests.rs", "mod external_infra_tests;")
        self.write("src/postgres_tests/external_infra_tests.rs", "mod nested;")
        leaf = self.write(
            "src/postgres_tests/external_infra_tests/nested.rs",
            PG_TEST.replace("requires PostgreSQL", "requires PostgreSQL and MinIO"),
        )
        self.assertEqual(self.errors(leaf), [])
        index = CHECKER["out_of_line_module_index"]([leaf])
        self.assertEqual(CHECKER["postgres_packages"]([leaf], index), [])
        leaf.write_text(PG_TEST)
        self.assertIn("excluded by an external_infra", " ".join(self.errors(leaf)))

    def test_distinct_paths_cannot_combine_postgres_and_unexcluded_authority(self):
        self.write("src/lib.rs", "mod postgres_tests; mod ordinary;")
        self.write("src/postgres_tests.rs", '#[path = "shared.rs"] mod external_infra_tests;')
        self.write("src/ordinary.rs", '#[path = "shared.rs"] mod cases;')
        leaf = self.write("src/shared.rs", PG_TEST)
        self.assertIn("not discoverable", " ".join(self.errors(leaf)))

    def test_explicit_path_does_not_also_import_conventional_file(self):
        self.write("src/lib.rs", '#[path = "actual.rs"] mod postgres_tests;')
        actual = self.write("src/actual.rs", PG_TEST)
        decoy = self.write("src/postgres_tests.rs", PG_TEST)
        self.assertEqual(self.errors(actual), [])
        self.assertIn("not discoverable", " ".join(self.errors(decoy)))

    def test_path_attribute_on_other_item_does_not_attach_to_next_module(self):
        self.write("src/lib.rs", '#[path = "decoy.rs"] mod unrelated {}\nmod postgres_tests;')
        actual = self.write("src/postgres_tests.rs", PG_TEST)
        decoy = self.write("src/decoy.rs", PG_TEST)
        self.assertEqual(self.errors(actual), [])
        self.assertIn("not discoverable", " ".join(self.errors(decoy)))

    def test_bare_ignore_in_descendant_is_rejected(self):
        self.write("tests/postgres_work.rs", '#[path = "nested.rs"] mod nested;')
        leaf = self.write("tests/nested.rs", '#[test]\n#[ignore]\nfn missing_reason() {}')
        self.assertIn("bare #[ignore]", " ".join(self.errors(leaf)))

    def test_cycles_terminate_with_separate_bounded_contexts(self):
        self.write("src/lib.rs", "mod postgres_tests;")
        leaf = self.write("src/postgres_tests.rs", '#[path = "postgres_tests.rs"] mod recursive;\n' + PG_TEST)
        index = CHECKER["out_of_line_module_index"]([leaf])
        self.assertLessEqual(len(index[leaf.resolve()]), 4)
        self.assertEqual(CHECKER["validate_file"](leaf, index), [])


if __name__ == "__main__":
    unittest.main()
