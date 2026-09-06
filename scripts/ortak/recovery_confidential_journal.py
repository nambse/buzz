"""Unactivated, metadata-only proof for the explicitly reviewed journal extension.

No keys, decryption, application imports or current-authority checks. The caller
owns the cold-copy barrier, SQLite read transaction and hard process deadline.
"""

import base64
import hashlib
import json
import time

TABLES = {
    'confidential_runs': ('start_key', 'format', 'identity', 'snapshot', 'key_id', 'snapshot_nonce'),
    'confidential_events': ('start_key', 'sequence', 'occurred_at', 'envelope', 'key_id', 'nonce'),
    'confidential_status': ('start_key', 'code', 'occurred_at'),
}
MAX_RUNS = 1024
MAX_EVENTS = 16384
MAX_BYTES = 8 * 1024 * 1024
STATUS_CODES = frozenset(('executor_interrupted', 'executor_unavailable', 'policy_denied',
    'provider_failed', 'deadline_exceeded', 'provider_incomplete', 'provider_response_invalid',
    'invalid_output', 'credential_denied', 'runtime_selection_changed',
    'unsupported_hermes_tool_selection', 'cancelled'))


def require(value):
    """Never expose rejected ciphertext, claims or nested decoder errors."""
    if not value:
        raise ValueError('recovery_confidential_journal_refused')


def canonical(value):
    return json.dumps(value, sort_keys=True, separators=(',', ':'),
                      ensure_ascii=True, allow_nan=False).encode('ascii')


def object_bytes(raw, maximum):
    """Canonical roundtrip rejects duplicate fields and noncanonical encodings."""
    require(type(raw) is bytes and 0 < len(raw) <= maximum and raw[:1] == b'{')
    value = None
    try:
        parsed = json.loads(raw)
        if type(parsed) is dict and canonical(parsed) == raw:
            value = parsed
    except (ValueError, TypeError, UnicodeError, RecursionError):
        pass
    require(value is not None)
    return value


def decoded(value, count):
    require(type(value) is str and len(value) == ((count + 2) // 3) * 4)
    result = None
    try:
        raw = base64.b64decode(value, validate=True)
        if len(raw) == count and base64.b64encode(raw).decode('ascii') == value:
            result = raw
    except (ValueError, UnicodeError):
        pass
    require(result is not None)
    return result


def envelope(raw, identity, purpose, ordinal, nonce):
    """Check public envelope framing and stored pins, never authenticate an AEAD tag."""
    value = object_bytes(raw, 98304)
    require(set(value) == {'ciphertext', 'header', 'nonce'})
    header = value['header']
    require(type(header) is dict and set(header) ==
        {'algorithm', 'format', 'identity', 'ordinal', 'plaintext_bytes', 'purpose'})
    require(header['algorithm'] == 'A256GCM' and header['format'] == 'ortak-confidential-payload/1'
        and header['identity'] == identity and header['purpose'] == purpose
        and type(header['ordinal']) is int and header['ordinal'] == ordinal
        and len(canonical(header)) <= 2048)
    count = header['plaintext_bytes']
    require(type(count) is int and 0 <= count <= (49152 if purpose == 'snapshot' else 32768))
    require(type(nonce) is bytes and len(nonce) == 12 and decoded(value['nonce'], 12) == nonce)
    decoded(value['ciphertext'], count + 16)


def observe(database, deadline):
    """Bound and hash all three tables inside the caller's single read-only snapshot."""
    counts = {}
    for table, columns in TABLES.items():
        require(tuple(row[1] for row in database.execute('PRAGMA table_info(' + table + ')')) == columns)
        counts[table] = database.execute('SELECT count(*) FROM ' + table).fetchone()[0]
        require(counts[table] <= (MAX_EVENTS if table == 'confidential_events' else MAX_RUNS))
    size = database.execute('''SELECT
        coalesce((SELECT sum(length(CAST(identity AS BLOB))+length(CAST(snapshot AS BLOB))) FROM confidential_runs),0)
        +coalesce((SELECT sum(length(CAST(envelope AS BLOB))) FROM confidential_events),0)''').fetchone()[0]
    require(size <= MAX_BYTES and time.monotonic() < deadline)
    malformed_storage = database.execute('''SELECT
        (SELECT count(*) FROM confidential_runs WHERE typeof(identity)<>'blob'
            OR typeof(snapshot)<>'blob' OR typeof(snapshot_nonce)<>'blob'
            OR length(identity) NOT BETWEEN 1 AND 2048 OR length(snapshot) NOT BETWEEN 1 AND 98304
            OR length(snapshot_nonce)<>12 OR typeof(start_key)<>'text' OR length(start_key)>96
            OR typeof(key_id)<>'text' OR length(key_id)<>36
            OR typeof(format)<>'text' OR length(format)>64)
        +(SELECT count(*) FROM confidential_events WHERE typeof(envelope)<>'blob'
            OR length(envelope) NOT BETWEEN 1 AND 98304 OR typeof(nonce)<>'blob' OR length(nonce)<>12
            OR typeof(start_key)<>'text' OR length(start_key)>96
            OR typeof(key_id)<>'text' OR length(key_id)<>36
            OR typeof(occurred_at)<>'text' OR length(occurred_at) NOT BETWEEN 1 AND 64)
        +(SELECT count(*) FROM confidential_status WHERE typeof(start_key)<>'text' OR length(start_key)>96
            OR typeof(code)<>'text' OR length(code)>64
            OR typeof(occurred_at)<>'text' OR length(occurred_at) NOT BETWEEN 1 AND 64)
        ''').fetchone()[0]
    require(malformed_storage == 0)
    # Completed protected runs have no ordinary events. Each mode has its own
    # contiguous sequence, with the shared runs.sequence as the final cursor.
    invalid = database.execute('''SELECT count(*) FROM confidential_runs c
        LEFT JOIN runs r USING(start_key) LEFT JOIN (
            SELECT start_key,count(*) n,count(DISTINCT sequence) distinct_n,
                min(sequence) first,max(sequence) last FROM confidential_events GROUP BY start_key
        ) e USING(start_key) LEFT JOIN confidential_status s USING(start_key)
        WHERE r.start_key IS NULL OR r.status NOT IN ('completed','failed','cancelled')
            OR typeof(r.fingerprint)<>'text' OR length(r.fingerprint)<>64
            OR typeof(r.sequence)<>'integer' OR r.sequence NOT BETWEEN 0 AND 512
            OR coalesce(e.n,0)<>r.sequence OR coalesce(e.distinct_n,0)<>r.sequence
            OR (e.n IS NOT NULL AND (e.first<>1 OR e.last<>r.sequence))
            OR (r.status='completed' AND (r.sequence=0 OR s.start_key IS NOT NULL))
            OR (r.status IN ('failed','cancelled') AND s.start_key IS NULL)
            OR (r.status='cancelled' AND s.code<>'cancelled')
            OR (r.status='failed' AND s.code='cancelled')''').fetchone()[0]
    for table in ('confidential_events', 'confidential_status'):
        invalid += database.execute('SELECT count(*) FROM ' + table +
            ' t LEFT JOIN confidential_runs c USING(start_key) WHERE c.start_key IS NULL').fetchone()[0]
    names = {row[0] for row in database.execute("SELECT name FROM sqlite_master WHERE type='table'")}
    for table in ('events', 'workspace_runs', 'workspace_tool_calls', 'profile_probes', 'private_failure_diagnostics'):
        if table in names:
            invalid += database.execute('SELECT count(*) FROM ' + table +
                ' t JOIN confidential_runs c USING(start_key)').fetchone()[0]
    require(invalid == 0)
    identities, nonces, hashes = {}, set(), {}
    for key, format_name, raw_identity, snapshot, key_id, nonce in database.execute(
            'SELECT * FROM confidential_runs ORDER BY start_key'):
        require(time.monotonic() < deadline)
        identity = object_bytes(raw_identity, 2048)
        require(format_name == 'ortak-confidential-journal/1' and type(key_id) is str
            and identity.get('key_id') == key_id and key_id not in identities
            and key == 'ortak-run:' + str(identity.get('company_id')) + ':' + str(identity.get('run_id')))
        envelope(snapshot, identity, 'snapshot', 0, nonce)
        fingerprint = database.execute('SELECT fingerprint FROM runs WHERE start_key=?', (key,)).fetchone()[0]
        require(fingerprint == hashlib.sha256(snapshot).hexdigest())
        identities[key_id] = (key, identity)
    for key, ordinal, at, raw, key_id, nonce in database.execute(
            'SELECT * FROM confidential_events ORDER BY start_key,sequence'):
        require(time.monotonic() < deadline and type(ordinal) is int and 1 <= ordinal <= 512
            and type(at) is str and 0 < len(at) <= 64 and key_id in identities
            and identities[key_id][0] == key)
        envelope(raw, identities[key_id][1], 'runtime_event', ordinal, nonce)
        # Purpose-separated keys permit snapshot/event nonce equality; event
        # nonce reuse within the same key is never permitted.
        require((key_id, nonce) not in nonces)
        nonces.add((key_id, nonce))
    for _, code, at in database.execute('SELECT * FROM confidential_status'):
        require(code in STATUS_CODES and type(at) is str and 0 < len(at) <= 64)
    for table in TABLES:
        hashed = hashlib.sha256()
        order = 'start_key,sequence' if table == 'confidential_events' else 'start_key'
        for row in database.execute('SELECT * FROM ' + table + ' ORDER BY ' + order):
            require(time.monotonic() < deadline)
            encoded = canonical([{'blob': value.hex()} if isinstance(value, bytes) else value for value in row])
            require(len(encoded) <= 256 * 1024)
            hashed.update(encoded + b'\n')
        hashes[table] = hashed.hexdigest()
    return {'format': 'ortak-recovery-confidential-journal/1', 'tables': counts,
        'logical_rows_sha256': hashes, 'ciphertext_bytes': size, 'invalid_cursors': 0,
        'nonterminal': 0, 'cryptographic_authentication': 'not_performed', 'automatic_activation': False}
