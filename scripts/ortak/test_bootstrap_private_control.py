"""Capability publication using disposable private files and a mocked database."""
import copy
from contextlib import redirect_stdout
import io
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

import bootstrap_private_control as subject
import private_native_services

COMPANY = "e714c402-1f19-4e94-9af1-0bc5f960269e"
COMMUNITY = "496b4e50-cf90-4137-9e44-6c2b157bc3c1"
CHANNEL = "ccf58a39-be72-4c0f-bbd1-1caccc2157e7"


class ControlBootstrapTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name).resolve()
        self.owner = "a" * 64
        self.config = {"origin": "http://127.0.0.1:8787", "community_id": COMMUNITY,
            "humans": [{"public_key": self.owner, "role": "operator", "channel_ids": [CHANNEL],
                        "employee_ids": ["ada-private"]}],
            "allowed_web_origins": ["http://localhost:1427", "tauri://localhost"]}
        self.write(".ortak-private-stack.json", {"project": subject.PROJECT, "state_directory": str(self.root)})
        self.write("identities.json", {"project": subject.PROJECT, "company_id": COMPANY,
            "employee_id": "ada-private", "owner": {"public_key": self.owner, "secret_key": "unused-fixture-secret"}})
        selected = patch.object(private_native_services, "STATE_DIRECTORY", self.root)
        selected.start()
        self.addCleanup(selected.stop)
        process = patch.object(subject.subprocess, "run")
        self.db = process.start()
        self.addCleanup(process.stop)

    def write(self, name, value, mode=0o600):
        path = self.root / name
        path.write_text(json.dumps(value))
        path.chmod(mode)

    def read(self, name):
        return json.loads((self.root / name).read_text())

    def invoke(self, enable=False):
        args = ["bootstrap_private_control.py", "--state-dir", str(self.root),
                "--community", COMMUNITY, "--channel", CHANNEL]
        if enable:
            args.append("--enable-project-creation")
        output = io.StringIO()
        with patch("sys.argv", args), redirect_stdout(output):
            subject.main()
        self.assertNotIn("unused-fixture-secret", output.getvalue())

    def test_upgrade_changes_only_capability_and_retains_exact_private_previous_config(self):
        self.invoke()
        self.assertEqual(self.read("api-config.json"), self.config)
        before = (self.root / "api-config.json").read_bytes()
        self.invoke(True)
        enabled = copy.deepcopy(self.config)
        enabled["humans"][0]["can_create_projects"] = True
        self.assertEqual(self.read("api-config.json"), enabled)
        self.assertEqual((self.root / "api-config.before-work.json").read_bytes(), before)
        self.assertFalse((self.root / "api-config.work-next.json").exists())
        for name in ("api-config.json", "api-config.before-work.json"):
            self.assertEqual((self.root / name).stat().st_mode & 0o777, 0o600)
        self.assertEqual(self.db.call_count, 2)
        for call in self.db.call_args_list:
            self.assertEqual(call.args[0][:3], ["/usr/local/bin/docker", "--host", "unix:///Users/nambse/.docker/run/docker.sock"])
            self.assertEqual(call.kwargs["env"], {"PATH": "/usr/bin:/bin:/usr/sbin:/sbin", "LANG": "C", "LC_ALL": "C"})
            self.assertEqual(call.kwargs["timeout"], 20)
            self.assertEqual(call.kwargs["stdout"], subprocess.DEVNULL)
            self.assertEqual(call.kwargs["stderr"], subprocess.DEVNULL)
            sql = call.kwargs["input"].decode()
            for required in ("BEGIN;", "COMMIT;", "status = 'draft' AND active_revision_id IS NULL"):
                self.assertIn(required, sql)
            self.assertNotIn("unused-fixture-secret", sql)

    def test_default_preserves_enabled_true_and_explicit_false_without_rewrite(self):
        for value in (False, True):
            config = copy.deepcopy(self.config)
            config["humans"][0]["can_create_projects"] = value
            self.write("api-config.json", config)
            before = (self.root / "api-config.json").read_bytes()
            self.invoke()
            self.assertEqual((self.root / "api-config.json").read_bytes(), before)
            self.assertFalse((self.root / "api-config.before-work.json").exists())

    def test_unknown_malformed_or_changed_audience_refuses_before_database(self):
        invalid = [None, [], {"humans": []}]
        for key, value in (("role", "reader"), ("channel_ids", [COMPANY]), ("employee_ids", ["cem"]),
                           ("public_key", "b" * 64), ("unknown", True), ("can_create_projects", 1),
                           ("can_create_projects", None), ("can_create_projects", "true")):
            config = copy.deepcopy(self.config)
            config["humans"][0][key] = value
            invalid.append(config)
        invalid.append({**self.config, "unexpected": "field"})
        for config in invalid:
            with self.subTest(config=config):
                self.write("api-config.json", config)
                before = (self.root / "api-config.json").read_bytes()
                with self.assertRaises(ValueError):
                    self.invoke(True)
                self.assertEqual((self.root / "api-config.json").read_bytes(), before)
        self.db.assert_not_called()

    def test_duplicate_flag_is_refused_instead_of_last_value_winning(self):
        value = json.dumps(self.config).replace('"role": "operator"',
            '"role": "operator", "can_create_projects": false, "can_create_projects": true')
        path = self.root / "api-config.json"
        path.write_text(value)
        path.chmod(0o600)
        with self.assertRaises(ValueError):
            self.invoke(True)
        self.db.assert_not_called()

    def test_wrong_backup_or_permissions_refuses_before_database(self):
        self.write("api-config.json", self.config)
        wrong = copy.deepcopy(self.config)
        wrong["humans"][0]["channel_ids"] = [COMPANY]
        for backup, mode in ((wrong, 0o600), (self.config, 0o644), (None, 0o600)):
            self.write("api-config.before-work.json", backup, mode)
            with self.assertRaises(ValueError):
                self.invoke(True)
            self.assertEqual(self.read("api-config.json"), self.config)
        self.db.assert_not_called()

    def test_interrupted_matching_pending_converges_atomically_and_preserves_backup(self):
        self.write("api-config.json", self.config)
        before = (self.root / "api-config.json").read_bytes()
        with patch.object(subject.os, "replace", side_effect=OSError("fixture interruption")):
            with self.assertRaises(OSError):
                self.invoke(True)
        self.assertEqual((self.root / "api-config.json").read_bytes(), before)
        self.assertEqual((self.root / "api-config.before-work.json").read_bytes(), before)
        self.assertTrue(self.read("api-config.work-next.json")["humans"][0]["can_create_projects"])
        with patch.object(subject.os, "replace", wraps=os.replace) as replace:
            self.invoke(True)
            replace.assert_called_once_with(self.root / "api-config.work-next.json", self.root / "api-config.json")
        self.assertTrue(self.read("api-config.json")["humans"][0]["can_create_projects"])
        self.assertEqual((self.root / "api-config.before-work.json").read_bytes(), before)
        self.invoke()
        self.assertTrue(self.read("api-config.json")["humans"][0]["can_create_projects"])

    def test_different_pending_is_refused_even_without_requested_upgrade(self):
        self.write("api-config.json", self.config)
        different = copy.deepcopy(self.config)
        different["humans"][0]["can_create_projects"] = True
        self.write("api-config.work-next.json", different)
        with self.assertRaises(ValueError):
            self.invoke()
        self.db.assert_not_called()
        self.assertEqual(self.read("api-config.json"), self.config)
        self.assertEqual(self.read("api-config.work-next.json"), different)

    def test_failed_database_or_concurrent_edit_never_publishes_upgrade(self):
        self.write("api-config.json", self.config)
        self.db.side_effect = subprocess.CalledProcessError(1, ["fixture"])
        with self.assertRaises(subprocess.SubprocessError):
            self.invoke(True)
        self.assertFalse((self.root / "api-config.before-work.json").exists())
        self.assertFalse((self.root / "api-config.work-next.json").exists())
        changed = copy.deepcopy(self.config)
        changed["humans"][0]["channel_ids"] = [COMPANY]
        self.db.side_effect = lambda *args, **kwargs: self.write("api-config.json", changed)
        with self.assertRaises(ValueError):
            self.invoke(True)
        self.assertEqual(self.read("api-config.json"), changed)
        self.assertFalse((self.root / "api-config.before-work.json").exists())


if __name__ == "__main__":
    unittest.main()
