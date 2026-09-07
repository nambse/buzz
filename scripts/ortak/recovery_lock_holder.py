"""Standard-library-only Linux lease process; no provider, application import or Docker socket."""

from contextlib import contextmanager
import base64
import fcntl
import hashlib
import json
import os
from pathlib import Path
import selectors
import signal
import sqlite3
import stat
import sys
import tempfile
import time

import recovery_journal_archive

RUNTIME = Path('/private/tmp/ortak-hermes-v0-private-20260905')
LOCKS = ('state/executor.lock', 'oauth/ada-private/oauth.lock')


def require(condition):
    """Every error is a fixed code; no journal, credential or rejected input is printed."""
    if not condition:
        raise ValueError('recovery_lease_refused')


@contextmanager
def held_locks(root):
    """Open existing exact lock files only; hold both Linux flock leases until context exit."""
    descriptors = []
    try:
        for relative in LOCKS:
            path = root / relative
            require(root.resolve() == root)
            # Only state/ and oauth/ are source mounts. Their common parent is
            # an image-created directory, not source ownership authority.
            for parent in [root.joinpath(*Path(relative).parts[:n]) for n in range(1, len(Path(relative).parts))]:
                row = parent.lstat()
                require(stat.S_ISDIR(row.st_mode) and row.st_uid == os.getuid() and not row.st_mode & 0o077)
            descriptor = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
            descriptors.append(descriptor)
            row = os.fstat(descriptor)
            require(stat.S_ISREG(row.st_mode) and row.st_uid == os.getuid() and row.st_nlink == 1
                    and stat.S_IMODE(row.st_mode) == 0o600 and row.st_size == 0)
            fcntl.flock(descriptor, fcntl.LOCK_EX | fcntl.LOCK_NB)
            require(path.lstat().st_ino == row.st_ino)
        yield [{'path': str(root / relative), 'inode': os.fstat(fd).st_ino}
               for relative, fd in zip(LOCKS, descriptors)]
        for relative, fd in zip(LOCKS, descriptors):
            require((root / relative).lstat().st_ino == os.fstat(fd).st_ino)
    finally:
        for descriptor in reversed(descriptors):
            os.close(descriptor)


@contextmanager
def cold_journal(root, working_parent=None):
    """Stage a bounded cold main/WAL pair so SQLite can create SHM without source write access."""
    with cold_journal_file(root / 'state/journal.sqlite', working_parent) as path:
        yield path


@contextmanager
def cold_journal_file(path, working_parent=None):
    """Only a caller holding the writer barrier may stage these fixed cold file generations."""
    sources = [path]
    wal = Path(str(path) + '-wal')
    if wal.exists(): sources.append(wal)
    before = {}
    def identity(row):
        return row.st_dev, row.st_ino, row.st_size, row.st_mtime_ns, row.st_mode
    with tempfile.TemporaryDirectory(prefix='recovery-journal-', dir=working_parent) as directory:
        for source in sources:
            row = source.lstat()
            require(stat.S_ISREG(row.st_mode) and row.st_uid == os.getuid() and row.st_nlink == 1
                    and stat.S_IMODE(row.st_mode) == 0o600 and row.st_size <= 64 * 1024**2)
            before[source] = identity(row)
            descriptor = os.open(source, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
            target = Path(directory) / source.name
            target_descriptor = os.open(target, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
            with os.fdopen(descriptor, 'rb') as incoming, os.fdopen(target_descriptor, 'wb') as outgoing:
                require(identity(os.fstat(incoming.fileno())) == before[source])
                size = 0
                while block := incoming.read(65536):
                    size += len(block); require(size <= 64 * 1024**2)
                    outgoing.write(block)
                require(size == row.st_size)
            require(identity(source.lstat()) == before[source])
        yield Path(directory) / path.name
        require(wal.exists() == (wal in sources))
        require(all(identity(source.lstat()) == before[source] for source in sources))


def journal_status(root, working_parent=None, *, confidential_reviewed=False):
    """Read cold counters with SQLite's WAL handling; no Journal() or immutable-mode shortcut."""
    with cold_journal(root, working_parent) as path:
        return staged_journal_status(path, confidential_reviewed=confidential_reviewed)


def staged_journal_status(path, *, confidential_reviewed=False):
    """Only a private working copy permits missing WAL/SHM creation; all SQL remains query-only."""
    database = sqlite3.connect(path.as_uri() + '?mode=rw', uri=True, timeout=2)
    deadline = time.monotonic() + 5
    database.set_progress_handler(lambda: time.monotonic() >= deadline, 1000)
    try:
        database.execute('PRAGMA query_only=ON')
        require(type(confidential_reviewed) is bool)
        database.execute('BEGIN')
        confidential = {r[0] for r in database.execute(
            "SELECT name FROM sqlite_master WHERE type='table' AND name GLOB 'confidential_*'")}
        require(not confidential if not confidential_reviewed else confidential == {
            'confidential_runs', 'confidential_events', 'confidential_status'})
        protected = None
        if confidential_reviewed:
            # Only an explicit source-bound capture/restore selection enables this branch.
            import recovery_confidential_journal
            protected = recovery_confidential_journal.observe(database, deadline)
        count = database.execute('SELECT count(*) FROM runs').fetchone()[0]
        require(count <= 100000)
        active = database.execute("SELECT count(*) FROM runs WHERE status NOT IN ('completed','failed','cancelled')").fetchone()[0]
        cursor_sql = '''SELECT count(*) FROM runs r LEFT JOIN (
            SELECT start_key,count(*) n,min(sequence) first,max(sequence) last FROM events GROUP BY start_key
            ) e ON e.start_key=r.start_key WHERE COALESCE(e.n,0)>512 OR
            (e.n IS NOT NULL AND (e.first<>1 OR e.last<>r.sequence OR e.n<>r.sequence)) OR
            (e.n IS NULL AND r.sequence<>0)'''
        if confidential_reviewed:
            cursor_sql = cursor_sql.replace('FROM runs r LEFT JOIN',
                'FROM (SELECT * FROM runs ordinary WHERE NOT EXISTS '
                '(SELECT 1 FROM confidential_runs c WHERE c.start_key=ordinary.start_key)) r LEFT JOIN')
        malformed = database.execute(cursor_sql).fetchone()[0]
        require(active == 0 and malformed == 0)
        result = {'runs': count, 'nonterminal': active, 'invalid_cursors': malformed}
        if protected is not None:
            result['confidential'] = protected
        names = {r[0] for r in database.execute("SELECT name FROM sqlite_master WHERE type='table' AND name IN ('workspace_runs','workspace_tool_calls')")}
        if names:
            require(names == {'workspace_runs','workspace_tool_calls'})
            pending = database.execute("SELECT count(*) FROM workspace_tool_calls WHERE state IN ('pending','resolved') OR result_json IS NOT NULL").fetchone()[0]
            invalid = database.execute("""SELECT count(*) FROM workspace_tool_calls t
                LEFT JOIN workspace_runs w USING(start_key) LEFT JOIN runs r USING(start_key)
                WHERE w.start_key IS NULL OR r.start_key IS NULL
                    OR t.state NOT IN ('consumed','interrupted')""").fetchone()[0]
            invalid += database.execute("SELECT count(*) FROM workspace_runs w LEFT JOIN runs r USING(start_key) WHERE r.start_key IS NULL").fetchone()[0]
            require(pending == 0 and invalid == 0)
            hashed, counts = hashlib.sha256(), {}
            for table, order, maximum in [('workspace_runs','start_key',100000),
                                          ('workspace_tool_calls','start_key,ordinal',400000)]:
                counts[table] = database.execute('SELECT count(*) FROM ' + table).fetchone()[0]
                require(counts[table] <= maximum)
                hashed.update(table.encode() + b'\n')
                for row in database.execute('SELECT * FROM ' + table + ' ORDER BY ' + order):
                    require(time.monotonic() < deadline)
                    encoded=json.dumps(row,ensure_ascii=False,separators=(',',':'),allow_nan=False).encode()
                    require(len(encoded)<=32768)
                    hashed.update(encoded + b'\n')
            result['workspace'] = {**counts,'pending':pending,'invalid':invalid,'sha256':hashed.hexdigest()}
        return result
    finally:
        database.close()


def serve(root, working, incoming, outgoing, *, confidential_reviewed=False):
    """Status and raw export stay inside the same live lock context, with finite RPC count."""
    def emit(value):
        outgoing.write(json.dumps(value,separators=(',',':')).encode()+b'\n');outgoing.flush()
    class Chunks:
        def __init__(self):self.count=0;self.hashed=hashlib.sha256()
        def write(self, data):
            for start in range(0,len(data),3072):
                block=data[start:start+3072];self.count+=len(block)
                require(self.count<=recovery_journal_archive.MAX_ARCHIVE_BYTES)
                self.hashed.update(block);emit({'chunk':base64.b64encode(block).decode()})
            return len(data)
        def flush(self):outgoing.flush()
    deadline=time.monotonic()+900
    with held_locks(root) as locks:
        journal=journal_status(root,working,confidential_reviewed=confidential_reviewed)
        emit({'status':'held','locks':locks,'journal':journal})
        with selectors.DefaultSelector() as ready:
            ready.register(incoming,selectors.EVENT_READ)
            for _ in range(64):
                left=deadline-time.monotonic()
                require(left>0 and ready.select(left))
                action=incoming.readline(32)
                require(action in (b'release\n',b'journal-status\n',b'journal-archive\n'))
                require(journal_status(root,working,confidential_reviewed=confidential_reviewed)==journal)
                if action==b'release\n':break
                if action==b'journal-status\n':emit({'status':'journal','journal':journal})
                else:
                    stream=Chunks()
                    metadata=recovery_journal_archive.write(root/'state',stream,uid=os.getuid())
                    require(journal_status(root,working,confidential_reviewed=confidential_reviewed)==journal)
                    emit({'status':'archive','bytes':stream.count,'sha256':stream.hashed.hexdigest(),'archive':metadata})
            else: require(False)
    emit({'status':'released'})


def main():
    """Hold for at most900 seconds, including blocked RPC I/O; never leave an unbounded lease."""
    require(sys.platform == 'linux' and os.getuid() == 10001)
    require(not Path('/var/run/docker.sock').exists())
    working = Path('/recovery-working')
    row = working.lstat()
    require(stat.S_ISDIR(row.st_mode) and row.st_uid == os.getuid() and not row.st_mode & 0o077)
    signal.alarm(900)
    serve(RUNTIME,working,sys.stdin.buffer,sys.stdout.buffer,
        confidential_reviewed=globals().get('RECOVERY_CONFIDENTIAL_REVIEWED',False))


if __name__ == '__main__':
    try:
        main()
    except (OSError, ValueError, sqlite3.Error):
        print('{"status":"refused"}', flush=True)
        raise SystemExit(1) from None
