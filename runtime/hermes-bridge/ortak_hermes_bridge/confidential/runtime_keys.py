"""Volatile keys restricted to one identity and the two runtime purposes."""
import os
from cryptography.hazmat.primitives.ciphers.aead import AESGCM
from .codec import OpenedPayload
from .wire import ConfidentialEnvelope, ConfidentialError, PayloadHeader, ValidatedIdentity


class RuntimeKey:
    """A derived key, never the employee key or per-run master; not serializable."""
    __slots__ = ('_data', '_identity', '_purpose', '_closed')

    def __init__(self, data, identity, purpose):
        if (type(data) not in (bytes, bytearray) or len(data) != 32
                or type(identity) is not ValidatedIdentity
                or purpose not in ('snapshot', 'runtime_event')):
            raise ConfidentialError('key')
        self._data = bytearray(data)
        self._identity, self._purpose, self._closed = identity, purpose, False

    def __repr__(self): return '<confidential runtime key>'
    def __reduce_ex__(self, _protocol):
        raise TypeError('confidential material cannot be serialized')
    def close(self):
        self._data[:] = b'\0' * len(self._data)
        self._closed = True
    def __del__(self):
        if hasattr(self, '_data'): self.close()

    def _check(self, envelope, ordinal):
        if self._closed or type(envelope) is not ConfidentialEnvelope:
            raise ConfidentialError('key')
        envelope.header.require_expected(self._identity, self._purpose, ordinal)

    def open(self, envelope, ordinal):
        """Authenticate full expected claims before returning volatile bytes."""
        self._check(envelope, ordinal)
        result = None
        try:
            result = AESGCM(bytes(self._data)).decrypt(envelope.nonce, envelope.ciphertext, envelope.header.aad)
        except Exception:
            pass
        if result is None: raise ConfidentialError('authentication')
        return OpenedPayload(result)

    def seal_event(self, ordinal, plaintext):
        """The single journal writer seals new events; retries read stored bytes."""
        if self._closed or self._purpose != 'runtime_event' or type(plaintext) is not bytes:
            raise ConfidentialError('key')
        header = PayloadHeader(self._identity, 'runtime_event', ordinal, len(plaintext))
        result = None
        try:
            nonce = os.urandom(12)
            result = AESGCM(bytes(self._data)).encrypt(nonce, plaintext, header.aad)
        except Exception:
            pass
        if result is None: raise ConfidentialError('crypto')
        return ConfidentialEnvelope(header, nonce, result)
