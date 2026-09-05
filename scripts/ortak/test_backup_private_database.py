"""Falsifiable command-bound and backup state-machine checks; no Docker access."""

from contextlib import contextmanager
import json
import os
from pathlib import Path
import sys
import signal
import tempfile
import time
import unittest
from uuid import uuid4
from unittest.mock import patch

import backup_private_database as subject


class FakeCommands:
    failure = None
    mismatch = False
    calls = []

    def __init__(self, root):
        self.root = root
        self.held = False

    def inspect(self):
        return {"container_id": "a" * 64, "image": subject.IMAGE, "volume": subject.PROJECT + "_postgres_data"}

    def command(self, *args):
        self.calls.append(args)
        return args

    def psql(self, database):
        return ("psql", database)

    @contextmanager
    def snapshot(self):
        self.held = True
        yield "00000001-00000002-1"
        self.held = False

    def metadata(self, database, label, snapshot=None):
        if database == subject.DATABASE:
            assert self.held and snapshot == "00000001-00000002-1"
        else:
            assert not self.held
            subject.verification_name(database)
        return {"migration_checksums": [[52, "a" * 64, True]],
                "schema_sha256": "b" * 64, "private_company": 1,
                "tables": {"public.companies": 2 if self.mismatch and label == "restored" else 1},
                "employee_states": {"draft": 1}, "server_version": "17.6"}

    def run(self, label, args, **kwargs):
        if self.failure == label:
            raise subject.Refused("fixture_operation_failed")
        if label == "database-size":
            return b"100\n"
        if label == "dump":
            assert self.held
            assert "--snapshot=00000001-00000002-1" in args
            path = kwargs["output"]
            path.write_bytes(b"PGDMP:fixture-private-data")
            path.chmod(0o600)
        if label == "create-verification":
            # The original intent must precede creation, even if its reply is lost.
            assert json.loads((self.root / "intent.json").read_text())["verification_database"] == args[-1]
            subject.verification_name(args[-1])
        if label == "restore":
            assert not self.held
            assert "--single-transaction" in args and "--exit-on-error" in args
            assert "--clean" not in args and "--create" not in args
            subject.verification_name(args[-1])
            assert kwargs["archive"].read_bytes() == b"PGDMP:fixture-private-data"
        return b""


class BackupTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.root.chmod(0o700)
        FakeCommands.failure = None
        FakeCommands.mismatch = False
        FakeCommands.calls = []

    def test_success_uses_snapshot_and_fresh_verification_never_original_restore(self):
        original = self.root / "original-marker"
        original.write_bytes(b"preserved")
        with patch.object(subject.shutil, "disk_usage") as usage:
            usage.return_value.free = 1024**3
            destination = subject.backup(self.root, FakeCommands)
        receipt = json.loads((destination / "manifest.json").read_text())
        self.assertEqual(receipt["status"], "verified")
        self.assertEqual(receipt["archive_sha256"], subject.digest(destination / "database.dump"))
        self.assertEqual(original.read_bytes(), b"preserved")
        self.assertNotEqual(receipt["verification_database"], subject.DATABASE)
        self.assertTrue(receipt["database_only"])
        for path in (destination.parent, destination):
            self.assertEqual(path.stat().st_mode & 0o777, 0o700)
        for path in destination.iterdir():
            self.assertEqual(path.stat().st_mode & 0o777, 0o600)

    def test_restore_failure_or_mismatch_retains_archive_and_failed_manifest(self):
        for failure, mismatch in [("restore", False), (None, True), ("create-verification", False)]:
            FakeCommands.failure, FakeCommands.mismatch = failure, mismatch
            with patch.object(subject.shutil, "disk_usage") as usage:
                usage.return_value.free = 1024**3
                with self.assertRaises(subject.Refused):
                    subject.backup(self.root, FakeCommands)
        for path in (self.root / "backups").iterdir():
            receipt = json.loads((path / "manifest.json").read_text())
            self.assertEqual(receipt["status"], "failed")
            self.assertEqual(receipt["archive_sha256"], subject.digest(path / "database.dump"))
            if receipt["error_code"] == "restore_metadata_mismatch":
                self.assertEqual(receipt["different_fields"], ["tables"])
                self.assertEqual(receipt["restored"]["tables"]["public.companies"], 2)
        self.assertFalse(any("dropdb" in args or "--clean" in args for args in FakeCommands.calls))

    def test_unsafe_targets_and_ambient_remote_settings_are_refused(self):
        for name in ["ortak", "ortak_api_20260905", "ortak_verify_../x", "ortak_verify_" + "g" * 32]:
            with self.assertRaises(subject.Refused):
                subject.verification_name(name)
        with patch.dict(os.environ, {"DOCKER_HOST": "tcp://remote:2375", "DOCKER_CONTEXT": "remote", "PGPASSWORD": "fixture-private"}):
            self.assertEqual(set(subject.environment()), {"PATH", "LANG", "LC_ALL"})
            self.assertEqual(subject.Commands(self.root).docker("ps")[:3], [subject.DOCKER, "--host", subject.SOCKET])
        (self.root / "backups").symlink_to(self.root, target_is_directory=True)
        with self.assertRaises(subject.Refused):
            subject.backup(self.root, FakeCommands)

    def test_real_capture_rejects_excess_output_and_kills_timed_out_process(self):
        commands = subject.Commands(self.root)
        with self.assertRaisesRegex(subject.Refused, "output_limit"):
            commands.run("oversized", [sys.executable, "-c", "import sys; sys.stdout.buffer.write(b'x'*2048)"],
                         output=self.root / "partial.dump", ceiling=1024)
        self.assertLessEqual((self.root / "partial.dump").stat().st_size, 1024)
        commands.deadline = time.monotonic() + 0.1
        with self.assertRaisesRegex(subject.Refused, "deadline"):
            commands.run("slow", [sys.executable, "-c", "import time; time.sleep(60)"])

    def test_exited_leader_cannot_leave_child_holding_output_pipe(self):
        commands = subject.Commands(self.root)
        commands.deadline = time.monotonic() + 0.3
        child_file = self.root / "owned-child.pid"
        program = (
            "import os,sys,time; child=os.fork(); "
            "os._exit(0) if child else None; "
            "open(sys.argv[1],'w').write(str(os.getpid())); time.sleep(60)"
        )
        with self.assertRaisesRegex(subject.Refused, "deadline"):
            commands.run("exited-leader", [sys.executable, "-c", program, str(child_file)])
        child = int(child_file.read_text())
        alive = True
        try:
            for _ in range(100):
                try:
                    os.kill(child, 0)
                except ProcessLookupError:
                    alive = False
                    break
                time.sleep(0.01)
            self.assertFalse(alive, "the exited leader's child remained alive after command cleanup")
        finally:
            # Keep this falsifiable test from leaking its deliberately sleeping
            # child when the production process-group guard is removed.
            if alive:
                try:
                    os.kill(child, signal.SIGKILL)
                except ProcessLookupError:
                    pass

    def test_diagnostics_stay_in_private_file_not_exception(self):
        commands = subject.Commands(self.root)
        with self.assertRaises(subject.Refused) as result:
            commands.run("diagnostic", [sys.executable, "-c", "import sys; sys.stderr.write('fixture-private-value'); sys.exit(1)"])
        self.assertNotIn("fixture-private-value", str(result.exception))
        path = self.root / "diagnostic.stderr"
        self.assertEqual(path.read_text(), "fixture-private-value")
        self.assertEqual(path.stat().st_mode & 0o777, 0o600)

    @unittest.skipUnless(os.environ.get("ORTAK_BACKUP_SQL_TEST_DATABASE"),
        "explicit retained private verification database required for real SQL catalog test")
    def test_real_column_catalog_ignores_dropped_holes_but_preserves_column_order(self):
        database = subject.verification_name(os.environ["ORTAK_BACKUP_SQL_TEST_DATABASE"])
        commands = subject.Commands(self.root)
        commands.inspect()
        suffix = uuid4().hex
        hole, compact, reordered = ["backup_probe_" + kind + "_" + suffix for kind in ("h", "c", "r")]
        # Probe DDL is confined to the generated verification database and is
        # rolled back. The definitions CTE is the exact production query.
        sql = f"""BEGIN;
CREATE TABLE public.{hole}(first_col integer, removed_col text, last_col text);
ALTER TABLE public.{hole} DROP COLUMN removed_col;
CREATE TABLE public.{compact}(first_col integer, last_col text);
CREATE TABLE public.{reordered}(last_col text, first_col integer);
WITH definitions AS ({subject.COLUMN_ROWS_SQL}), selected AS (
 SELECT relation,jsonb_agg(jsonb_build_array(ordinal,name,data_type,not_null,
  identity_kind,generated_kind,default_value) ORDER BY ordinal) definition
 FROM definitions WHERE relation IN ('{hole}','{compact}','{reordered}') GROUP BY relation
) SELECT jsonb_object_agg(relation,definition) FROM selected;
ROLLBACK;
"""
        result = json.loads(commands.run("column-hole", commands.psql(database), sql=sql))
        self.assertEqual(result[hole], result[compact])
        self.assertNotEqual(result[compact], result[reordered])
        self.assertEqual([column[0] for column in result[hole]], [1, 2])

    def test_invalid_row_counts_cannot_become_verified_metadata(self):
        commands = subject.Commands(self.root)
        commands.container = "a" * 64
        value = FakeCommands(self.root).metadata("ortak_verify_" + "a" * 32, "restored")
        for invalid in [None, -1, True]:
            value["tables"]["public.other"] = invalid
            with patch.object(commands, "run", return_value=json.dumps(value).encode()):
                with self.assertRaises(subject.Refused):
                    commands.metadata(subject.DATABASE, "invalid-count")

    def test_wrong_container_or_volume_ownership_cannot_produce_exec_command(self):
        commands = subject.Commands(self.root)
        mount = {"Type": "volume", "Name": subject.PROJECT + "_postgres_data", "RW": True,
                 "Destination": "/var/lib/postgresql/data", "Source": "/local/fresh/volume"}
        fields = ["a" * 64, subject.IMAGE, subject.PROJECT, "postgres", True, [mount]]
        encoded = "\n".join(json.dumps(value) for value in fields).encode()
        wrong = {"Driver": "local", "Mountpoint": mount["Source"],
                 "Labels": {"com.docker.compose.project": "old-stack", "com.docker.compose.volume": "postgres_data"}}
        with patch.object(commands, "run", side_effect=[encoded, json.dumps(wrong).encode()]):
            with self.assertRaisesRegex(subject.Refused, "volume_ownership"):
                commands.inspect()
        with self.assertRaisesRegex(subject.Refused, "not_verified"):
            commands.command("pg_dump")
        fields[2] = "old-stack"
        with patch.object(commands, "run", return_value="\n".join(json.dumps(value) for value in fields).encode()):
            with self.assertRaisesRegex(subject.Refused, "container_identity"):
                commands.inspect()


if __name__ == "__main__":
    unittest.main()
