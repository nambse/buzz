"""Durable bounded tool requests. Public events never contain file contents."""
import json
import time

from .journal import BridgeError, redact
from .workspace_contract import TOOL, canonical, digest, validate_request, validate_result

SCHEMA = '''
CREATE TABLE IF NOT EXISTS workspace_runs (
    start_key TEXT PRIMARY KEY REFERENCES runs(start_key),
    grant_json TEXT NOT NULL CHECK(length(grant_json)<=8192)
);
CREATE TABLE IF NOT EXISTS workspace_tool_calls (
    start_key TEXT NOT NULL REFERENCES workspace_runs(start_key),
    call_id TEXT NOT NULL, ordinal INTEGER NOT NULL CHECK(ordinal BETWEEN 1 AND 4),
    request_json TEXT NOT NULL, deadline REAL NOT NULL,
    state TEXT NOT NULL CHECK(state IN ('pending','resolved','consumed','interrupted')),
    result_json TEXT, result_hash TEXT,
    PRIMARY KEY(start_key,call_id), UNIQUE(start_key,ordinal)
);
'''


def grant_on(db, key):
    row = db.execute('SELECT grant_json FROM workspace_runs WHERE start_key=?', (key,)).fetchone()
    if row is None:
        raise BridgeError('workspace_run_not_found', 404)
    return json.loads(row[0])


def workspace(journal, key):
    """Read the exact admitted workspace; no lookup admits a new selection."""
    with journal.connection() as db:
        row = db.execute('SELECT grant_json FROM workspace_runs WHERE start_key=?', (key,)).fetchone()
        return json.loads(row[0]) if row else None


def reserve(journal, key, call_id, file_id, arguments_hash, seconds=10):
    """Persist one pending call and start event before central filesystem access."""
    if not 0 < seconds <= 10:
        raise BridgeError('tool_deadline_exceeded', 409)
    with journal.transaction(timeout=min(seconds, 0.1)) as db:
        grant = grant_on(db, key)
        run = db.execute('SELECT status FROM runs WHERE start_key=?', (key,)).fetchone()
        if run['status'] != 'running':
            raise BridgeError('tool_run_not_running', 409)
        rows = db.execute('SELECT * FROM workspace_tool_calls WHERE start_key=? ORDER BY ordinal', (key,)).fetchall()
        if any(r['call_id'] == call_id for r in rows):
            # A provider repeating a consumed ID is never another execution.
            raise BridgeError('tool_call_conflict', 409)
        if len(rows) >= 4 or any(r['state'] != 'consumed' for r in rows):
            raise BridgeError('tool_capacity', 409)
        request = {'call_id': call_id, 'file_id': file_id, 'arguments_hash': arguments_hash, 'ordinal': len(rows) + 1}
        validate_request(request)
        if not any(file['file_id'] == file_id for file in grant['files']):
            raise BridgeError('file_unavailable', 422)
        db.execute('INSERT INTO workspace_tool_calls VALUES (?,?,?,?,?,\'pending\',NULL,NULL)',
                   (key, call_id, request['ordinal'], canonical(request), time.time() + seconds))
        journal.append(db, key, {'event_type': 'tool_call.started', 'call_id': redact(call_id),
                                'tool': TOOL, 'arguments': {'text': canonical({'file_id': file_id})}})
        return request


def pending(journal, key):
    """Read only a still-live request; terminal rows never solicit fresh work."""
    with journal.connection() as db:
        db.execute('BEGIN')
        grant_on(db, key)
        row = db.execute('''SELECT t.request_json FROM workspace_tool_calls t JOIN runs r USING(start_key)
            WHERE t.start_key=? AND r.status='running' AND t.state='pending' AND t.deadline>?
            ORDER BY t.ordinal LIMIT 1''', (key, time.time())).fetchone()
        return {'request': json.loads(row[0]) if row else None}


def resolve(journal, key, request, result):
    """Commit exact result plus safe events atomically; lost ACKs replay once."""
    validate_request(request)
    with journal.transaction() as db:
        grant = grant_on(db, key)
        file = next((f for f in grant['files'] if f['file_id'] == request['file_id']), None)
        if file is None:
            raise BridgeError('tool_call_conflict', 409)
        validate_result(result, file)
        result_hash = digest(result)
        row = db.execute('SELECT * FROM workspace_tool_calls WHERE start_key=? AND call_id=?',
                         (key, request['call_id'])).fetchone()
        if row is None or row['request_json'] != canonical(request):
            raise BridgeError('tool_call_conflict', 409)
        ack = {'acknowledged': True, 'call_id': request['call_id'], 'arguments_hash': request['arguments_hash']}
        if row['result_hash'] is not None:
            if row['result_hash'] != result_hash:
                raise BridgeError('tool_result_conflict', 409)
            return ack
        run = db.execute('SELECT status FROM runs WHERE start_key=?', (key,)).fetchone()
        if run['status'] != 'running' or row['state'] != 'pending' or row['deadline'] <= time.time():
            raise BridgeError('tool_run_not_running', 409)
        db.execute("UPDATE workspace_tool_calls SET state='resolved',result_json=?,result_hash=? WHERE start_key=? AND call_id=?",
                   (canonical(result), result_hash, key, request['call_id']))
        if result['status'] == 'completed':
            summary = f"Read {file['bytes']} bytes; SHA256 {file['sha256']}"
            journal.append(db, key, {'event_type': 'file.changed', 'path': redact(file['name']), 'change': 'read',
                                    'summary': {'text': summary}, 'bytes': file['bytes']})
            journal.append(db, key, {'event_type': 'tool_call.completed', 'call_id': redact(request['call_id']),
                                    'result': {'text': summary}})
        else:
            journal.append(db, key, {'event_type': 'tool_call.failed', 'call_id': redact(request['call_id']),
                                    'error': {'text': result['code']}})
        return ack


def consume(journal, key, request):
    """Gate model release against cancellation and retire transient content once."""
    validate_request(request)
    with journal.transaction(timeout=0.05) as db:
        row = db.execute('SELECT t.*,r.status FROM workspace_tool_calls t JOIN runs r USING(start_key) '
                         'WHERE t.start_key=? AND t.call_id=?', (key, request['call_id'])).fetchone()
        if (row is None or row['request_json'] != canonical(request) or row['status'] != 'running'
                or row['state'] not in {'pending', 'resolved'} or row['deadline'] <= time.time()):
            raise BridgeError('tool_run_not_running', 409)
        if row['state'] == 'pending':
            return None
        result = json.loads(row['result_json'])
        grant = grant_on(db, key)
        file = next((f for f in grant['files'] if f['file_id'] == request['file_id']), None)
        if file is None or digest(result) != row['result_hash']:
            raise BridgeError('tool_result_conflict', 409)
        validate_result(result, file)
        db.execute("UPDATE workspace_tool_calls SET state='consumed',result_json=NULL WHERE start_key=? AND call_id=?",
                   (key, request['call_id']))
        return result


def retire(journal, db, key, code):
    """Same-transaction cancellation/failure closes requests and erases held text."""
    rows = db.execute("SELECT call_id,state FROM workspace_tool_calls WHERE start_key=? AND state IN ('pending','resolved')",
                      (key,)).fetchall()
    for row in rows:
        if row['state'] == 'pending':
            journal.append(db, key, {'event_type': 'tool_call.failed', 'call_id': redact(row['call_id']),
                                    'error': {'text': code}})
    db.execute("UPDATE workspace_tool_calls SET state='interrupted',result_json=NULL WHERE start_key=? AND state IN ('pending','resolved')", (key,))


def settled_on(db, key):
    """No final output may overtake an unresolved or refused tool operation."""
    return db.execute("SELECT 1 FROM workspace_tool_calls WHERE start_key=? AND state!='consumed' LIMIT 1", (key,)).fetchone() is None
