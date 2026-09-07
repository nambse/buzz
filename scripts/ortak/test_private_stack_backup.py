"""Backup refusals bind the cold-source and native-application production gates."""

import json
from pathlib import Path
import plistlib
import tempfile
import unittest
from unittest.mock import Mock, patch

import private_stack_backup as backup
import private_stack_database as database
import private_stack_restore as restore


class BackupScopeTests(unittest.TestCase):
    def test_running_source_prevents_any_cold_volume_read(self):
        installation = Mock()
        installation.verify.return_value = {"hermes": {"Running": True}}
        with patch.object(backup, "docker") as docker:
            with self.assertRaises(ValueError):
                backup.cold(installation)
        docker.assert_not_called()

    def test_other_writer_on_owned_volume_refuses(self):
        installation = Mock()
        installation.verify.return_value = {"hermes": {"Running": False}}
        installation.manifest = {"services": {}, "containers": {
            name: {"mounts": [{"Type": "volume", "Name": name}]} for name in backup.VOLUMES.values()}}
        with patch.object(backup, "docker", return_value=(0, "another-writer\n")):
            with self.assertRaises(ValueError):
                backup.cold(installation)

    def test_open_private_application_refuses_host_data_capture(self):
        with tempfile.TemporaryDirectory() as directory:
            bundle = Path(directory).resolve() / "Ortak Private.app"
            (bundle / "Contents/MacOS").mkdir(parents=True)
            (bundle / "Contents/Info.plist").write_bytes(plistlib.dumps({"CFBundleIdentifier": "dev.ortak.private20260905"}))
            (bundle / "Contents/MacOS/buzz-desktop").write_bytes(b"fixture")
            with patch.object(backup, "command", return_value=(0, "1234\n")):
                with self.assertRaises(ValueError):
                    backup.native_selection(bundle)

    def test_unreviewed_database_namespace_stops_before_content_read(self):
        commands = Mock()
        commands.run.return_value = json.dumps(["private_extra", "public"]).encode()
        with self.assertRaises(ValueError):
            database.metadata(commands, "a" * 64, "main", "fixture")
        self.assertEqual(commands.run.call_count, 1)

    def test_failed_restored_database_comparison_stops_its_exact_container(self):
        identifier = "a" * 64
        image = "sha256:" + "b" * 64
        output = Path("/fixture/operation")
        receipt = {"installation": {"containers": {"postgres-1": {"image": image}}},
                   "databases": {"main": {"expected": True}}}
        row = {"Id": identifier, "Name": "/ortak-restore80-db-operation-main", "Image": image,
               "Config": {"Labels": {restore.LABEL: "operation"}},
               "HostConfig": {"NetworkMode": "none", "PortBindings": {}},
               "Mounts": [{"Name": "fresh-target"}], "State": {"Running": True}}
        with patch.object(restore, "docker", side_effect=[(0, identifier), (0, ""), (0, "")]) as docker, \
                patch.object(restore, "inspect_container", return_value=row), \
                patch.object(restore, "save_json"), \
                patch.object(restore, "metadata", return_value={"unexpected": True}):
            with self.assertRaises(ValueError):
                restore.verify_database(receipt, output, "main", {"volume": "fresh-target"}, Mock())
        self.assertEqual(docker.call_args.args, ("stop", "--time", "20", identifier))
        self.assertEqual(docker.call_args.kwargs, {"timeout": 30})


if __name__ == "__main__":
    unittest.main()
