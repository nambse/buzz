"""Selected desktop artifacts and duplicate launches bind the production entry points."""

import json
from pathlib import Path
import plistlib
import tempfile
import unittest
from unittest.mock import Mock, patch

import private_stack_desktop as desktop


class DesktopTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name).resolve()
        self.bundle = self.root / "source.app"
        (self.bundle / "Contents/MacOS").mkdir(parents=True)
        (self.bundle / desktop.BINARY).write_bytes(b"selected-native-artifact")
        (self.bundle / "Contents/Info.plist").write_bytes(plistlib.dumps({"CFBundleIdentifier": desktop.IDENTIFIER}))
        self.lifecycle = self.root / "lifecycle"
        self.lifecycle.mkdir()
        self.installation = Mock(root=self.root, directory=self.lifecycle)

    def install(self):
        with patch.object(desktop, "running", return_value=[]):
            return desktop.install_app(self.installation, self.bundle)

    def test_modified_selected_bundle_refuses_before_start_or_identity_read(self):
        selected = Path(self.install()["bundle"])
        (selected / desktop.BINARY).write_bytes(b"modified")
        with patch("private_stack_services.start") as start, patch.object(desktop, "environment") as environment:
            with self.assertRaises(ValueError):
                desktop.open_app(self.installation)
            start.assert_not_called()
            environment.assert_not_called()

    def test_selected_copy_survives_source_changes_and_reinstall_keeps_previous(self):
        first = self.install()
        (self.bundle / desktop.BINARY).write_bytes(b"new-native-artifact")
        second = self.install()
        self.assertNotEqual(first["bundle"], second["bundle"])
        self.assertEqual((Path(first["bundle"]) / desktop.BINARY).read_bytes(), b"selected-native-artifact")
        self.assertEqual(json.loads((self.lifecycle / "desktop.previous.json").read_text())["bundle"], first["bundle"])

    def test_symlink_cannot_escape_selected_bundle(self):
        (self.bundle / "Contents/alias").symlink_to(self.root)
        with self.assertRaises(ValueError):
            self.install()
        self.assertFalse((self.lifecycle / "desktop.json").exists())

    def test_existing_unowned_application_cannot_be_reported_as_ready(self):
        selected = self.install()["bundle"]
        # Same binary/PID without the matching launcher birth receipt is not our launch.
        with patch.object(desktop, "running", return_value=[{"pid": 42, "bundle": selected}]), \
                patch("private_stack_services.start") as start, patch.object(desktop.subprocess, "Popen") as spawn:
            with self.assertRaises(FileNotFoundError):
                desktop.open_app(self.installation)
            start.assert_not_called()
            spawn.assert_not_called()


if __name__ == "__main__":
    unittest.main()
