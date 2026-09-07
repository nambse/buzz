"""Cold archive roundtrips and refusal paths against the actual archive seam."""

import io
import os
from pathlib import Path
import tempfile
import unittest

import private_stack_archive as capture
import recovery_archive_io as restore


class ColdArchiveTests(unittest.TestCase):
    def test_precise_timestamps_empty_directories_and_bytes_survive(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            source, target = root / "source", root / "target"
            source.mkdir(mode=0o700)
            target.mkdir(mode=0o700)
            (source / "empty").mkdir(mode=0o700)
            (source / "data").write_bytes(b"private fixture\0bytes")
            os.utime(source / "data", ns=(1788750000123456789, 1788750000123456789))
            outgoing = io.BytesIO()
            capture.capture(outgoing, [(source, ".")], 4096, linux_metadata=False)
            outgoing.seek(0)
            receipt = restore.archive(outgoing, 4096, target)
            self.assertEqual((target / "data").read_bytes(), (source / "data").read_bytes())
            self.assertEqual((target / "data").stat().st_mtime_ns, (source / "data").stat().st_mtime_ns)
            self.assertTrue((target / "empty").is_dir())
            self.assertEqual(receipt["files"], 1)

    def test_links_and_payload_over_budget_refuse(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "data").write_bytes(b"123456789")
            with self.assertRaises(ValueError):
                capture.capture(io.BytesIO(), [(root / "data", "data")], 8, linux_metadata=False)
            (root / "link").symlink_to(root / "data")
            with self.assertRaises(ValueError):
                capture.capture(io.BytesIO(), [(root / "link", "link")], 4096, linux_metadata=False)

    def test_duplicate_and_traversal_members_refuse(self):
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory) / "data"
            source.write_text("x")
            for entries in [[(source, "../escape")], [(source, "data"), (source, "data")]]:
                with self.assertRaises(ValueError):
                    capture.capture(io.BytesIO(), entries, 4096, linux_metadata=False)


if __name__ == "__main__":
    unittest.main()
