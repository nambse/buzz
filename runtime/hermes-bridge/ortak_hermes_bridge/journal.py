"""Durable execution registry. Storage errors propagate, without memory fallback."""
import hashlib
import json
import re
import sqlite3
from contextlib import contextmanager
from datetime import datetime, timezone
import time
from pathlib import Path
from uuid import UUID
from .failure_diagnostics import validate_diagnostic

TERMINAL = {'completed', 'failed', 'cancelled'}
MAX_RUNS = 100_000
MAX_EVENTS = 512

class BridgeError(Exception):
    """A secret-free public error code."""
    def __init__(self, code, status=400):
        super().__init__(code)
        self.code, self.status = code, status

def identity(key):
    """Parse the control plane's canonical start key."""
    try:
        prefix, company, run = key.split(':')
        if prefix != 'ortak-run' or str(UUID(company)) != company or str(UUID(run)) != run:
            raise ValueError()
    except (AttributeError, TypeError, ValueError):
        raise BridgeError('invalid_start_key') from None
    return company, run

def reference(key):
    """Return the stable public runtime reference."""
    company, run = identity(key)
    return f'ortak:{company}:{run}'

def start_key(ref):
    """Validate and reverse the runtime reference, never accepting aliases."""
    if not isinstance(ref, str) or not ref.startswith('ortak:'):
        raise BridgeError('invalid_runtime_ref')
    key = 'ortak-run:' + ref[len('ortak:'):]
    identity(key)
    return key

def now():
    """RFC3339 timestamp."""
    return datetime.now(timezone.utc).isoformat().replace('+00:00', 'Z')

def redact(text, secrets=()):
    """Redact entire output before persistence, including resolver-supplied literals."""
    for secret in sorted(secrets, key=len, reverse=True):
        if secret:
            text = text.replace(secret, '[redacted]')
    text = re.sub(r'-----BEGIN[\s\S]*?(?:-----END[^\n]*|$)', '[redacted]', text)
    text = re.sub(r'(?i)\b(?:sk-|nsec1|ghp_|github_pat_|xox[bpa]-)[^\s,;}]+', '[redacted]', text)
    text = re.sub(r'(?i)bearer\s+[^\s,;}]+', '[redacted]', text)
    text = re.sub(r'(?i)(?:api[_-]?key|token|password|secret|authorization)["\']?\s*[:=]\s*(?:"[^"]*"|\'[^\']*\'|[^\s,;}]+)', '[redacted]', text)
    return text

class Journal:
    """Append-only ordered events and permanent start/cancellation identities."""
    def __init__(self, path):
        self.path = str(path)
        Path(path).parent.mkdir(parents=True, exist_ok=True, mode=0o700)
        with self.connection() as db:
            db.executescript('''
                CREATE TABLE IF NOT EXISTS runs (
                    start_key TEXT PRIMARY KEY, fingerprint TEXT,
                    status TEXT NOT NULL, started_at TEXT NOT NULL,
                    sequence INTEGER NOT NULL DEFAULT 0
                );
                CREATE TABLE IF NOT EXISTS profile_probes (
                    start_key TEXT PRIMARY KEY REFERENCES runs(start_key),
                    selection TEXT NOT NULL
                );
                CREATE INDEX IF NOT EXISTS profile_probe_selection ON profile_probes(selection);
                CREATE TABLE IF NOT EXISTS events (
                    start_key TEXT NOT NULL REFERENCES runs(start_key),
                    sequence INTEGER NOT NULL, occurred_at TEXT NOT NULL,
                    payload TEXT NOT NULL,
                    PRIMARY KEY(start_key,sequence)
                );
                CREATE TABLE IF NOT EXISTS private_failure_diagnostics (
                    start_key TEXT PRIMARY KEY REFERENCES runs(start_key),
                    recorded_at TEXT NOT NULL,
                    diagnostic TEXT NOT NULL CHECK(length(diagnostic)<=2048)
                );
            ''')
            from .journal_tools import SCHEMA
            db.executescript(SCHEMA)
            from .confidential.journal import install
            install(db)

    @contextmanager
    def connection(self, timeout=3):
        """Fully durable storage with bounded lock wait; connections always close."""
        db = sqlite3.connect(self.path, timeout=timeout, isolation_level=None)
        db.row_factory = sqlite3.Row
        try:
            db.execute('PRAGMA journal_mode=WAL')
            db.execute('PRAGMA synchronous=FULL')
            db.execute('PRAGMA foreign_keys=ON')
            yield db
        finally:
            db.close()

    @contextmanager
    def transaction(self, timeout=3):
        """Registry and event transitions commit as one atomic operation."""
        with self.connection(timeout) as db:
            db.execute('BEGIN IMMEDIATE')
            try:
                yield db
                db.commit()
            except BaseException:
                db.rollback()
                raise

    @staticmethod
    def receipt(row):
        """Expose no raw input or provider credentials."""
        return {'runtime_run_ref': reference(row['start_key']),
                'started_at': row['started_at'], 'status': row['status']}

    def lookup(self, key):
        """Read only: lookup never admits, starts or restarts work."""
        identity(key)
        with self.connection() as db:
            row = db.execute('SELECT * FROM runs WHERE start_key=?', (key,)).fetchone()
            return self.receipt(row) if row else None

    def has_start(self, key):
        """Distinguish an admitted start identity from a cancellation-only tombstone."""
        identity(key)
        with self.connection() as db:
            row = db.execute('SELECT fingerprint FROM runs WHERE start_key=?', (key,)).fetchone()
            return row is not None and row[0] is not None

    def reserve(self, spec, *, probe_selection=None, workspace=None):
        """Persist execution identity before an executor may be invoked."""
        key = spec['idempotency_key']
        identity(key)
        # Preserve the original empty-policy fingerprint; workspace starts bind
        # the separate explicit grant as well as every immutable RunSpec field.
        pinned = spec if workspace is None else {'spec': spec, 'workspace': workspace}
        digest = hashlib.sha256(json.dumps(pinned, sort_keys=True, separators=(',', ':')).encode()).hexdigest()
        selection = json.dumps(probe_selection, sort_keys=True, separators=(',', ':')) if probe_selection is not None else None
        with self.transaction() as db:
            from .confidential.journal import is_confidential_on
            if is_confidential_on(db, key):
                raise BridgeError('start_conflict', 409)
            row = db.execute('SELECT * FROM runs WHERE start_key=?', (key,)).fetchone()
            if row:
                if selection is not None:
                    probe = db.execute('SELECT selection FROM profile_probes WHERE start_key=?', (key,)).fetchone()
                    if probe is None or probe[0] != selection:
                        raise BridgeError('probe_conflict', 409)
                if row['fingerprint'] is not None and row['fingerprint'] != digest:
                    raise BridgeError('start_conflict', 409)
                return self.receipt(row), False
            if db.execute('SELECT count(*) FROM runs').fetchone()[0] >= MAX_RUNS:
                raise BridgeError('journal_capacity', 503)
            db.execute('INSERT INTO runs VALUES (?,?,?,?,0)', (key, digest, 'accepted', now()))
            if workspace is not None:
                from .workspace_contract import canonical
                db.execute('INSERT INTO workspace_runs VALUES (?,?)', (key, canonical(workspace)))
            if selection is not None:
                db.execute('INSERT INTO profile_probes VALUES (?,?)', (key, selection))
            row = db.execute('SELECT * FROM runs WHERE start_key=?', (key,)).fetchone()
            return self.receipt(row), True

    def recent_profile_probe(self, selection, ttl_seconds=120):
        """Return only a completed explicit probe bound to the current exact selection."""
        encoded = json.dumps(selection, sort_keys=True, separators=(',', ':'))
        with self.connection() as db:
            row = db.execute("""
                SELECT r.start_key,e.occurred_at,e.payload FROM profile_probes p
                JOIN runs r ON r.start_key=p.start_key
                JOIN events e ON e.start_key=r.start_key AND e.sequence=r.sequence
                WHERE p.selection=? AND r.status='completed'
                ORDER BY e.occurred_at DESC LIMIT 1
            """, (encoded,)).fetchone()
        if row is None or json.loads(row['payload']).get('event_type') != 'run.completed':
            return None
        age = time.time() - datetime.fromisoformat(row['occurred_at'].replace('Z', '+00:00')).timestamp()
        return row['start_key'] if 0 <= age < ttl_seconds else None

    @staticmethod
    def append(db, key, payload):
        """Internal append under the same transaction as the state change."""
        seq = db.execute('SELECT sequence FROM runs WHERE start_key=?', (key,)).fetchone()[0] + 1
        if seq > MAX_EVENTS:
            raise BridgeError('event_capacity', 503)
        encoded = json.dumps(payload, separators=(',', ':'))
        if len(encoded.encode()) > 32 * 1024:
            raise BridgeError('event_capacity', 503)
        db.execute('INSERT INTO events VALUES (?,?,?,?)', (key, seq, now(), encoded))
        db.execute('UPDATE runs SET sequence=? WHERE start_key=?', (seq, key))

    def begin_execution(self, key):
        """Linearize execution against a prior cancellation before provider I/O."""
        with self.transaction() as db:
            row = db.execute('SELECT status FROM runs WHERE start_key=?', (key,)).fetchone()
            if not row or row[0] != 'accepted':
                return False
            self.append(db, key, {'event_type': 'run.started', 'runtime_run_ref': reference(key)})
            db.execute("UPDATE runs SET status='running' WHERE start_key=?", (key,))
            return True

    def complete(self, key, text, secrets=(), *, work_output=False):
        """Store a previously redacted bounded reply and terminal state atomically."""
        if type(work_output) is not bool or not isinstance(text, str) or len(text.encode()) > 8192 or any(ord(c) < 32 and c not in '\n\r\t' for c in text):
            raise BridgeError('invalid_output')
        text = redact(text, secrets)
        with self.transaction() as db:
            row = db.execute('SELECT status FROM runs WHERE start_key=?', (key,)).fetchone()
            if not row or row[0] != 'running':
                return False
            from .journal_tools import settled_on
            if not settled_on(db, key):
                raise BridgeError('unsettled_workspace_tool', 409)
            intent = 'reply' if text.strip() and not work_output else 'silent'
            if text.strip():
                self.append(db, key, {'event_type': 'assistant.delta', 'turn': 0, 'delta': {'text': text}})
            self.append(db, key, {'event_type': 'delivery.intent', 'intent': intent})
            self.append(db, key, {'event_type': 'run.completed', 'delivery_intent': intent})
            db.execute("UPDATE runs SET status='completed' WHERE start_key=?", (key,))
            return True

    def fail(self, key, code='executor_interrupted', *, diagnostic=None):
        """Only closed failure codes reach persistence, never provider exceptions."""
        if code not in {'executor_interrupted', 'executor_unavailable', 'policy_denied', 'provider_failed', 'deadline_exceeded',
                        'provider_incomplete', 'provider_response_invalid', 'invalid_output', 'credential_denied',
                        'runtime_selection_changed', 'unsupported_hermes_tool_selection', 'workspace_tool_failed'}:
            raise BridgeError('invalid_failure_code')
        if diagnostic is not None and not validate_diagnostic(diagnostic):
            raise BridgeError('invalid_failure_diagnostic')
        with self.transaction() as db:
            from .confidential.journal import is_confidential_on, fail_on
            if is_confidential_on(db, key):
                fail_on(db, key, code)
                return
            row = db.execute('SELECT status FROM runs WHERE start_key=?', (key,)).fetchone()
            if row and row[0] not in TERMINAL and row[0] != 'cancelling':
                from .journal_tools import retire
                retire(self, db, key, 'deadline_exceeded' if code == 'deadline_exceeded' else 'workspace_unavailable')
                if diagnostic is not None:
                    db.execute('INSERT INTO private_failure_diagnostics VALUES (?,?,?)',
                               (key, now(), json.dumps(diagnostic, separators=(',', ':'))))
                self.append(db, key, {'event_type': 'run.failed', 'code': code, 'message': {'text': code}})
                db.execute("UPDATE runs SET status='failed' WHERE start_key=?", (key,))

    def request_cancel(self, key):
        """Persist an irreversible tombstone even when a delayed start has not arrived."""
        identity(key)
        with self.transaction() as db:
            row = db.execute('SELECT status FROM runs WHERE start_key=?', (key,)).fetchone()
            if row and row[0] in TERMINAL:
                return 'already_terminal'
            if row is None:
                if db.execute('SELECT count(*) FROM runs').fetchone()[0] >= MAX_RUNS:
                    raise BridgeError('journal_capacity', 503)
                db.execute('INSERT INTO runs VALUES (?,?,?,?,0)', (key, None, 'cancelling', now()))
            else:
                from .journal_tools import retire
                retire(self, db, key, 'cancelled')
                db.execute("UPDATE runs SET status='cancelling' WHERE start_key=?", (key,))
            return 'cancelled'

    def finish_cancel(self, key):
        """Call only after the execution owner has proven containment and reaped work."""
        with self.transaction() as db:
            from .confidential.journal import is_confidential_on, finish_cancel_on
            if is_confidential_on(db, key):
                finish_cancel_on(db, key)
                return
            row = db.execute('SELECT status FROM runs WHERE start_key=?', (key,)).fetchone()
            if row and row[0] == 'cancelling':
                self.append(db, key, {'event_type': 'run.cancelled', 'reason': {'text': 'cancelled by control plane'}})
                db.execute("UPDATE runs SET status='cancelled' WHERE start_key=?", (key,))

    def unsettled(self):
        """Return recovery work; entries must never be blindly executed again."""
        with self.connection() as db:
            return [(r[0], r[1]) for r in db.execute("SELECT start_key,status FROM runs WHERE status NOT IN ('completed','failed','cancelled')")]

    def recover(self, assert_stopped):
        """Require positive containment evidence before sealing interrupted work."""
        for key, status in self.unsettled():
            if not assert_stopped(key):
                raise BridgeError('execution_owner_not_stopped', 503)
            if status == 'cancelling':
                self.finish_cancel(key)
            else:
                self.fail(key)

    def events(self, key, after=0, limit=100):
        """Replay exclusively after cursor; terminal=true only on the final page."""
        identity(key)
        if type(after) is not int or after < 0 or type(limit) is not int or not 1 <= limit <= 100:
            raise BridgeError('invalid_cursor')
        with self.connection() as db:
            db.execute('BEGIN')
            from .confidential.journal import is_confidential_on
            if is_confidential_on(db, key):
                raise BridgeError('run_not_found', 404)
            row = db.execute('SELECT * FROM runs WHERE start_key=?', (key,)).fetchone()
            if row is None:
                raise BridgeError('run_not_found', 404)
            if after > row['sequence']:
                raise BridgeError('cursor_ahead', 409)
            rows = db.execute('SELECT * FROM events WHERE start_key=? AND sequence>? ORDER BY sequence LIMIT ?', (key, after, limit)).fetchall()
            result = [{'cursor': str(r['sequence']), 'occurred_at': r['occurred_at'], 'payload': json.loads(r['payload'])} for r in rows]
            last = rows[-1]['sequence'] if rows else after
            return {'events': result, 'terminal': row['status'] in TERMINAL and last == row['sequence']}
