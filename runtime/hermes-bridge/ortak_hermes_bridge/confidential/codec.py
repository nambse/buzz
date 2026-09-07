"""AES256-GCM/HKDF only: caller-owned key, fresh nonce, no persistence or routes.

PyCA may make immutable transient copies. close() overwrites our owned buffers;
this does not promise erasure of Python/backend/host memory or caller copies.
"""
import os

from cryptography.hazmat.primitives import hashes
from cryptography.hazmat.primitives.ciphers.aead import AESGCM
from cryptography.hazmat.primitives.kdf.hkdf import HKDF

from .wire import (ConfidentialEnvelope, ConfidentialError, PayloadHeader,
                   ValidatedIdentity)


class MasterKey:
    """One explicitly supplied 32-byte key; no discovery or authorization."""
    __slots__ = ('_data', '_closed')
    def __init__(self, data):
        if type(data) not in (bytes, bytearray) or len(data) != 32:
            raise ConfidentialError('key')
        self._data = bytearray(data)
        self._closed = False
    def __repr__(self): return '<confidential key>'
    def __reduce_ex__(self, _protocol):
        raise TypeError('confidential material cannot be serialized')
    def close(self):
        self._data[:] = b'\0' * len(self._data)
        self._closed = True
    def __del__(self):
        if hasattr(self, '_data'):
            self.close()


class OpenedPayload:
    """Volatile bytes only; opening has not validated the inner DM/RunSpec."""
    __slots__ = ('_data', '_closed')
    def __init__(self, data):
        if type(data) is not bytes or len(data) > 48 * 1024:
            raise ConfidentialError('bound')
        self._data = bytearray(data)
        self._closed = False
    def __repr__(self): return '<confidential plaintext>'
    def __reduce_ex__(self, _protocol):
        raise TypeError('confidential material cannot be serialized')
    def view(self):
        if self._closed:
            raise ConfidentialError('key')
        return memoryview(self._data).toreadonly()
    def close(self):
        self._data[:] = b'\0' * len(self._data)
        self._closed = True
    def __del__(self):
        if hasattr(self, '_data'):
            self.close()


def _hkdf(ikm, salt, info, length=32):
    try:
        return HKDF(algorithm=hashes.SHA256(), length=length, salt=salt, info=info).derive(ikm)
    except Exception:
        pass
    raise ConfidentialError('crypto')


def _derive(master, identity, purpose):
    if type(master) is not MasterKey or master._closed or len(master._data) != 32:
        raise ConfidentialError('key')
    # Identity/purpose have already passed PayloadHeader checks before this call.
    info = b'ortak-confidential-dm-aead/1\0' + purpose.encode('ascii')
    return bytearray(_hkdf(bytes(master._data), identity.sha256, info))


def _encrypt(key, nonce, plaintext, aad):
    try:
        return AESGCM(bytes(key)).encrypt(nonce, plaintext, aad)
    except Exception:
        pass
    raise ConfidentialError('crypto')


def _seal_with_nonce(master, header, plaintext, nonce):
    """Private deterministic vector seam; public seal never accepts a nonce."""
    if (type(plaintext) not in (bytes, bytearray) or len(plaintext) != header.plaintext_bytes
            or type(nonce) is not bytes or len(nonce) != 12):
        raise ConfidentialError('bound')
    key = _derive(master, header.identity, header.purpose)
    try:
        ciphertext = _encrypt(key, nonce, bytes(plaintext), header.aad)
        return ConfidentialEnvelope(header, nonce, ciphertext)
    finally:
        key[:] = b'\0' * len(key)


def seal(master, identity, purpose, ordinal, plaintext):
    """Protect one NEW record; storage must enforce nonce/ordinal uniqueness.

    Replay persisted canonical_bytes, never call seal again to implement retry.
    Current authorization and atomic persistence are outside this pure module.
    """
    if type(plaintext) not in (bytes, bytearray):
        raise ConfidentialError('bound')
    header = PayloadHeader(identity, purpose, ordinal, len(plaintext))
    nonce = None
    try:
        nonce = os.urandom(12)
    except OSError:
        pass
    if type(nonce) is not bytes or len(nonce) != 12:
        raise ConfidentialError('entropy')
    return _seal_with_nonce(master, header, plaintext, nonce)


def open_payload(master, identity, purpose, ordinal, envelope):
    """Authenticate exact expected claims; returns no partial plaintext on failure."""
    if type(envelope) is not ConfidentialEnvelope or type(identity) is not ValidatedIdentity:
        raise ConfidentialError('encoding')
    envelope.header.require_expected(identity, purpose, ordinal)
    key = _derive(master, identity, purpose)
    plaintext = None
    try:
        try:
            plaintext = AESGCM(bytes(key)).decrypt(envelope.nonce, envelope.ciphertext, envelope.header.aad)
        except Exception:
            pass
    finally:
        key[:] = b'\0' * len(key)
    if plaintext is None:
        raise ConfidentialError('authentication')
    return OpenedPayload(plaintext)
