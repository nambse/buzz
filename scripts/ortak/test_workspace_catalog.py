"""Equal-but-weakened C2 catalogs must fail the real parity admission seam."""
import copy
import hashlib
import json
import re
import unittest

import check_schema_parity as subject
import workspace_catalog as workspace
from test_check_schema_parity import catalog


class WorkspaceCatalogTests(unittest.TestCase):
    def reject(self, value):
        # The same weakened catalog would otherwise compare equal on both sides.
        for candidate in (value, copy.deepcopy(value)):
            with self.assertRaises(subject.Refused):
                subject.checked_catalog(candidate)

    def test_reviewed_catalog_inventory_cannot_shrink_with_its_fixture(self):
        # Independent fingerprint of the reviewed read-only PG catalog, checked
        # against immutable 74 SQL. A deleted expected row must fail even when
        # synthetic catalogs are generated from the production requirement list.
        fields = ['TABLES', 'FUNCTIONS', 'TRIGGERS', 'FUNCTION_DEFAULTS',
                  'COLUMNS', 'CHECKS', 'UNIQUE_INDEXES']
        expected = {name: getattr(workspace, name) for name in fields}
        encoded = json.dumps(expected, sort_keys=True, separators=(',', ':')).encode()
        self.assertEqual(hashlib.sha256(encoded).hexdigest(),
                         'ca98b9815e5dcbe2e51ba8c021c087f9cb01a99354b0cf8b7ba79eb5b317a1d3')
        self.assertEqual([len(expected[name]) for name in fields], [6, 14, 34, 14, 73, 32, 13])
        subject.checked_catalog(catalog())

    def test_all_34_triggers_require_exact_mode_function_args_columns_and_no_qualifier(self):
        for index, row in enumerate(workspace.TRIGGERS):
            for position in range(len(row)):
                value = catalog()
                original = row[position]
                changed = (not original if isinstance(original, bool) else
                           original + 1 if isinstance(original, int) else
                           ['unexpected_column'] if isinstance(original, list) else 'changed')
                value['workspace_triggers'][index][position] = changed
                with self.subTest(trigger=row[:2], position=position):
                    self.reject(value)
            value = catalog()
            value['workspace_triggers'].pop(index)
            with self.subTest(missing=row[:2]):
                self.reject(value)
            # Bind the extension to the ordinary complete trigger catalog too.
            value = catalog()
            found = next(i for i, entry in enumerate(value['triggers']) if entry[:2] == row[:2])
            value['triggers'][found][3] = 0
            self.reject(value)
        for replacement in (None, [], workspace.TRIGGERS + [workspace.TRIGGERS[0]]):
            value = catalog()
            value['workspace_triggers'] = replacement
            self.reject(value)

    def test_binding_authority_arguments_include_withdrawal_and_exact_order(self):
        index = next(i for i, row in enumerate(workspace.TRIGGERS) if row[1] == 'workspace_binding_authority')
        for arguments in (('company', 'company_id'), ('company', 'revoked_at', 'company_id'),
                          ('community', 'company_id', 'revoked_at')):
            value = catalog()
            value['workspace_triggers'][index][8] = ''.join(arg + '\0' for arg in arguments).encode().hex()
            value['workspace_triggers'][index][9] = len(arguments)
            self.reject(value)

    def test_each_workspace_table_requires_its_universal_community_fence(self):
        for table in workspace.TABLES:
            value = catalog()
            value['fence_targets'] = [row for row in value['fence_targets'] if row[0] != table]
            with self.subTest(missing=table):
                self.reject(value)
            for position, replacement in ((1, 'wrong_fence'), (2, 'D'), (3, 23), (4, True), (5, True)):
                value = catalog()
                row = next(row for row in value['fence_targets'] if row[0] == table)
                row[position] = replacement
                with self.subTest(table=table, position=position):
                    self.reject(value)

    def test_all_columns_keep_types_nullability_and_defaults(self):
        for row in workspace.COLUMNS:
            for position in range(1, len(row)):
                value = catalog()
                found = next(i for i, entry in enumerate(value['columns']) if entry[:2] == row[:2])
                value['columns'][found][position] = not row[position] if isinstance(row[position], bool) else 'changed'
                with self.subTest(column=row[:2], position=position):
                    self.reject(value)
            value = catalog()
            value['columns'] = [entry for entry in value['columns'] if entry[:2] != row[:2]]
            self.reject(value)

    def test_critical_checks_and_unique_concurrency_indexes_cannot_be_weakened_equally(self):
        for component, requirements in (('constraints', workspace.CHECKS), ('indexes', workspace.UNIQUE_INDEXES)):
            for row in requirements:
                for position in range(1, len(row)):
                    value = catalog()
                    found = next(i for i, entry in enumerate(value[component]) if entry[:2] == row[:2])
                    value[component][found][position] = not row[position] if isinstance(row[position], bool) else 'changed'
                    with self.subTest(component=component, rule=row[:2], position=position):
                        self.reject(value)
                value = catalog()
                value[component] = [entry for entry in value[component] if entry[:2] != row[:2]]
                self.reject(value)

    def test_all_14_function_metadata_and_require_use_defaults_are_fixed(self):
        for name, metadata in workspace.FUNCTIONS.items():
            for position, original in enumerate(metadata, 1):
                value = catalog()
                row = next(row for row in value['functions'] if row[0] == name)
                row[position] = not original if isinstance(original, bool) else 'changed'
                with self.subTest(function=name, position=position):
                    self.reject(value)
            value = catalog()
            next(row for row in value['functions'] if row[0] == name)[-1] = ''
            self.reject(value)
        for index, row in enumerate(workspace.FUNCTION_DEFAULTS):
            for position in range(3):
                value = catalog()
                value['workspace_function_defaults'][index][position] = 'false' if position == 2 else 'changed'
                with self.subTest(function=row[0], default_position=position):
                    self.reject(value)
        value = catalog()
        value['workspace_function_defaults'] = []
        self.reject(value)

    def test_immutable74_definitions_match_final_desired_or_one_fail_closed_bootstrap(self):
        pattern = (r"CREATE(?: OR REPLACE)? FUNCTION (?P<name>\w+)\([\s\S]*?\bAS\s+"
                   r"(?P<tag>\$(?:[a-zA-Z_]\w*)?\$)(?P<body>[\s\S]*?)(?P=tag);")
        migration = (subject.REPO / 'migrations/0074_ortak_workspace_text_tools.sql').read_text()
        definitions = list(re.finditer(pattern, migration))
        self.assertEqual({entry['name'] for entry in definitions}, set(workspace.FUNCTIONS))
        self.assertEqual(len(definitions), 14)
        desired = (subject.REPO / 'schema/schema.sql').read_text()
        reconciler = (subject.REPO / 'scripts/reconcile-schema-after-pgschema.sql').read_text()
        restored = {name for name, metadata in workspace.FUNCTIONS.items()
                    if metadata[1] == 'plpgsql'} | {'ortak_run_workspace_current'}
        self.assertEqual(len(restored), 12)
        final_definitions = [entry for entry in re.finditer(pattern, reconciler)
                             if entry['name'] in workspace.FUNCTIONS]
        self.assertCountEqual([entry['name'] for entry in final_definitions], restored)
        for original in definitions:
            current = [entry for entry in re.finditer(pattern, desired) if entry['name'] == original['name']]
            self.assertEqual(len(current), 1, original['name'])
            if original['name'] == 'ortak_run_workspace_current':
                # pgschema can emit this SQL-language function before its
                # referenced tables. Only this exact closed bootstrap is allowed;
                # the mandatory reconciler must install the immutable definition.
                self.assertEqual(current[0]['body'].strip(), 'SELECT false')
                declaration = current[0].group(0).replace(current[0]['body'], original['body'], 1)
                self.assertEqual(declaration, original.group(0), original['name'])
            else:
                self.assertEqual(current[0].group(0), original.group(0), original['name'])
            if original['name'] in restored:
                # pgschema also rewrites final END whitespace in PL/pgSQL.
                # Every required final body remains byte-exact; restoring only
                # the SQL bootstrap must not hide a changed release body.
                current = [entry for entry in final_definitions if entry['name'] == original['name']]
            self.assertEqual(current[0].group(0).replace('CREATE OR REPLACE FUNCTION', 'CREATE FUNCTION'),
                             original.group(0), original['name'])


if __name__ == '__main__':
    unittest.main()
