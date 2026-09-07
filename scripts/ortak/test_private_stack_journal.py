"""The real restored-journal reader handles cold SQLite/WAL without changing its source."""

import hashlib
from pathlib import Path
import sqlite3
import tempfile
import unittest

import private_stack_journal as journal


class JournalTests(unittest.TestCase):
    def test_committed_wal_is_read_without_touching_source_files(self):
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "journal.sqlite"
            connection = sqlite3.connect(path)
            try:
                connection.execute("PRAGMA journal_mode=WAL")
                connection.execute("CREATE TABLE runs(id INTEGER PRIMARY KEY)")
                connection.execute("INSERT INTO runs VALUES(1)")
                connection.commit()
                files = lambda: {p.name: hashlib.sha256(p.read_bytes()).hexdigest() for p in Path(temporary).iterdir()}
                before = files()
                self.assertIn("journal.sqlite-wal", before)
                self.assertEqual(journal.inspect(path), {"integrity": "ok", "foreign_keys": "ok", "tables": {"runs": 1}})
                self.assertEqual(files(), before)
            finally:
                connection.close()

    def test_corrupt_database_does_not_become_empty_success(self):
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "journal.sqlite"
            path.write_bytes(b"invalid database")
            with self.assertRaises(sqlite3.DatabaseError):
                journal.inspect(path)

    def test_missing_database_is_not_created(self):
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "journal.sqlite"
            with self.assertRaises(FileNotFoundError):
                journal.inspect(path)
            self.assertFalse(path.exists())


if __name__ == "__main__":
    unittest.main()
