"""Bounded canonical claim/envelope bytes, never source or effect authorization."""
import base64
import datetime
import hashlib
import json
import re

MAX_HEADER = 2 * 1024
MAX_ENVELOPE = 96 * 1024
LIMITS = {'snapshot': 48 * 1024, 'runtime_event': 32 * 1024, 'reply_draft': 16 * 1024}
IDENTITY_FIELDS = frozenset((
    'authority_epoch', 'community_id', 'company_id', 'conversation_id', 'employee_id',
    'employee_lifecycle_epoch', 'employee_public_key', 'employee_revision_id',
    'human_public_key', 'key_id', 'key_version', 'office_binding_id', 'rumor_id',
    'run_id', 'source_evidence_hash', 'source_outer_created_at', 'source_outer_id',
))


class ConfidentialError(Exception):
    """Closed error codes only; no retained input or nested backend exception."""
    def __init__(self, code):
        if code not in ('bound', 'encoding', 'identity', 'expectation', 'key',
                        'entropy', 'authentication', 'crypto'):
            code = 'encoding'
        super().__init__('confidential_' + code)


def _pairs(pairs):
    value = {}
    for key, item in pairs:
        if key in value:
            raise ConfidentialError('encoding')
        value[key] = item
    return value


def _no_float(_):
    raise ConfidentialError('encoding')


def _load(data, limit):
    if type(data) is not bytes or not data or len(data) > limit:
        raise ConfidentialError('bound')
    if data[:1] != b'{':
        raise ConfidentialError('encoding')
    try:
        return json.loads(data, object_pairs_hook=_pairs, parse_float=_no_float,
                          parse_constant=_no_float)
    except (ValueError, TypeError, UnicodeError, RecursionError, ConfidentialError):
        pass
    # Raise after leaving except so a JSONDecodeError retaining raw input is
    # not attached as __context__, even when a caller inspects the exception.
    raise ConfidentialError('encoding')


def _dump(value, limit):
    data = json.dumps(value, sort_keys=True, separators=(',', ':'),
                      ensure_ascii=True, allow_nan=False).encode('ascii')
    if len(data) > limit:
        raise ConfidentialError('bound')
    return data


def _keys(value, fields):
    if type(value) is not dict or set(value) != set(fields):
        raise ConfidentialError('encoding')


def _match(value, pattern):
    return type(value) is str and re.fullmatch(pattern, value) is not None


def _identity(value):
    _keys(value, IDENTITY_FIELDS)
    for field in ('authority_epoch', 'employee_lifecycle_epoch', 'key_version'):
        item = value[field]
        if not _match(item, r'0|[1-9][0-9]{0,18}') or int(item) > 9223372036854775807:
            raise ConfidentialError('identity')
    for field in ('company_id', 'community_id', 'conversation_id', 'employee_revision_id',
                  'office_binding_id', 'key_id', 'run_id'):
        item = value[field]
        if (not _match(item, r'[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}')
                or item == '00000000-0000-0000-0000-000000000000'):
            raise ConfidentialError('identity')
    if not _match(value['employee_id'], r'[a-z0-9][a-z0-9_-]{0,63}'):
        raise ConfidentialError('identity')
    for field in ('employee_public_key', 'human_public_key', 'rumor_id',
                  'source_outer_id', 'source_evidence_hash'):
        if not _match(value[field], r'[0-9a-f]{64}'):
            raise ConfidentialError('identity')
    if value['employee_public_key'] == value['human_public_key']:
        raise ConfidentialError('identity')
    at = value['source_outer_created_at']
    if not _match(at, r'[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}\.000000Z'):
        raise ConfidentialError('identity')
    valid = False
    try:
        parsed = datetime.datetime.strptime(at, '%Y-%m-%dT%H:%M:%S.000000Z')
        valid = parsed.year >= 1970
    except ValueError:
        pass
    if not valid:
        raise ConfidentialError('identity')


def _budget(purpose, ordinal, count):
    if type(purpose) is not str or purpose not in LIMITS:
        raise ConfidentialError('encoding')
    if (type(ordinal) is not int or type(count) is not int or not 0 <= count <= LIMITS[purpose]
            or (not 1 <= ordinal <= 512 if purpose == 'runtime_event' else ordinal != 0)):
        raise ConfidentialError('bound')


class _Immutable:
    __slots__ = ()
    def __setattr__(self, _name, _value):
        raise AttributeError('immutable confidential claims')
    def __repr__(self):
        return '<confidential claims>'


class ValidatedIdentity(_Immutable):
    """Shape/canonical-byte claims only; no authorized constructor exists."""
    __slots__ = ('_bytes',)

    def __init__(self, data):
        value = _load(data, MAX_HEADER)
        _identity(value)
        canonical = _dump(value, MAX_HEADER)
        if canonical != data:
            raise ConfidentialError('encoding')
        object.__setattr__(self, '_bytes', canonical)

    @property
    def canonical_bytes(self):
        return self._bytes

    @property
    def sha256(self):
        return hashlib.sha256(self._bytes).digest()


class PayloadHeader(_Immutable):
    """Immutable AAD; callers must independently establish current rights."""
    __slots__ = ('_identity', '_purpose', '_ordinal', '_count', '_bytes')

    def __init__(self, identity, purpose, ordinal, plaintext_bytes):
        if type(identity) is not ValidatedIdentity:
            raise ConfidentialError('identity')
        _budget(purpose, ordinal, plaintext_bytes)
        value = {'algorithm': 'A256GCM', 'format': 'ortak-confidential-payload/1',
                 'identity': _load(identity.canonical_bytes, MAX_HEADER), 'ordinal': ordinal,
                 'plaintext_bytes': plaintext_bytes, 'purpose': purpose}
        object.__setattr__(self, '_bytes', _dump(value, MAX_HEADER))
        object.__setattr__(self, '_identity', identity)
        object.__setattr__(self, '_purpose', purpose)
        object.__setattr__(self, '_ordinal', ordinal)
        object.__setattr__(self, '_count', plaintext_bytes)

    @property
    def aad(self): return self._bytes
    @property
    def identity(self): return self._identity
    @property
    def purpose(self): return self._purpose
    @property
    def ordinal(self): return self._ordinal
    @property
    def plaintext_bytes(self): return self._count

    def require_expected(self, identity, purpose, ordinal):
        if type(identity) is not ValidatedIdentity:
            raise ConfidentialError('identity')
        _budget(purpose, ordinal, self.plaintext_bytes)
        if (identity.canonical_bytes != self.identity.canonical_bytes
                or purpose != self.purpose or ordinal != self.ordinal):
            raise ConfidentialError('expectation')


def _header(value):
    _keys(value, ('algorithm', 'format', 'identity', 'ordinal', 'plaintext_bytes', 'purpose'))
    if value['algorithm'] != 'A256GCM' or value['format'] != 'ortak-confidential-payload/1':
        raise ConfidentialError('encoding')
    _identity(value['identity'])
    identity = ValidatedIdentity(_dump(value['identity'], MAX_HEADER))
    return PayloadHeader(identity, value['purpose'], value['ordinal'], value['plaintext_bytes'])


def _decode64(value, count):
    # Exact encoded bound before the library allocates decoded output.
    if type(value) is not str or len(value) != ((count + 2) // 3) * 4:
        raise ConfidentialError('bound')
    try:
        decoded = base64.b64decode(value, validate=True)
        if len(decoded) == count and base64.b64encode(decoded).decode('ascii') == value:
            return decoded
    except (ValueError, UnicodeError):
        pass
    raise ConfidentialError('encoding')


class ConfidentialEnvelope(_Immutable):
    """Canonical ciphertext, not a verified tag or a current permission."""
    __slots__ = ('_header', '_nonce', '_ciphertext', '_bytes')

    def __init__(self, header, nonce, ciphertext):
        if type(header) is not PayloadHeader:
            raise ConfidentialError('encoding')
        if (type(nonce) is not bytes or len(nonce) != 12 or type(ciphertext) is not bytes
                or len(ciphertext) != header.plaintext_bytes + 16):
            raise ConfidentialError('bound')
        value = {'ciphertext': base64.b64encode(ciphertext).decode('ascii'),
                 'header': _load(header.aad, MAX_HEADER), 'nonce': base64.b64encode(nonce).decode('ascii')}
        object.__setattr__(self, '_bytes', _dump(value, MAX_ENVELOPE))
        object.__setattr__(self, '_header', header)
        object.__setattr__(self, '_nonce', nonce)
        object.__setattr__(self, '_ciphertext', ciphertext)

    @classmethod
    def parse(cls, data):
        value = _load(data, MAX_ENVELOPE)
        _keys(value, ('ciphertext', 'header', 'nonce'))
        header = _header(value['header'])
        result = cls(header, _decode64(value['nonce'], 12),
                     _decode64(value['ciphertext'], header.plaintext_bytes + 16))
        if result.canonical_bytes != data:
            raise ConfidentialError('encoding')
        return result

    @property
    def canonical_bytes(self): return self._bytes
    @property
    def header(self): return self._header
    @property
    def nonce(self): return self._nonce
    @property
    def ciphertext(self): return self._ciphertext
