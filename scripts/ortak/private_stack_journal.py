"""Read-only SQLite integrity observation for a restored cold Hermes journal."""

import json
from contextlib import closing
import os
from pathlib import Path
import sqlite3
import stat
import tempfile
import time


def inspect(path):
    """Return bounded table counts only; never expose stored run inputs or credentials."""
    # SQLite may need a writable directory for WAL/SHM even with mode=ro.
    # Inspect a bounded inert copy, retaining the restored cold volume unchanged.
    with tempfile.TemporaryDirectory(prefix="ortak-journal-") as temporary:
        total = 0
        destination = Path(temporary) / "journal.sqlite"
        for suffix in ("", "-wal", "-shm"):
            source = Path(str(path) + suffix)
            if suffix and not source.exists():
                continue
            row = source.lstat()
            if not stat.S_ISREG(row.st_mode) or row.st_nlink != 1:
                raise ValueError("journal_copy_refused")
            total += row.st_size
            if total > 192 * 1024**2:
                raise ValueError("journal_copy_bound")
            with os.fdopen(os.open(source, os.O_RDONLY | os.O_NOFOLLOW), "rb") as incoming, \
                    open(str(destination) + suffix, "xb") as outgoing:
                copied = 0
                while block := incoming.read(65536):
                    copied += len(block)
                    if copied > row.st_size:
                        raise ValueError("journal_copy_changed")
                    outgoing.write(block)
                after = os.fstat(incoming.fileno())
                if copied != row.st_size or (row.st_ino, row.st_size, row.st_mtime_ns) != (after.st_ino, after.st_size, after.st_mtime_ns):
                    raise ValueError("journal_copy_changed")
        return inspect_copy(destination)


def inspect_copy(path):
    """Query only the inert copy, with SQLite extensions and unsafe schema functions disabled."""
    deadline = time.monotonic() + 5
    with closing(sqlite3.connect(f"file:{path}?mode=ro", uri=True, timeout=1)) as database:
        database.execute("PRAGMA query_only=ON")
        database.execute("PRAGMA trusted_schema=OFF")
        database.set_progress_handler(lambda: int(time.monotonic() >= deadline), 1000)
        if database.execute("PRAGMA quick_check").fetchall() != [("ok",)]:
            raise ValueError("journal_integrity_refused")
        if database.execute("PRAGMA foreign_key_check").fetchone() is not None:
            raise ValueError("journal_foreign_keys_refused")
        names = database.execute("SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' LIMIT 65").fetchall()
        if not 0 < len(names) <= 64:
            raise ValueError("journal_table_bound")
        counts = {}
        for (name,) in names:
            if not name.replace("_", "").isalnum():
                raise ValueError("journal_table_name_refused")
            counts[name] = database.execute('SELECT count(*) FROM "' + name + '"').fetchone()[0]
        if sum(counts.values()) > 100000:
            raise ValueError("journal_row_bound")
        return {"integrity": "ok", "foreign_keys": "ok", "tables": counts}


if __name__ == "__main__":
    try:
        print(json.dumps(inspect("/journal/journal.sqlite")))
    except Exception:
        raise SystemExit("restored_journal_refused") from None
