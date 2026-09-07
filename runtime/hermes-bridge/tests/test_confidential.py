"""Production codecs, shared literal vector and malformed inputs; no provider/DB.

The full vector was calculated outside these codecs with raw PyCA APIs. JSON
mutations below are adversarial input generation, not a second parser/validator.
"""
import copy
import json
import pickle
from pathlib import Path
import unittest
from unittest.mock import patch

from ortak_hermes_bridge.confidential import codec, wire


VECTOR = Path(__file__).resolve().parents[3] / 'crates/ortak-control/src/confidential/vector.json'


def mutation_bytes(value):
    return json.dumps(value, sort_keys=True, separators=(',', ':')).encode()


class ConfidentialCodec(unittest.TestCase):
    def setUp(self):
        self.f = json.loads(VECTOR.read_bytes())
        self.identity = wire.ValidatedIdentity(self.f['expected']['identity_utf8'].encode())
        self.envelope = wire.ConfidentialEnvelope.parse(self.f['expected']['envelope_utf8'].encode())
        self.master = codec.MasterKey(bytes.fromhex(self.f['master_hex']))
        self.addCleanup(self.master.close)

    def test_standard_hkdf_and_aes256_gcm_anchors(self):
        # RFC5869 SHA256 test case1; independent of the application wire.
        actual = codec._hkdf(b'\x0b' * 22, bytes.fromhex('000102030405060708090a0b0c'),
                             bytes.fromhex('f0f1f2f3f4f5f6f7f8f9'), 42)
        self.assertEqual(actual.hex(), '3cb25f25faacd57a90434f64d0362f2a2d2d0a90cf1a5a4c5db02d56ecc4c5bf34007208d5b887185865')
        # NIST CAVS gcmEncryptExtIV256.rsp, also in retained RustCrypto tests.
        key = bytes.fromhex('b52c505a37d78eda5dd34f20c22540ea1b58963cf8e5bf8ffa85f9f2492505b4')
        nonce = bytes.fromhex('516c33929df5a3284ff463d7')
        self.assertEqual(codec._encrypt(key, nonce, b'', b'').hex(),
                         'bdc1ac884d332457a1d2664f168c76f0')

    def test_literal_full_vector_matches_both_directions(self):
        expected = self.f['expected']
        self.assertEqual(self.identity.sha256.hex(), expected['identity_sha256_hex'])
        self.assertEqual(self.envelope.header.aad, expected['aad_utf8'].encode())
        key = codec._derive(self.master, self.identity, 'snapshot')
        self.assertEqual(key.hex(), expected['derived_key_hex'])
        key[:] = bytes(len(key))
        header = wire.PayloadHeader(self.identity, 'snapshot', 0, 20)
        result = codec._seal_with_nonce(self.master, header, bytes.fromhex(self.f['plaintext_hex']),
                                        bytes.fromhex(self.f['nonce_hex']))
        self.assertEqual(result.canonical_bytes, expected['envelope_utf8'].encode())
        self.assertEqual(result.ciphertext.hex(), expected['ciphertext_hex'])
        opened = codec.open_payload(self.master, self.identity, 'snapshot', 0, self.envelope)
        self.addCleanup(opened.close)
        self.assertEqual(bytes(opened.view()), bytes.fromhex(self.f['plaintext_hex']))

    def test_identity_unknown_duplicate_shapes_and_noncanonical_claims(self):
        cases = [('authority_epoch', 3), ('authority_epoch', '03'), ('key_version', '-1'),
                 ('key_version', '9223372036854775808'), ('employee_id', 'Ada'),
                 ('employee_id', 'a' * 65), ('employee_id', 'a\n'),
                 ('company_id', '11111111111141118111111111111111'),
                 ('run_id', '00000000-0000-0000-0000-000000000000'),
                 ('source_outer_id', 'D' * 64),
                 ('human_public_key', self.f['identity']['employee_public_key']),
                 ('source_outer_created_at', '2026-09-06T03:00:00.000000+03:00'),
                 ('source_outer_created_at', '2026-09-06T00:00:00.000001Z'),
                 ('source_outer_created_at', '2016-12-31T23:59:60.000000Z'),
                 ('source_outer_created_at', '1969-12-31T23:59:59.000000Z'), ('unknown', None)]
        for field, value in cases:
            item = copy.deepcopy(self.f['identity']); item[field] = value
            with self.subTest(field=field), self.assertRaises(wire.ConfidentialError):
                wire.ValidatedIdentity(mutation_bytes(item))
        text = self.identity.canonical_bytes
        malformed = [text.replace(b'{', b'{"authority_epoch":"3",', 1), b' ' + text, text + b'\n',
                     text.replace(b'fixture-employee', b'fixture\\u002demployee'), b'\xff',
                     mutation_bytes(list(self.f['identity'].values()))]
        for value in malformed:
            with self.assertRaises(wire.ConfidentialError): wire.ValidatedIdentity(value)
        with patch.object(wire.json, 'loads', side_effect=AssertionError('must bound before JSON')):
            with self.assertRaises(wire.ConfidentialError): wire.ValidatedIdentity(b'x' * (wire.MAX_HEADER + 1))

    def test_envelope_shape_base64_header_and_predecode_bounds(self):
        base = json.loads(self.envelope.canonical_bytes)
        for field, value in [('algorithm', 'A128GCM'), ('format', 'ortak-confidential-payload/2'),
                             ('purpose', 'other'), ('ordinal', 1), ('ordinal', True), ('ordinal', 0.0),
                             ('plaintext_bytes', 49153), ('unknown', None)]:
            item = copy.deepcopy(base); item['header'][field] = value
            with self.subTest(field=field), self.assertRaises(wire.ConfidentialError):
                wire.ConfidentialEnvelope.parse(mutation_bytes(item))
        for field, value in [('nonce', base['nonce'] + '='),
                             ('ciphertext', '_' * 48), ('unknown', None)]:
            item = copy.deepcopy(base); item[field] = value
            with self.assertRaises(wire.ConfidentialError): wire.ConfidentialEnvelope.parse(mutation_bytes(item))
        for target in ('envelope', 'header', 'identity'):
            item = copy.deepcopy(base)
            if target == 'envelope': item = list(item.values())
            elif target == 'header': item['header'] = list(item['header'].values())
            else: item['header']['identity'] = list(item['header']['identity'].values())
            with self.assertRaises(wire.ConfidentialError): wire.ConfidentialEnvelope.parse(mutation_bytes(item))
        duplicate = self.envelope.canonical_bytes.replace(b'"ordinal":0', b'"ordinal":0,"ordinal":0')
        with self.assertRaises(wire.ConfidentialError): wire.ConfidentialEnvelope.parse(duplicate)
        empty = wire.ConfidentialEnvelope(wire.PayloadHeader(self.identity, 'snapshot', 0, 0), bytes(12), bytes(16))
        changed = empty.canonical_bytes.replace(b'AAAAAAAAAAAAAAAAAAAAAA==', b'AAAAAAAAAAAAAAAAAAAAAB==')
        with self.assertRaises(wire.ConfidentialError): wire.ConfidentialEnvelope.parse(changed)
        with patch.object(wire.base64, 'b64decode', side_effect=AssertionError('must bound before decode')):
            item = copy.deepcopy(base); item['nonce'] = 'a' * 20
            with self.assertRaises(wire.ConfidentialError): wire.ConfidentialEnvelope.parse(mutation_bytes(item))
        with patch.object(wire.json, 'loads', side_effect=AssertionError('must bound before JSON')):
            with self.assertRaises(wire.ConfidentialError): wire.ConfidentialEnvelope.parse(b'x' * (wire.MAX_ENVELOPE + 1))

    def test_open_exact_expectations_and_authenticated_corruption(self):
        changed = wire.ValidatedIdentity(self.identity.canonical_bytes.replace(b'"authority_epoch":"3"', b'"authority_epoch":"4"'))
        for identity, purpose, ordinal in [(changed, 'snapshot', 0), (self.identity, 'reply_draft', 0),
                                           (self.identity, 'snapshot', 1)]:
            with patch.object(codec, '_derive', side_effect=AssertionError('expected before decrypt')):
                with self.assertRaises(wire.ConfidentialError):
                    codec.open_payload(self.master, identity, purpose, ordinal, self.envelope)
        wrong = codec.MasterKey(bytes(32)); self.addCleanup(wrong.close)
        with self.assertRaises(wire.ConfidentialError):
            codec.open_payload(wrong, self.identity, 'snapshot', 0, self.envelope)
        for nonce, ciphertext in [
            (bytes([self.envelope.nonce[0] ^ 1]) + self.envelope.nonce[1:], self.envelope.ciphertext),
            (self.envelope.nonce, self.envelope.ciphertext[:-1] + bytes([self.envelope.ciphertext[-1] ^ 1]))]:
            item = wire.ConfidentialEnvelope(self.envelope.header, nonce, ciphertext)
            with self.assertRaises(wire.ConfidentialError):
                codec.open_payload(self.master, self.identity, 'snapshot', 0, item)
        item = wire.ConfidentialEnvelope(wire.PayloadHeader(changed, 'snapshot', 0, 20),
                                         self.envelope.nonce, self.envelope.ciphertext)
        with self.assertRaises(wire.ConfidentialError): codec.open_payload(self.master, changed, 'snapshot', 0, item)

    def test_fresh_nonce_full_bounds_and_exact_opaque_bytes(self):
        text = '  confidential\nİstanbul \\ \u2028\0  '.encode()
        first = codec.seal(self.master, self.identity, 'runtime_event', 1, text)
        second = codec.seal(self.master, self.identity, 'runtime_event', 1, text)
        self.assertNotEqual(first.nonce, second.nonce)
        opened = codec.open_payload(self.master, self.identity, 'runtime_event', 1, first)
        self.addCleanup(opened.close); self.assertEqual(bytes(opened.view()), text)
        # Opaque bytes are preserved, not implicitly accepted as DM text/RunSpec.
        for purpose, limit in wire.LIMITS.items():
            ordinal = 1 if purpose == 'runtime_event' else 0
            full = b'\xff' * limit
            sealed = codec.seal(self.master, self.identity, purpose, ordinal, full)
            result = codec.open_payload(self.master, self.identity, purpose, ordinal, sealed)
            self.assertEqual(bytes(result.view()), full); result.close()
            with patch.object(codec.os, 'urandom', side_effect=AssertionError('bound before entropy')):
                with self.assertRaises(wire.ConfidentialError): codec.seal(self.master, self.identity, purpose, ordinal, full + b'x')
        for ordinal in (0, 513):
            with self.assertRaises(wire.ConfidentialError): wire.PayloadHeader(self.identity, 'runtime_event', ordinal, 0)
        with self.assertRaises(TypeError): codec.seal(self.master, self.identity, 'snapshot', 0, b'', nonce=bytes(12))
        with patch.object(codec.os, 'urandom', side_effect=OSError('synthetic-private-diagnostic')):
            with self.assertRaises(wire.ConfidentialError) as failed:
                codec.seal(self.master, self.identity, 'snapshot', 0, b'private')
            self.assertIsNone(failed.exception.__context__)
            self.assertNotIn('synthetic-private', repr(failed.exception))

    def test_repr_errors_and_owned_buffers_do_not_expose_content(self):
        result = codec.open_payload(self.master, self.identity, 'snapshot', 0, self.envelope)
        view = result.view(); result.close()
        self.assertEqual(bytes(view), bytes(len(view)))
        with self.assertRaises(wire.ConfidentialError): result.view()
        for value in (self.master, result, self.identity, self.envelope, self.envelope.header):
            self.assertNotIn('fixture-employee', repr(value))
            self.assertNotIn('confidential vector', repr(value))
            self.assertNotIn(self.f['master_hex'], repr(value))
        for value in (self.master, result):
            with self.assertRaises(TypeError): pickle.dumps(value)
        with self.assertRaises(AttributeError): self.identity._bytes = b'changed'
        with self.assertRaises(wire.ConfidentialError) as bad:
            wire.ValidatedIdentity(b'{"private canary": invalid')
        self.assertIsNone(bad.exception.__context__)
        self.assertNotIn('private canary', repr(bad.exception))
        with patch.object(codec, 'AESGCM', side_effect=ValueError('private canary')):
            with self.assertRaises(wire.ConfidentialError) as bad:
                codec.open_payload(self.master, self.identity, 'snapshot', 0, self.envelope)
            self.assertIsNone(bad.exception.__context__)
            self.assertNotIn('private canary', repr(bad.exception))
        self.master.close()
        with self.assertRaises(wire.ConfidentialError):
            codec.open_payload(self.master, self.identity, 'snapshot', 0, self.envelope)


if __name__ == '__main__':
    unittest.main()
