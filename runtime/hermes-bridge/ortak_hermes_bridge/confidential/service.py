"""Distinct authenticated routes; exact replay/lookup/cancel need no decrypt keys."""
import hashlib
from urllib.parse import parse_qs
from ..journal import BridgeError, identity, reference, start_key
from . import journal
from .request import ConfidentialRequest
from .wire import ConfidentialEnvelope, ConfidentialError, _dump, _keys


def dispatch(bridge, method, path, query, body):
    """Only called after the ordinary handler's authentication and body bounds."""
    try:
        if method == 'POST' and path == '/v1/confidential/runs':
            if not getattr(bridge.executor, 'confidential_dm', False):
                raise BridgeError('confidential_executor_unavailable', 503)
            request = ConfidentialRequest(body, bridge)
            try:
                with bridge.lock:
                    existing = bridge.journal.lookup(request.key)
                    if existing is None and not bridge.executor.available:
                        raise BridgeError('executor_unavailable', 503)
                    receipt, fresh = journal.reserve(bridge.journal, request)
                    if fresh:
                        try:
                            bridge.executor.start_confidential(request, bridge.journal)
                        except Exception:
                            bridge.journal.fail(request.key, 'executor_unavailable')
                            raise BridgeError('executor_unavailable', 503) from None
                    return receipt
            finally:
                request.close()
        if method == 'POST' and path == '/v1/confidential/runs/replay':
            _keys(body, ('company_id', 'snapshot'))
            snapshot = ConfidentialEnvelope.parse(_dump(body['snapshot'], 96 * 1024))
            snapshot.header.require_expected(snapshot.header.identity, 'snapshot', 0)
            import json
            claims = json.loads(snapshot.header.identity.canonical_bytes)
            if body['company_id'] != bridge.company_id or claims['company_id'] != bridge.company_id:
                raise BridgeError('run_not_found', 404)
            key = f"ortak-run:{bridge.company_id}:{claims['run_id']}"
            with bridge.journal.connection() as db:
                row = db.execute('SELECT r.*,c.snapshot FROM runs r JOIN confidential_runs c USING(start_key) WHERE start_key=?', (key,)).fetchone()
                if row is None: raise BridgeError('run_not_found', 404)
                if (row['snapshot'] != snapshot.canonical_bytes
                        or row['fingerprint'] != hashlib.sha256(snapshot.canonical_bytes).hexdigest()):
                    raise BridgeError('start_conflict', 409)
                return bridge.journal.receipt(row)
        if method == 'POST' and path in ('/v1/confidential/runs/lookup', '/v1/confidential/runs/cancel'):
            _keys(body, ('company_id', 'run_id', 'idempotency_key'))
            key = bridge.scoped_key(body)
            journal.require_mode(bridge.journal, key)
            if path.endswith('/lookup'):
                receipt = bridge.journal.lookup(key)
                if receipt is None: raise BridgeError('run_not_found', 404)
                return receipt
            with bridge.lock:
                known = bridge.journal.has_start(key)
                outcome = bridge.journal.request_cancel(key)
                if known and not bridge.executor.stop(key):
                    raise BridgeError('execution_not_stopped', 503)
                if outcome != 'already_terminal': bridge.journal.finish_cancel(key)
                return {'runtime_run_ref': reference(key), 'outcome': outcome}
        if method == 'GET' and path.startswith('/v1/confidential/runs/') and path.endswith('/events'):
            key = start_key(path[len('/v1/confidential/runs/'):-len('/events')])
            if identity(key)[0] != bridge.company_id: raise BridgeError('run_not_found', 404)
            args = parse_qs(query, keep_blank_values=True)
            if set(args) - {'after', 'limit'} or any(len(values) != 1 for values in args.values()):
                raise BridgeError('invalid_cursor')
            try:
                after, limit = int(args.get('after', ['0'])[0]), int(args.get('limit', ['4'])[0])
            except ValueError:
                raise BridgeError('invalid_cursor') from None
            return journal.events(bridge.journal, key, after, limit)
    except ConfidentialError:
        pass
    else:
        raise BridgeError('not_found', 404)
    raise BridgeError('invalid_confidential_request', 422)
