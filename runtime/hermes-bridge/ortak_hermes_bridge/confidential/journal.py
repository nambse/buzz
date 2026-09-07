"""Ciphertext journal sharing only start/status ownership with ordinary runs."""
import hashlib
import json
from pathlib import Path
from ..journal import BridgeError, MAX_RUNS, TERMINAL, identity, now, redact, reference
from .request import canonical_inner
from .wire import ConfidentialEnvelope


def install(db):
    """Idempotent explicit versioned schema; no old run mode is inferred."""
    db.executescript(Path(__file__).with_name('schema.sql').read_text())


def is_confidential_on(db, key):
    return db.execute('SELECT 1 FROM confidential_runs WHERE start_key=?', (key,)).fetchone() is not None


def fail_on(db, key, code):
    """Closed receipt settlement needs no content key, including after restart."""
    row = db.execute('SELECT status FROM runs WHERE start_key=?', (key,)).fetchone()
    if row and row[0] not in TERMINAL and row[0] != 'cancelling':
        db.execute('INSERT INTO confidential_status VALUES (?,?,?)', (key, code, now()))
        db.execute("UPDATE runs SET status='failed' WHERE start_key=?", (key,))


def finish_cancel_on(db, key):
    row = db.execute('SELECT status FROM runs WHERE start_key=?', (key,)).fetchone()
    if row and row[0] == 'cancelling':
        db.execute('INSERT INTO confidential_status VALUES (?,?,?)', (key, 'cancelled', now()))
        db.execute("UPDATE runs SET status='cancelled' WHERE start_key=?", (key,))


def reserve(journal, request):
    """Bind the encrypted snapshot, excluding keys, before any process launch."""
    key = request.key
    snapshot = request.snapshot.canonical_bytes
    fingerprint = hashlib.sha256(snapshot).hexdigest()
    with journal.transaction() as db:
        row = db.execute('SELECT * FROM runs WHERE start_key=?', (key,)).fetchone()
        if row:
            saved = db.execute('SELECT snapshot FROM confidential_runs WHERE start_key=?', (key,)).fetchone()
            if row['fingerprint'] is None and saved is None:
                # A cancellation preceding either mode's start is authoritative.
                return journal.receipt(row), False
            if saved is None or row['fingerprint'] != fingerprint or saved[0] != snapshot:
                raise BridgeError('start_conflict', 409)
            return journal.receipt(row), False
        if db.execute('SELECT count(*) FROM runs').fetchone()[0] >= MAX_RUNS:
            raise BridgeError('journal_capacity', 503)
        db.execute('INSERT INTO runs VALUES (?,?,?,?,0)', (key, fingerprint, 'accepted', now()))
        db.execute('INSERT INTO confidential_runs VALUES (?,?,?,?,?,?)',
                   (key, 'ortak-confidential-journal/1', request.identity.canonical_bytes, snapshot,
                    request.claims['key_id'], request.snapshot.nonce))
        return journal.receipt(db.execute('SELECT * FROM runs WHERE start_key=?', (key,)).fetchone()), True


def require_mode(journal, key):
    """Metadata-only lookup/cancel accepts a pre-start cancellation tombstone."""
    identity(key)
    with journal.connection() as db:
        row = db.execute('SELECT fingerprint FROM runs WHERE start_key=?', (key,)).fetchone()
        if row is not None and row[0] is not None and not is_confidential_on(db, key):
            raise BridgeError('run_not_found', 404)


def events(journal, key, after=0, limit=4):
    """Replay exact stored envelopes and closed status metadata, without keys."""
    identity(key)
    if type(after) is not int or after < 0 or type(limit) is not int or not 1 <= limit <= 4:
        raise BridgeError('invalid_cursor')
    with journal.connection() as db:
        db.execute('BEGIN')
        row = db.execute('SELECT * FROM runs WHERE start_key=?', (key,)).fetchone()
        if row is None or not is_confidential_on(db, key):
            raise BridgeError('run_not_found', 404)
        if after > row['sequence']: raise BridgeError('cursor_ahead', 409)
        stored = db.execute('SELECT * FROM confidential_events WHERE start_key=? AND sequence>? ORDER BY sequence LIMIT ?',
                            (key, after, limit)).fetchall()
        items = []
        for event in stored:
            envelope = ConfidentialEnvelope.parse(event['envelope'])
            items.append({'cursor': str(event['sequence']), 'occurred_at': event['occurred_at'],
                          'envelope': json.loads(envelope.canonical_bytes)})
        status = db.execute('SELECT code,occurred_at FROM confidential_status WHERE start_key=?', (key,)).fetchone()
        last = stored[-1]['sequence'] if stored else after
        return {'events': items, 'status': row['status'], 'failure': dict(status) if status else None,
                'terminal': row['status'] in TERMINAL and last == row['sequence']}


class ExecutionJournal:
    """Existing toolless executor seam with an exclusive protected event sink."""
    def __init__(self, journal, request):
        self._journal, self._request = journal, request
        self.path = journal.path

    def connection(self, *args, **kwargs): return self._journal.connection(*args, **kwargs)
    def lookup(self, key):
        self._same(key)
        return self._journal.lookup(key)
    def fail(self, key, code='executor_interrupted', *, diagnostic=None):
        self._same(key)
        # Provider diagnostics are never copied to the confidential journal.
        self._journal.fail(key, code)

    def _same(self, key):
        if key != self._request.key: raise BridgeError('run_not_found', 404)

    def _append(self, db, key, payload):
        seq = db.execute('SELECT sequence FROM runs WHERE start_key=?', (key,)).fetchone()[0] + 1
        at = now()
        inner = {'format': 'ortak-confidential-event/1', 'identity': self._request.claims,
                 'sequence': seq, 'occurred_at': at, 'payload': payload}
        envelope = self._request.event_key.seal_event(seq, canonical_inner(inner, 32 * 1024))
        db.execute('INSERT INTO confidential_events VALUES (?,?,?,?,?,?)',
                   (key, seq, at, envelope.canonical_bytes, self._request.claims['key_id'], envelope.nonce))

    def begin_execution(self, key):
        self._same(key)
        with self._journal.transaction() as db:
            row = db.execute('SELECT r.status,c.snapshot FROM runs r JOIN confidential_runs c USING(start_key) WHERE start_key=?', (key,)).fetchone()
            if row is None or row['status'] != 'accepted': return False
            if row['snapshot'] != self._request.snapshot.canonical_bytes:
                raise BridgeError('start_conflict', 409)
            self._append(db, key, {'event_type': 'run.started', 'runtime_run_ref': reference(key)})
            db.execute("UPDATE runs SET status='running' WHERE start_key=?", (key,))
            return True

    def complete(self, key, text, secrets=(), *, work_output=False):
        self._same(key)
        if (work_output is not False or type(text) is not str or len(text.encode('utf-8')) > 8192
                or any(ord(c) < 32 and c not in '\n\r\t' for c in text)):
            raise BridgeError('invalid_output')
        text = redact(text, secrets)
        with self._journal.transaction() as db:
            row = db.execute('SELECT status FROM runs WHERE start_key=?', (key,)).fetchone()
            if row is None or row[0] != 'running': return False
            intent = 'reply' if text.strip() else 'silent'
            if text.strip():
                self._append(db, key, {'event_type': 'assistant.delta', 'turn': 0, 'delta': {'text': text}})
            self._append(db, key, {'event_type': 'delivery.intent', 'intent': intent})
            self._append(db, key, {'event_type': 'run.completed', 'delivery_intent': intent})
            db.execute("UPDATE runs SET status='completed' WHERE start_key=?", (key,))
            return True
