"""Bounded cold-tree archive creation, shared with inert recovery verification."""

from decimal import Decimal
import os
from pathlib import Path
import stat
import sys
import tarfile
import time

import recovery_archive_io as archive_io


def stamp(row):
    """Detect file replacement or content/metadata changes while reading."""
    return (row.st_dev, row.st_ino, row.st_size, row.st_mtime_ns, row.st_ctime_ns,
            row.st_mode, row.st_uid, row.st_gid, row.st_nlink)


def capture(stream, entries, maximum, *, linux_metadata=True, seconds=120):
    """Archive explicit cold sources only; links and changing files fail closed."""
    deadline = time.monotonic() + seconds
    pending = list(reversed(entries))
    seen = set()
    total = 0
    with tarfile.open(fileobj=stream, mode="w|", format=tarfile.PAX_FORMAT) as outgoing:
        while pending:
            path, name = pending.pop()
            row = path.lstat()
            directory = stat.S_ISDIR(row.st_mode)
            archive_io.name(name, directory)
            archive_io.require(name not in seen and len(seen) < archive_io.MAX_FILES
                               and time.monotonic() < deadline and not stat.S_IMODE(row.st_mode) & ~0o777
                               and (directory or stat.S_ISREG(row.st_mode) and row.st_nlink == 1))
            seen.add(name)
            info = tarfile.TarInfo(name)
            info.mode, info.uid, info.gid = stat.S_IMODE(row.st_mode), row.st_uid, row.st_gid
            info.mtime = row.st_mtime_ns // 10**9
            info.pax_headers["mtime"] = str(Decimal(row.st_mtime_ns) / Decimal(10**9))
            metadata = archive_io.xattrs(path) if linux_metadata else {}
            info.pax_headers.update({f"ORTAK.xattr.{key}": value for key, value in metadata.items()})
            if directory:
                info.type = tarfile.DIRTYPE
                outgoing.addfile(info)
                children = sorted(path.iterdir(), reverse=True)
                archive_io.require(len(children) + len(seen) + len(pending) <= archive_io.MAX_FILES)
                pending.extend((child, str(Path(name) / child.name)) for child in children)
            else:
                total += row.st_size
                archive_io.require(total <= maximum)
                info.size = row.st_size
                with os.fdopen(os.open(path, os.O_RDONLY | os.O_NOFOLLOW), "rb") as incoming:
                    archive_io.require(stamp(os.fstat(incoming.fileno())) == stamp(row))
                    outgoing.addfile(info, incoming)
                    archive_io.require(stamp(os.fstat(incoming.fileno())) == stamp(row))
            archive_io.require(stamp(path.lstat()) == stamp(row))
            if linux_metadata:
                archive_io.require(archive_io.xattrs(path) == metadata)
    return {"entries": len(seen), "payload_bytes": total}


def main():
    """The helper has one fixed cold source mount and no network/controller imports."""
    archive_io.require(sys.platform == "linux" and not Path("/var/run/docker.sock").exists())
    maximum = int(sys.argv[1])
    archive_io.require(0 < maximum <= 2 * 1024**3)
    capture(sys.stdout.buffer, [(Path("/capture-source"), ".")], maximum)


if __name__ == "__main__":
    try:
        main()
    except Exception:
        raise SystemExit("cold_archive_refused") from None
