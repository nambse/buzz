"""Closed confidential transport and authenticated inner spec, with no persistence."""
import json
import base64
from ..journal import BridgeError, identity as start_identity
from .runtime_keys import RuntimeKey
from .wire import ConfidentialEnvelope, ConfidentialError, _decode64, _keys, _load, _dump

MAX_REQUEST = 112 * 1024


def canonical_inner(value, limit):
    """Sorted compact UTF-8 JSON; arbitrary text is inside the AEAD only."""
    try:
        data = json.dumps(value, sort_keys=True, separators=(',', ':'), ensure_ascii=False, allow_nan=False).encode('utf-8')
    except (ValueError, TypeError, UnicodeError, RecursionError):
        data = None
    if data is None or len(data) > limit:
        raise ConfidentialError('bound')
    return data


class ConfidentialRequest:
    """Bounded volatile selection. close() clears owned keys and spec references."""
    __slots__ = ('snapshot', 'identity', 'claims', 'key', 'snapshot_key', 'event_key', 'spec')

    def __repr__(self): return '<confidential request>'
    def __reduce_ex__(self, _protocol):
        raise TypeError('confidential request cannot be serialized')

    def __init__(self, body, bridge):
        self.snapshot_key = self.event_key = None
        self.spec = None
        _keys(body, ('company_id', 'snapshot', 'keys'))
        # Object API and HTTP parser both enforce the complete request budget.
        canonical_inner(body, MAX_REQUEST)
        self.snapshot = ConfidentialEnvelope.parse(_dump(body['snapshot'], 96 * 1024))
        self.identity = self.snapshot.header.identity
        self.snapshot.header.require_expected(self.identity, 'snapshot', 0)
        self.claims = json.loads(self.identity.canonical_bytes)
        if body['company_id'] != bridge.company_id or self.claims['company_id'] != bridge.company_id:
            raise BridgeError('run_not_found', 404)
        self.key = f"ortak-run:{bridge.company_id}:{self.claims['run_id']}"
        start_identity(self.key)
        _keys(body['keys'], ('snapshot', 'runtime_event'))
        try:
            self.snapshot_key = RuntimeKey(_decode64(body['keys']['snapshot'], 32), self.identity, 'snapshot')
            self.event_key = RuntimeKey(_decode64(body['keys']['runtime_event'], 32), self.identity, 'runtime_event')
            opened = self.snapshot_key.open(self.snapshot, 0)
            try:
                data = bytes(opened.view())
                inner = _load(data, 48 * 1024)
                _keys(inner, ('format', 'identity', 'spec'))
                if (inner['format'] != 'ortak-confidential-run/1' or inner['identity'] != self.claims
                        or canonical_inner(inner, 48 * 1024) != data):
                    raise ConfidentialError('expectation')
                self.spec = inner['spec']
                from ..service import EMPTY_POLICY
                key, spec = bridge.validate({'company_id': bridge.company_id, 'spec': self.spec})
                context = spec['context']
                if (key != self.key or spec['employee_id'] != self.claims['employee_id']
                        or spec['revision_id'] != self.claims['employee_revision_id']
                        or spec['permissions'] != EMPTY_POLICY or len(spec['input'].encode('utf-8')) > 8192
                        or set(context) - {'conversation_ref', 'reply_to_message_id'}
                        or context.get('conversation_ref') != self.claims['conversation_id']):
                    raise ConfidentialError('expectation')
                reply = context.get('reply_to_message_id')
                if reply is not None:
                    from .wire import _match
                    if not _match(reply, r'[0-9a-f]{64}'):
                        raise ConfidentialError('expectation')
            finally:
                opened.close()
        except BaseException:
            self.close()
            raise

    def close(self):
        if self.snapshot_key is not None: self.snapshot_key.close()
        if self.event_key is not None: self.event_key.close()
        self.spec = None

    def child_body(self):
        """Only the sealed volatile stdin transport may serialize these keys."""
        if self.spec is None or self.snapshot_key._closed or self.event_key._closed:
            raise ConfidentialError('key')
        return {'company_id': self.claims['company_id'], 'snapshot': json.loads(self.snapshot.canonical_bytes),
                'keys': {'snapshot': base64.b64encode(self.snapshot_key._data).decode('ascii'),
                         'runtime_event': base64.b64encode(self.event_key._data).decode('ascii')}}

    def __del__(self):
        if hasattr(self, 'snapshot_key'): self.close()
