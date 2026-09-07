"""Exercise the real bootstrap state machine without sockets or credentials."""

import copy
import hashlib
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import bootstrap_private_memory as subject

COMPANY = "49be31fd-fbcf-4e74-bd26-e36f1d24c8b5"
DEPLOYMENT = "c0c957fc-c78e-4441-a968-30d1ac71c604"
TOKEN_ENV = "ORTAK_HONCHO_PRIVATE_TOKEN"


def digest(value):
    encoded = json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=False)
    return hashlib.sha256(encoded.encode()).hexdigest()


class Service:
    """Protocol fixture with its own receipt construction and idempotency store."""

    def __init__(self, root):
        self.root = root
        self.calls = []
        self.create = None
        self.writes = {}
        self.lose_create_reply = False
        self.lose_write_reply = False
        self.empty_recall = False
        self.replace_identity = False
        self.missing = False
        self.corrupt_write = False

    def request(self, method, path, body=None):
        self.calls.append((method, path, copy.deepcopy(body)))
        # A request must never precede the durable original intent.
        persisted = json.loads((self.root / "memory/bootstrap.json").read_text())
        intent = persisted["intent"]
        if path == "/v3/ortak/protocol":
            assert method == "GET"
            return {"protocol": "ortak-honcho/1", "honcho_version": "3.1.1"}
        assert method == "POST"
        if path.endswith("/resources/create"):
            assert body["idempotency_key"] == intent["creation_key"]
            assert intent["validation_run_id"] and intent["validation_recorded_at"]
            if self.create is None:
                self.create = copy.deepcopy(body)
            else:
                assert body == self.create, "a retry changed the original create request"
            if self.lose_create_reply:
                self.lose_create_reply = False
                raise TimeoutError("fixture lost create acknowledgement")
            return self.resource()
        if path.endswith("/resources/inspect"):
            if self.missing:
                raise subject.Refused("fixture_resource_missing")
            assert set(body) == {"company_id", "employee_id", "user_peer", "employee_peer"}
            return {**self.resource(), "company_id": self.create["company_id"],
                    "employee_id": self.create["employee_id"], "request_hash": digest(self.create),
                    "native_ids": {"workspace": "replacement" if self.replace_identity else "workspace-native",
                                   "peers": {"operator-private": "operator-native", "ada-private": "ada-native"}}}
        session = path.split("/")[-2]
        if path.endswith("/remember"):
            key = body["idempotency_key"]
            if key not in self.writes:
                request_hash = digest({**body, "workspace_id": self.create["workspace_id"], "session_id": session})
                context = {"protocol": "ortak-honcho/1", "company_id": body["company_id"],
                           "employee_id": body["employee_id"], "scope": body["scope"]}
                assert session == "ortak_" + digest(context)
                fact = body["facts"][0]
                record = {"record_ref": "message-native", "content": fact["content"],
                          "scope": body["scope"], "provenance": fact["provenance"],
                          "metadata": {"ortak": {**context, "write_key": key, "request_hash": request_hash,
                                                 "fact_index": 0, "provenance": fact["provenance"]}}}
                self.writes[key] = (copy.deepcopy(body), {
                    "protocol": "ortak-honcho/1", "workspace_id": self.create["workspace_id"],
                    "session_id": session, "request_hash": request_hash,
                    "record_refs": ["message-native"], "records": [record]})
            original, response = self.writes[key]
            assert original == body, "a retry changed the original diagnostic write"
            if self.lose_write_reply:
                self.lose_write_reply = False
                raise TimeoutError("fixture lost write acknowledgement")
            response = copy.deepcopy(response)
            if self.corrupt_write:
                response["records"][0]["metadata"]["ortak"]["provenance"]["source"] = "forged"
            return response
        if path.endswith("/recall"):
            assert len(self.writes) == 1
            record = next(iter(self.writes.values()))[1]["records"][0]
            assert body["query"] == record["content"]
            assert body["scope"] == record["scope"]
            return {"records": [] if self.empty_recall else [
                {key: record[key] for key in ("record_ref", "content", "scope", "provenance")}], "truncated": True}
        raise AssertionError("unexpected protocol operation: " + path)

    def resource(self):
        return {"protocol": "ortak-honcho/1", "ownership": "created", **{
            key: self.create[key] for key in ("workspace_id", "user_peer", "employee_peer")}}


class BootstrapTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        identity = self.root / "identities.json"
        identity.write_text(json.dumps({"project": subject.PROJECT, "company_id": COMPANY,
                                        "employee_id": "ada-private"}))
        identity.chmod(0o600)
        self.service = Service(self.root)

    def run_bootstrap(self, **kwargs):
        return subject.bootstrap(self.root, kwargs.get("deployment", DEPLOYMENT),
                                 kwargs.get("token_env", TOKEN_ENV), self.service,
                                 export_prepared=kwargs.get("export_prepared", False))

    def state(self):
        return json.loads((self.root / "memory/bootstrap.json").read_text())

    def test_success_persists_exact_receipts_and_secret_free_config(self):
        identity_before = (self.root / "identities.json").read_bytes()
        result = self.run_bootstrap()
        self.assertEqual(result["roundtrip"], "verified_now")
        self.assertFalse(result["employee_activated"] or result["worker_started"])
        state = self.state()
        self.assertTrue(state["completed"])
        self.assertEqual(state["resource_identity"]["request_hash"], digest(self.service.create))
        config = json.loads(Path(result["worker_config"]).read_text())
        self.assertEqual(set(config), {"deployment_id", "origin", "endpoint_ref", "token_ref",
                                      "token_env", "validate_memory_io", "employees"})
        self.assertEqual(config["token_env"], TOKEN_ENV)
        self.assertEqual(config["employees"][0]["creation_key"], f"ortak-memory:{COMPANY}:ada-private:{DEPLOYMENT}")
        self.assertEqual(config["employees"][0]["validation_run_id"], state["intent"]["validation_run_id"])
        self.assertEqual((self.root / "identities.json").read_bytes(), identity_before)
        for path in (self.root / "memory").iterdir():
            self.assertEqual(path.stat().st_mode & 0o777, 0o600)

    def test_lost_create_response_retries_original_durable_key(self):
        self.service.lose_create_reply = True
        with self.assertRaises(TimeoutError):
            self.run_bootstrap()
        original = self.state()["intent"]
        self.assertIsNone(self.state()["resource_receipt"])
        self.run_bootstrap()
        self.assertEqual(self.state()["intent"], original)
        self.assertEqual(sum(path.endswith("/create") for _, path, _ in self.service.calls), 2)
        self.assertEqual(len(self.service.writes), 1)

    def test_lost_write_response_replays_exact_original(self):
        self.service.lose_write_reply = True
        with self.assertRaises(TimeoutError):
            self.run_bootstrap()
        original = self.state()["intent"]
        self.run_bootstrap()
        self.assertEqual(self.state()["intent"], original)
        self.assertEqual(len(self.service.writes), 1)
        self.assertEqual(sum(path.endswith("/create") for _, path, _ in self.service.calls), 1)

    def test_completed_restart_only_inspects_and_does_not_write(self):
        self.run_bootstrap()
        before = {path.name: path.read_bytes() for path in (self.root / "memory").iterdir()}
        self.service.calls.clear()
        with patch.object(subject, "uuid4", side_effect=AssertionError("must retain diagnostic ID")):
            result = self.run_bootstrap()
        self.assertEqual(result["roundtrip"], "previously_verified")
        self.assertEqual(len(self.service.calls), 2)
        self.assertTrue(self.service.calls[-1][1].endswith("/resources/inspect"))
        self.assertEqual({path.name: path.read_bytes() for path in (self.root / "memory").iterdir()}, before)

    def test_missing_or_replaced_native_identity_never_creates_again(self):
        self.run_bootstrap()
        original = (self.root / "memory/worker-memory.json").read_bytes()
        for fault in ("missing", "replace_identity"):
            with self.subTest(fault=fault):
                self.service.calls.clear()
                setattr(self.service, fault, True)
                with self.assertRaises(subject.Refused):
                    self.run_bootstrap()
                setattr(self.service, fault, False)
                self.assertFalse(any(path.endswith("/create") or path.endswith("/remember")
                                     for _, path, _ in self.service.calls))
                self.assertEqual((self.root / "memory/worker-memory.json").read_bytes(), original)

    def test_changed_intent_or_tampered_receipt_fails_before_network(self):
        self.run_bootstrap()
        self.service.calls.clear()
        with self.assertRaises(subject.Refused):
            self.run_bootstrap(deployment="138b889a-af98-480c-8d71-9238c9cded25")
        with self.assertRaises(subject.Refused):
            self.run_bootstrap(token_env="ORTAK_HONCHO_OTHER_TOKEN")
        state = self.state()
        state["resource_identity"]["request_hash"] = "0" * 64
        subject.save(self.root / "memory/bootstrap.json", state)
        with self.assertRaises(subject.Refused):
            self.run_bootstrap()
        self.assertEqual(self.service.calls, [])

    def test_canonical_write_and_nonempty_recall_required(self):
        self.service.corrupt_write = True
        with self.assertRaisesRegex(subject.Refused, "diagnostic_receipt_mismatch"):
            self.run_bootstrap()
        self.assertFalse(self.state()["completed"])
        self.service.corrupt_write = False
        self.service.empty_recall = True
        with self.assertRaisesRegex(subject.Refused, "diagnostic_recall_mismatch"):
            self.run_bootstrap()
        self.assertIsNotNone(self.state()["roundtrip_receipt"])
        self.assertFalse((self.root / "memory/worker-memory.json").exists())
        self.service.empty_recall = False
        self.run_bootstrap()
        self.assertEqual(len(self.service.writes), 1)

    def test_interrupted_config_publication_recovers_without_memory_write(self):
        original_save = subject.save
        def interrupt(path, value):
            if path.name == "worker-memory.json":
                raise OSError("fixture publication interruption")
            original_save(path, value)
        with patch.object(subject, "save", side_effect=interrupt), self.assertRaises(OSError):
            self.run_bootstrap()
        self.assertTrue(self.state()["completed"])
        self.service.calls.clear()
        self.run_bootstrap()
        self.assertEqual(len(self.service.calls), 2)
        self.assertTrue((self.root / "memory/worker-memory.json").exists())

    def test_private_files_and_exact_employee_are_required(self):
        identity = self.root / "identities.json"
        identity.chmod(0o644)
        with self.assertRaises(ValueError):
            self.run_bootstrap()
        identity.chmod(0o600)
        identity.write_text(json.dumps({"project": subject.PROJECT, "company_id": COMPANY,
                                        "employee_id": "cem"}))
        with self.assertRaises(subject.Refused):
            self.run_bootstrap()
        identity.unlink()
        identity.symlink_to(self.root / "absent")
        with self.assertRaises(OSError):
            self.run_bootstrap()
        self.assertEqual(self.service.calls, [])

    def test_prepared_export_shares_original_receipt_and_preserves_legacy_files(self):
        self.run_bootstrap()
        state = self.state()
        before = {path.name: path.read_bytes() for path in (self.root / "memory").iterdir()}
        self.service.calls.clear()
        with patch.object(subject, "uuid4", side_effect=AssertionError("must retain diagnostic ID")):
            result = self.run_bootstrap(export_prepared=True)
        self.assertEqual(result["result"], "prepared_receipts_exported")
        self.assertEqual(result["roundtrip"], "previously_verified")
        worker = json.loads(Path(result["worker_config"]).read_text())
        prepared = json.loads(Path(result["prepared_memory_config"]).read_text())
        self.assertTrue(worker["require_creation_receipts"])
        entry = worker["employees"][0]
        self.assertEqual(entry["creation_receipt"], prepared["creation_receipt"])
        receipt = prepared["creation_receipt"]
        self.assertEqual(set(receipt), {"company_id", "deployment_id", "employee_id", "binding",
                                        "creation_key", "request_hash", "native_ids", "resources"})
        self.assertEqual(receipt["company_id"], COMPANY)
        self.assertEqual(receipt["deployment_id"], DEPLOYMENT)
        self.assertEqual(receipt["creation_key"], state["intent"]["creation_key"])
        self.assertEqual(receipt["request_hash"], state["resource_identity"]["request_hash"])
        self.assertEqual(receipt["native_ids"], state["resource_identity"]["native_ids"])
        workspace = state["intent"]["binding"]["workspace"]
        self.assertEqual(receipt["resources"], {
            "workspace": {"resource_ref": f"workspace:{workspace}", "ownership": "created"},
            "user_peer": {"resource_ref": f"peer:{workspace}/operator-private", "ownership": "created"},
            "employee_peer": {"resource_ref": f"peer:{workspace}/ada-private", "ownership": "created"}})
        for key in ("validation_run_id", "validation_recorded_at"):
            self.assertEqual(prepared[key], entry[key])
            self.assertEqual(prepared[key], state["intent"][key])
        self.assertEqual(set(prepared), {"origin", "token_ref", "token_env", "creation_receipt",
                                         "validate_memory_io", "validation_run_id", "validation_recorded_at"})
        for filename, original in before.items():
            self.assertEqual((self.root / "memory" / filename).read_bytes(), original)
        self.assertEqual(len(self.service.calls), 2)
        self.assertTrue(self.service.calls[-1][1].endswith("/resources/inspect"))
        self.assertFalse(result["employee_activated"] or result["worker_started"])

    def test_prepared_export_retry_is_immutable_and_read_only(self):
        self.run_bootstrap()
        self.run_bootstrap(export_prepared=True)
        before = {path.name: path.read_bytes() for path in (self.root / "memory").iterdir()}
        self.service.calls.clear()
        with patch.object(subject, "uuid4", side_effect=AssertionError("must retain diagnostic ID")):
            self.run_bootstrap(export_prepared=True)
        self.assertEqual({path.name: path.read_bytes() for path in (self.root / "memory").iterdir()}, before)
        self.assertEqual(len(self.service.calls), 2)

    def test_prepared_export_refuses_missing_or_incomplete_bootstrap(self):
        with self.assertRaisesRegex(subject.Refused, "completed_memory_bootstrap_required"):
            self.run_bootstrap(export_prepared=True)
        self.assertFalse((self.root / "memory/bootstrap.json").exists())
        self.assertEqual(self.service.calls, [])
        self.service.lose_write_reply = True
        with self.assertRaises(TimeoutError):
            self.run_bootstrap()
        self.service.calls.clear()
        with self.assertRaisesRegex(subject.Refused, "completed_memory_bootstrap_required"):
            self.run_bootstrap(export_prepared=True)
        self.assertEqual(self.service.calls, [])
        self.assertFalse((self.root / "memory/worker-memory-prepared.json").exists())

    def test_prepared_export_requires_current_original_native_identity(self):
        self.run_bootstrap()
        self.service.calls.clear()
        self.service.replace_identity = True
        with self.assertRaisesRegex(subject.Refused, "native_resource_identity_changed"):
            self.run_bootstrap(export_prepared=True)
        self.assertFalse((self.root / "memory/worker-memory-prepared.json").exists())
        self.assertFalse((self.root / "memory/prepared-memory.json").exists())
        self.assertFalse(any(path.endswith("/create") or path.endswith("/remember")
                             for _, path, _ in self.service.calls))

    def test_prepared_export_tamper_and_symlink_fail_before_network(self):
        self.run_bootstrap()
        self.run_bootstrap(export_prepared=True)
        path = self.root / "memory/prepared-memory.json"
        original = path.read_bytes()
        altered = json.loads(original)
        altered["creation_receipt"]["native_ids"]["workspace"] = "forged-native-id"
        subject.save(path, altered)
        self.service.calls.clear()
        with self.assertRaisesRegex(subject.Refused, "prepared_memory_config_changed"):
            self.run_bootstrap(export_prepared=True)
        self.assertEqual(self.service.calls, [])
        path.unlink()
        path.symlink_to(self.root / "missing")
        with self.assertRaises(OSError):
            self.run_bootstrap(export_prepared=True)
        self.assertEqual(self.service.calls, [])

    def test_interrupted_prepared_export_recovers_without_new_io(self):
        self.run_bootstrap()
        original_save = subject.save
        def interrupt(path, value):
            if path.name == "prepared-memory.json":
                raise OSError("fixture interrupted second export")
            original_save(path, value)
        with patch.object(subject, "save", side_effect=interrupt), self.assertRaises(OSError):
            self.run_bootstrap(export_prepared=True)
        worker = self.root / "memory/worker-memory-prepared.json"
        self.assertTrue(worker.exists())
        before = worker.read_bytes()
        self.service.calls.clear()
        self.run_bootstrap(export_prepared=True)
        self.assertEqual(worker.read_bytes(), before)
        self.assertTrue((self.root / "memory/prepared-memory.json").exists())
        self.assertEqual(len(self.service.calls), 2)
        self.assertEqual(len(self.service.writes), 1)


class TransportTests(unittest.TestCase):
    def response(self, chunks, length=None, status=200):
        class Response:
            def getheader(self, _):
                return length
            def read1(self, size):
                if not chunks:
                    return b""
                chunk = chunks[0][:size]
                chunks[0] = chunks[0][size:]
                if not chunks[0]:
                    chunks.pop(0)
                return chunk
        result = Response()
        result.status = status
        return result

    def test_streaming_size_cap_and_literal_authenticated_origin(self):
        with patch.object(subject.http.client, "HTTPConnection") as connection:
            connection.return_value.getresponse.return_value = self.response([b"x" * (subject.MAX_RESPONSE + 1)])
            with self.assertRaisesRegex(subject.Refused, "response_too_large"):
                subject.Http("fixture-only").request("GET", "/v3/ortak/protocol")
            self.assertEqual(connection.call_args.args, ("127.0.0.1", 8009))
            self.assertEqual(connection.return_value.request.call_args.kwargs["headers"],
                             {"Authorization": "Bearer fixture-only"})
            connection.return_value.close.assert_called_once()

    def test_large_length_redirect_and_expired_deadline_refuse(self):
        for response in (self.response([], str(subject.MAX_RESPONSE + 1)), self.response([], status=302)):
            with patch.object(subject.http.client, "HTTPConnection") as connection:
                connection.return_value.getresponse.return_value = response
                with self.assertRaises(subject.Refused):
                    subject.Http("fixture-only").request("GET", "/v3/ortak/protocol")
                connection.return_value.close.assert_called_once()
        transport = subject.Http("fixture-only")
        transport.deadline = 0
        with patch.object(subject.http.client, "HTTPConnection") as connection:
            with self.assertRaisesRegex(subject.Refused, "network_deadline"):
                transport.request("GET", "/v3/ortak/protocol")
            connection.assert_not_called()

    def test_rejects_missing_ambient_or_header_injection_token(self):
        for token in (None, "", " token", "token\nvalue", "token\tvalue", "token\x7fvalue", "x" * 16385):
            with self.subTest(token_type=type(token).__name__), self.assertRaises(subject.Refused):
                subject.Http(token)


if __name__ == "__main__":
    unittest.main()
