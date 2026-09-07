"""Exercise production build selection/capture with no Docker or real children."""

from contextlib import contextmanager, redirect_stdout
import importlib
import io
import json
import os
from pathlib import Path
import signal
import tempfile
import unittest
from unittest.mock import Mock, patch

import build_image as subject


class Process:
    """A real local output pipe paired with a mocked subprocess lifecycle."""

    def __init__(self, data=b"", hold_pipe=False, returncode=0):
        incoming, self.writer = os.pipe()
        self.stdout = os.fdopen(incoming, "rb", buffering=0)
        self.pid = 987654321
        self.wait = Mock(return_value=returncode)
        if data:
            os.write(self.writer, data)
        if not hold_pipe:
            os.close(self.writer)
            self.writer = None

    def close(self):
        self.stdout.close()
        if self.writer is not None:
            os.close(self.writer)
            self.writer = None


class BuildTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.base = "example.invalid/python@sha256:" + "a" * 64
        self.lock = self.root / "honcho-source-lock.json"
        self.lock.write_text(json.dumps({"candidate_build_base": self.base}))
        (self.root / "vendor").mkdir()
        for name in ("uv.lock", "pyproject.toml"):
            (self.root / "vendor" / name).write_text("fixture")

    @contextmanager
    def process(self, data=b"", **options):
        process = Process(data, **options)
        try:
            with patch.object(subject.subprocess, "Popen", return_value=process) as spawn, \
                    patch.object(subject.os, "killpg") as kill:
                yield process, spawn, kill
        finally:
            process.close()

    def test_import_does_not_build_or_prepare_source(self):
        with patch.object(subject.subprocess, "Popen") as spawn, \
                patch.object(subject.subprocess, "run") as run:
            importlib.reload(subject)
            spawn.assert_not_called()
            run.assert_not_called()

    def test_main_pins_transport_and_reads_no_ambient_docker_or_provider_settings(self):
        original_temporary = tempfile.TemporaryDirectory
        original_mkstemp = tempfile.mkstemp
        output = io.StringIO()
        # argparse/gettext may inspect display settings, but no Docker, proxy,
        # home or credential setting may influence the actual subprocess.
        display_settings = {"LANGUAGE", "LC_ALL", "LC_MESSAGES", "LANG", "COLUMNS", "LINES"}
        def ambient_get(name, default=None):
            self.assertIn(name, display_settings)
            return default
        def ambient_item(name):
            self.assertIn(name, display_settings)
            raise KeyError(name)
        with self.process(b"fixture build output") as (process, spawn, kill), \
                patch.object(subject, "ROOT", self.root), \
                patch.object(subject.os, "environ") as ambient, \
                patch.object(subject.tempfile, "TemporaryDirectory", side_effect=lambda **kw:
                    original_temporary(prefix=kw["prefix"], dir=self.root)), \
                patch.object(subject.tempfile, "mkstemp", side_effect=lambda **kw:
                    original_mkstemp(prefix=kw["prefix"], suffix=kw["suffix"], dir=self.root)), \
                redirect_stdout(output):
            ambient.get.side_effect = ambient_get
            ambient.__getitem__.side_effect = ambient_item
            subject.main(["tests"])
            self.assertTrue(all(call[0] in {"get", "__getitem__"} and call.args[0] in display_settings
                                for call in ambient.mock_calls))
            args = spawn.call_args.args[0]
            self.assertEqual(args[:7], [subject.DOCKER, "--host", subject.HOST, "buildx", "build", "--builder", "default"])
            self.assertIn("--load", args)
            self.assertEqual(args[args.index("--target") + 1], "tests")
            self.assertEqual(args[args.index("--build-arg") + 1], "BASE_IMAGE=" + self.base)
            self.assertEqual(args[-1], str(self.root))
            env = spawn.call_args.kwargs["env"]
            self.assertEqual(set(env), {"PATH", "LANG", "LC_ALL", "HOME", "DOCKER_CONFIG"})
            self.assertEqual(env["PATH"], "/usr/bin:/bin:/usr/sbin:/sbin")
            self.assertNotEqual(env["HOME"], env["DOCKER_CONFIG"])
            self.assertFalse(Path(env["HOME"]).exists())
            self.assertFalse(Path(env["DOCKER_CONFIG"]).exists())
            self.assertTrue(spawn.call_args.kwargs["start_new_session"])
            kill.assert_called_once_with(process.pid, signal.SIGKILL)
        started, completed = [json.loads(line) for line in output.getvalue().splitlines()]
        self.assertEqual(started["operation"], "build_started")
        self.assertEqual(completed["operation"], "build_completed")
        self.assertEqual(Path(completed["log"]).read_bytes(), b"fixture build output")
        self.assertEqual(Path(completed["log"]).stat().st_mode & 0o777, 0o600)
        self.assertEqual(completed["output_bytes"], len(b"fixture build output"))

    def test_invalid_target_base_or_unprepared_source_never_spawns(self):
        with patch.object(subject.subprocess, "Popen") as spawn:
            with self.assertRaises(subject.BuildFailed):
                subject.command("unselected", self.root)
            for base in ("example:latest", 123, "a@sha256:" + "g" * 64):
                self.lock.write_text(json.dumps({"candidate_build_base": base}))
                with self.assertRaises(subject.BuildFailed):
                    subject.command("runtime", self.root)
            self.lock.write_text("x" * 16385)
            with self.assertRaisesRegex(subject.BuildFailed, "source_lock_too_large"):
                subject.command("runtime", self.root)
            self.lock.write_text(json.dumps({"candidate_build_base": self.base}))
            (self.root / "vendor/uv.lock").unlink()
            with self.assertRaisesRegex(subject.BuildFailed, "prepared_source_required"):
                subject.command("runtime", self.root)
            spawn.assert_not_called()

    def test_fresh_config_directories_are_private_and_not_adopted(self):
        selected = subject.environment(self.root)
        for name in ("HOME", "DOCKER_CONFIG"):
            directory = Path(selected[name])
            self.assertEqual(directory.stat().st_mode & 0o777, 0o700)
            self.assertEqual(list(directory.iterdir()), [])
        with self.assertRaises(FileExistsError):
            subject.environment(self.root)

    def test_output_limit_applies_before_private_log_write_and_stops_group(self):
        output = io.BytesIO()
        with self.process(b"x" * 2048) as (process, _, kill), patch.object(subject, "MAX_OUTPUT", 1024):
            with self.assertRaisesRegex(subject.BuildFailed, "build_output_limit"):
                subject.run_build(["fixture"], {}, output)
            self.assertLessEqual(output.tell(), 1024)
            kill.assert_called_once_with(process.pid, signal.SIGKILL)

    def test_deadline_stops_group_even_if_leader_has_already_exited(self):
        with self.process(hold_pipe=True) as (process, _, kill), patch.object(subject, "MAX_SECONDS", 0.01):
            with self.assertRaisesRegex(subject.BuildFailed, "build_deadline"):
                subject.run_build(["fixture"], {}, io.BytesIO())
            kill.assert_called_once_with(process.pid, signal.SIGKILL)
            process.wait.assert_called_once_with(timeout=3)

    def test_nonzero_exit_is_a_fixed_failure_and_pipe_is_closed(self):
        with self.process(b"fixture private diagnostic", returncode=1) as (process, _, kill):
            with self.assertRaisesRegex(subject.BuildFailed, "^build_failed$"):
                subject.run_build(["fixture"], {}, io.BytesIO())
            kill.assert_called_once_with(process.pid, signal.SIGKILL)
            self.assertTrue(process.stdout.closed)


if __name__ == "__main__":
    unittest.main()
