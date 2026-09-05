"""Production selection, retained receipt and command bounds without database access."""
import copy
from contextlib import redirect_stdout
import io
import json
import os
from pathlib import Path
import signal
import subprocess
import tempfile
import time
import unittest
from unittest.mock import patch

import check_schema_parity as subject

URL = "postgres://fixture:selected-fixture@127.0.0.1:55432/postgres"
EXPECTED_FUNCTIONS = ["ortak_check_routing_claim_expiry", "ortak_check_work_api_receipt", "ortak_project_access_guard",
                      "ortak_assert_project_binding_purge", "ortak_guard_project_api_binding", "ortak_project_binding_purge_at_commit",
                      "ortak_check_activation_admission_at_commit", "ortak_guard_activation_operation", "ortak_guard_activation_receipt"]


def catalog():
    return {"tables": sorted(subject.TABLES), "columns": [["fixture", "column", "uuid"]],
            "indexes": [["fixture", "index", "ORDER BY a DESC NULLS FIRST", "{3}"]],
            "constraints": [["fixture", "constraint", "FOREIGN KEY (a,b)", True, True]],
            "triggers": [["routing_decisions", "ortak_routing_claim_expiry_at_commit", "O", 5, True, True, "definition"],
                         ["work_api_operations", "work_api_receipt_at_commit", "O", 5, True, True, "definition"],
                         ["project_api_bindings", "project_api_binding_purge_at_commit", "O", 9, True, True, "definition"],
                         ["provisioning_operations", "ortak_activation_admission_at_commit", "O", 21, True, True, "definition"],
                         ["provisioning_operations", "ortak_activation_operation_immutable", "O", 27, False, False, "definition"],
                         ["provisioning_operation_steps", "ortak_activation_receipt_immutable", "O", 27, False, False, "definition"],
                         ["provisioning_operations", "ortak_activation_operation_no_truncate", "O", 34, False, False, "definition"],
                         ["provisioning_operation_steps", "ortak_activation_receipt_no_truncate", "O", 34, False, False, "definition"]],
            "functions": [[name, "", "plpgsql", "body"] for name in sorted(EXPECTED_FUNCTIONS)],
            "fence_targets": [["project_api_bindings", "fence", "O", 23, False, False, "definition"]]}


class FakeCommands:
    calls = []
    def __init__(self, directory):
        self.directory = directory
        self.deadline = time.monotonic() + 300
    def run(self, label, args, environment):
        self.calls.append((label, args, environment))
        assert "selected-fixture" not in " ".join(args)
        assert all(name not in environment for name in ("DATABASE_URL", "PGSERVICE", "PGHOSTADDR", "HTTPS_PROXY"))
        if label == "migration-test":
            assert args[1:] == ["--exact", subject.TEST, "--ignored", "--test-threads=1"]
            assert ":55432/ortak_parity_" in environment["BUZZ_TEST_DATABASE_URL"]
        else:
            assert args[args.index("--port") + 1] == "55432"
            assert args[args.index("--plan-port") + 1] == "55432"
        assert environment["PGPORT"] == environment["PGSCHEMA_PLAN_PORT"] == "55432"
        subject.database_name(environment["PGDATABASE"])


class FakeDatabase:
    calls = []
    failure = False
    difference = None
    desired_reads = 0
    versions = list(range(1, 57))
    def __init__(self, selected, deadline, directory):
        self.selected = selected
    def query(self, database, sql, parameters=None, admin=False):
        self.calls.append((database, sql, parameters, admin))
        if admin:
            assert database == "postgres" and sql.startswith('CREATE DATABASE "ortak_parity_')
            assert sql.endswith('" TEMPLATE template0')
            if self.failure: raise RuntimeError("fixture private error")
            return
        subject.database_name(database)
        if "json_agg(version" in sql: return self.versions
        if sql == subject.CATALOG:
            assert parameters == (subject.TABLES, subject.FUNCTIONS, subject.TABLES)
            value = catalog()
            if database.endswith("desired"):
                type(self).desired_reads += 1
            if self.difference and database.endswith("migrated"):
                value[self.difference][0].append("semantic difference")
            return value


def hanging_query_worker(selected, database, sql, parameters, options, result_path):
    """A real unresponsive SQL-process seam, including ignored polite shutdown."""
    signal.signal(signal.SIGTERM, signal.SIG_IGN)
    subject.document(result_path, {"pid": os.getpid()})
    while True:
        time.sleep(60)


def completed_query_worker(selected, database, sql, parameters, options, result_path):
    subject.document(result_path, {"status": "ok", "value": {"database": database}})


def failed_query_worker(selected, database, sql, parameters, options, result_path):
    subject.document(result_path, {"status": "failed", "error_message": "private SQL diagnostic"})


class ParityTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name).resolve()
        self.binary = self.root / "test-binary"
        self.binary.write_bytes(b"fixture binary")
        self.binary.chmod(0o700)
        cached = patch.object(subject, "PGSCHEMA", self.binary)
        cached.start(); self.addCleanup(cached.stop)
        FakeCommands.calls = []
        FakeDatabase.calls = []
        FakeDatabase.failure = False
        FakeDatabase.difference = None
        FakeDatabase.desired_reads = 0
        FakeDatabase.versions = list(range(1, 57))

    def invoke(self, url=URL):
        return subject.probe(url, self.binary, self.root, FakeCommands, FakeDatabase)

    def test_wrong_or_absent_database_selection_stops_before_files_or_calls(self):
        for value in (None, "", URL.replace("55432", "55433"), URL.replace("55432", "5432"),
                      URL.replace("127.0.0.1", "localhost"), URL.replace("127.0.0.1", "remote.example"),
                      URL + "?host=remote", URL + "#fragment", URL.replace("/postgres", "/postgres/extra")):
            with self.subTest(value=value), self.assertRaises(subject.Refused): self.invoke(value)
        self.assertEqual(list(self.root.iterdir()), [self.binary])
        self.assertFalse(FakeCommands.calls)
        self.assertFalse(FakeDatabase.calls)
        with patch.dict(os.environ, {"DATABASE_URL": URL, "PGHOST": "remote", "PGSERVICE": "old"}):
            with self.assertRaises(subject.Refused): self.invoke(None)

    def test_cli_never_falls_back_to_database_url_or_reads_private_state(self):
        output = io.StringIO()
        args = ["check_schema_parity.py", "--migration-test-binary", str(self.binary),
                "--receipt-parent", str(self.root)]
        with patch.dict(os.environ, {"DATABASE_URL": URL, "PGPASSWORD": "must-not-read"}, clear=True), \
             patch("sys.argv", args), patch.object(subject, "probe") as probe, redirect_stdout(output):
            self.assertEqual(subject.main(), 1)
        probe.assert_not_called()
        self.assertEqual(json.loads(output.getvalue()), {"status": "failed", "code": "schema_parity_failed", "databases_retained": True})
        self.assertEqual(list(self.root.iterdir()), [self.binary])

    def test_success_retains_databases_and_protected_exact_receipt(self):
        with patch.dict(os.environ, {"DATABASE_URL": "remote", "PGSERVICE": "old", "HTTPS_PROXY": "remote"}):
            directory = self.invoke()
        intent = json.loads((directory / "intent.json").read_text())
        receipt = json.loads((directory / "receipt.json").read_text())
        self.assertEqual(receipt["status"], "verified")
        self.assertEqual(receipt["reconciliation_passes"], 2)
        self.assertEqual(receipt["migration_versions"], list(range(1, 57)))
        self.assertEqual(receipt["migration_target"], 56)
        self.assertTrue(receipt["databases_retained"])
        self.assertEqual(receipt["desired_database"], intent["desired_database"])
        self.assertNotEqual(receipt["desired_database"], receipt["migrated_database"])
        self.assertEqual(len([call for call in FakeDatabase.calls if call[3]]), 2)
        self.assertEqual(FakeDatabase.desired_reads, 2)
        for path in directory.iterdir():
            self.assertEqual(path.stat().st_mode & 0o777, 0o700 if path.is_dir() else 0o600)
            if path.is_file(): self.assertNotIn(b"selected-fixture", path.read_bytes())
        self.assertFalse(any("DROP DATABASE" in call[1] for call in FakeDatabase.calls))

    def test_creation_failure_retains_intent_and_failed_receipt_without_cleanup(self):
        FakeDatabase.failure = True
        with self.assertRaises(subject.Refused): self.invoke()
        directory = next(self.root.glob("schema-parity-*"))
        receipt = json.loads((directory / "receipt.json").read_text())
        self.assertEqual(receipt["status"], "failed")
        self.assertNotIn("fixture private error", json.dumps(receipt))
        self.assertTrue((directory / "intent.json").exists())
        self.assertFalse(FakeCommands.calls)
        self.assertFalse(any("DROP" in call[1] for call in FakeDatabase.calls))

    def test_semantic_catalog_differences_are_not_normalized_away(self):
        for component in ("columns", "indexes", "constraints", "functions", "fence_targets"):
            FakeDatabase.difference = component
            with self.assertRaises(subject.Refused): self.invoke()
        for directory in self.root.glob("schema-parity-*"):
            receipt = json.loads((directory / "receipt.json").read_text())
            self.assertEqual(receipt["status"], "failed")
            self.assertEqual(len(receipt["different_components"]), 1)
        value = copy.deepcopy(catalog())
        value["triggers"][0][5] = False
        with self.assertRaisesRegex(subject.Refused, "deferred_commit_guard"): subject.checked_catalog(value)

    def test_delete_commit_guard_must_remain_enabled_after_row_delete_and_deferred(self):
        for position, changed in ((2, "D"), (3, 5), (4, False), (5, False)):
            value = catalog()
            value["triggers"][2][position] = changed
            with self.assertRaisesRegex(subject.Refused, "deferred_commit_guard"):
                subject.checked_catalog(value)
        value = catalog()
        value["triggers"].pop(2)
        with self.assertRaisesRegex(subject.Refused, "deferred_commit_guard"):
            subject.checked_catalog(value)

    def test_activation_guards_are_bound_to_exact_table_events_and_deferred_mode(self):
        for index in range(3, 8):
            expected = "deferred_commit_guard" if index == 3 else "activation_mutation_guard"
            for position, changed in ((0, "other_table"), (2, "D"), (3, 5),
                                      (4, index != 3), (5, index != 3)):
                value = catalog()
                value["triggers"][index][position] = changed
                with self.subTest(index=index, position=position), self.assertRaisesRegex(subject.Refused, expected):
                    subject.checked_catalog(value)
            value = catalog()
            value["triggers"].pop(index)
            with self.assertRaisesRegex(subject.Refused, expected):
                subject.checked_catalog(value)
        for name in EXPECTED_FUNCTIONS[-3:]:
            value = catalog()
            value["functions"] = [row for row in value["functions"] if row[0] != name]
            with self.assertRaisesRegex(subject.Refused, "required_catalog_missing"):
                subject.checked_catalog(value)

    def test_exact56_target_rejects_old55_or_unreviewed57(self):
        for versions in (list(range(1, 56)), list(range(1, 58))):
            FakeDatabase.versions = versions
            with self.assertRaises(subject.Refused): self.invoke()
        for directory in self.root.glob("schema-parity-*"):
            receipt = json.loads((directory / "receipt.json").read_text())
            self.assertEqual(receipt["error_code"], "migration56_not_proven")
            self.assertEqual(receipt["migration_target"], 56)

    def test_direct_sql_hang_hits_wall_deadline_and_kills_owned_worker(self):
        selected = subject.selected_url(URL)
        database = "ortak_parity_" + "a" * 32 + "_desired"
        started = time.monotonic()
        client = subject.Database(selected, started + 1.0, self.root, hanging_query_worker)
        with self.assertRaisesRegex(subject.Refused, "sql_deadline_exceeded"):
            client.query(database, "SELECT 1")
        self.assertLess(time.monotonic() - started, 2.5)
        marker = next(self.root.glob("sql-*.json"))
        pid = json.loads(marker.read_text())["pid"]
        with self.assertRaises(ProcessLookupError):
            os.kill(pid, 0)
        self.assertEqual(marker.stat().st_mode & 0o777, 0o600)

    def test_direct_sql_result_and_private_failure_cross_only_after_worker_exit(self):
        selected = subject.selected_url(URL)
        database = "ortak_parity_" + "b" * 32 + "_migrated"
        client = subject.Database(selected, time.monotonic() + 5, self.root, completed_query_worker)
        self.assertEqual(client.query(database, "SELECT 1"), {"database": database})
        client.worker = failed_query_worker
        with self.assertRaisesRegex(subject.Refused, "^sql_query_failed$"):
            client.query(database, "SELECT invalid")
        results = [json.loads(path.read_text()) for path in self.root.glob("sql-*.json")]
        self.assertTrue(any(result.get("error_message") == "private SQL diagnostic" for result in results))

    def test_actual_output_reader_caps_mocked_child_and_stops_whole_group(self):
        read, write = os.pipe()
        os.write(write, b"oversized")
        os.close(write)
        class Child:
            pid = 987654321
            stdout = os.fdopen(read, "rb")
            def wait(self, timeout): return 0
        child = Child()
        with patch.object(subject.subprocess, "Popen", return_value=child) as spawn, \
             patch.object(subject.os, "killpg") as stop, patch.object(subject, "MAX_OUTPUT", 4):
            with self.assertRaisesRegex(subject.Refused, "output_limit"):
                subject.Commands(self.root).run("fixture", [str(self.binary)], {"PATH": "/usr/bin:/bin"})
        self.assertEqual((self.root / "fixture.log").stat().st_size, 0)
        self.assertTrue(spawn.call_args.kwargs["start_new_session"])
        self.assertEqual(spawn.call_args.kwargs["stdin"], subprocess.DEVNULL)
        stop.assert_called_once_with(child.pid, signal.SIGKILL)


if __name__ == "__main__": unittest.main()
