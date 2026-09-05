#!/usr/bin/env python3
"""Provision only the selected fresh Ada Honcho bundle; never activate a worker."""

import argparse
from contextlib import contextmanager
from datetime import datetime, timezone
import fcntl
import hashlib
import http.client
import json
import os
from pathlib import Path
import re
import signal
import stat
import tempfile
import time
from uuid import UUID, uuid4

from init_private_stack import PROJECT
from private_native_services import private_file, selected_root

PROTOCOL = "ortak-honcho/1"
ORIGIN = "http://127.0.0.1:8009"
ENDPOINT_REF = "service://ortak-private-20260905/honcho"
TOKEN_REF = "secret://ortak-private-20260905/honcho-admin"
FORMAT = "ortak-private-memory-bootstrap/1"
MAX_RESPONSE = 64 * 1024


class Refused(Exception):
    """Fixed-code failures never include credentials, response bodies or private state."""


def require(condition, code):
    if not condition:
        raise Refused(code)


def canonical(value):
    return json.dumps(value, sort_keys=True, separators=(",", ":"),
                      ensure_ascii=False, allow_nan=False)


def fingerprint(value):
    return hashlib.sha256(canonical(value).encode()).hexdigest()


def uuid(value):
    require(isinstance(value, str), "invalid_uuid")
    result = UUID(value)
    require(str(result) == value and result.int != 0, "invalid_uuid")
    return value


def name(value):
    return isinstance(value, str) and re.fullmatch(r"[A-Za-z0-9_-]{1,128}", value)


def token_variable(value):
    require(isinstance(value, str) and len(value) <= 128 and
            re.fullmatch(r"ORTAK_HONCHO_[A-Z0-9_]+", value), "invalid_token_environment")
    return value


def fresh_intent(company, deployment, token_env):
    timestamp = datetime.now(timezone.utc)
    recorded_at = timestamp.isoformat(timespec="microseconds" if timestamp.microsecond else "seconds")
    return {
        "format": FORMAT, "project": PROJECT, "company_id": uuid(company),
        "employee_id": "ada-private", "deployment_id": uuid(deployment),
        "origin": ORIGIN, "token_env": token_variable(token_env),
        "binding": {"adapter": "honcho", "endpoint_ref": ENDPOINT_REF,
                    "workspace": "ortak_ada_" + UUID(company).hex,
                    "user_peer": "operator-private", "employee_peer": "ada-private", "options": {}},
        "creation_key": f"ortak-memory:{company}:ada-private:{deployment}",
        "validation_run_id": str(uuid4()),
        "validation_recorded_at": recorded_at.replace("+00:00", "Z"),
    }


def validate_intent(intent, company, deployment, token_env):
    require(isinstance(intent, dict), "invalid_bootstrap_intent")
    expected = fresh_intent(company, deployment, token_env)
    for key in ("validation_run_id", "validation_recorded_at"):
        expected[key] = intent.get(key)
    require(intent == expected, "bootstrap_intent_changed")
    uuid(intent["validation_run_id"])
    recorded = intent["validation_recorded_at"]
    require(isinstance(recorded, str) and re.fullmatch(
        r"\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d{6})?Z", recorded), "invalid_diagnostic_time")
    parsed = datetime.fromisoformat(recorded.replace("Z", "+00:00"))
    normalized = parsed.isoformat(timespec="microseconds" if parsed.microsecond else "seconds").replace("+00:00", "Z")
    require(recorded == normalized, "invalid_diagnostic_time")


def create_body(intent):
    binding = intent["binding"]
    return {"idempotency_key": intent["creation_key"], "company_id": intent["company_id"],
            "employee_id": intent["employee_id"], "workspace_id": binding["workspace"],
            "user_peer": binding["user_peer"], "employee_peer": binding["employee_peer"]}


def expected_resource(intent):
    binding = intent["binding"]
    return {"protocol": PROTOCOL, "workspace_id": binding["workspace"],
            "user_peer": binding["user_peer"], "employee_peer": binding["employee_peer"],
            "ownership": "created"}


def validate_identity(intent, response):
    require(isinstance(response, dict), "invalid_resource_identity")
    ids = response.get("native_ids")
    require(isinstance(ids, dict) and set(ids) == {"workspace", "peers"}
            and name(ids["workspace"]) and isinstance(ids["peers"], dict), "invalid_resource_identity")
    binding = intent["binding"]
    require(set(ids["peers"]) == {binding["user_peer"], binding["employee_peer"]}
            and all(name(value) for value in ids["peers"].values())
            and len(set(ids["peers"].values())) == 2, "invalid_resource_identity")
    expected = {**expected_resource(intent), "company_id": intent["company_id"],
                "employee_id": intent["employee_id"], "request_hash": fingerprint(create_body(intent)),
                "native_ids": ids}
    require(response == expected, "resource_receipt_mismatch")


def diagnostic(intent):
    scope = {"scope": "run_scratch", "run_id": intent["validation_run_id"]}
    context = {"protocol": PROTOCOL, "company_id": intent["company_id"],
               "employee_id": intent["employee_id"], "scope": scope}
    session = "ortak_" + fingerprint(context)
    text = f"Ortak memory roundtrip {intent['deployment_id']} {intent['validation_run_id']}"
    provenance = {"employee_id": intent["employee_id"], "run_id": intent["validation_run_id"],
                  "source": "ortak_memory_roundtrip", "recorded_at": intent["validation_recorded_at"]}
    write = {"idempotency_key": f"roundtrip:{intent['deployment_id']}:{intent['validation_run_id']}",
             "company_id": intent["company_id"], "employee_id": intent["employee_id"], "scope": scope,
             "facts": [{"content": text, "provenance": provenance}]}
    recall = {"company_id": intent["company_id"], "employee_id": intent["employee_id"], "scope": scope,
              "query": text, "max_records": 1, "max_bytes": 4096}
    return context, session, write, recall


def validate_write(intent, response):
    require(isinstance(response, dict), "invalid_diagnostic_receipt")
    context, session, body, _ = diagnostic(intent)
    digest = fingerprint({**body, "workspace_id": intent["binding"]["workspace"], "session_id": session})
    refs = response.get("record_refs")
    require(isinstance(refs, list) and len(refs) == 1 and name(refs[0]), "invalid_diagnostic_record")
    fact = body["facts"][0]
    record = {"record_ref": refs[0], "content": fact["content"], "scope": body["scope"],
              "provenance": fact["provenance"], "metadata": {"ortak": {
                  **context, "write_key": body["idempotency_key"], "request_hash": digest,
                  "fact_index": 0, "provenance": fact["provenance"]}}}
    expected = {"protocol": PROTOCOL, "workspace_id": intent["binding"]["workspace"],
                "session_id": session, "request_hash": digest, "record_refs": refs, "records": [record]}
    require(response == expected, "diagnostic_receipt_mismatch")
    return {key: record[key] for key in ("record_ref", "content", "scope", "provenance")}


def worker_config(intent):
    return {"deployment_id": intent["deployment_id"], "origin": ORIGIN,
            "endpoint_ref": ENDPOINT_REF, "token_ref": TOKEN_REF, "token_env": intent["token_env"],
            "validate_memory_io": True, "employees": [{
                key: intent[key] for key in ("employee_id", "binding", "creation_key",
                                            "validation_run_id", "validation_recorded_at")}]}


def save(path, value):
    """Atomic snapshot and directory fsync; existing private files are validated first."""
    if path.exists() or path.is_symlink():
        private_file(path)
    encoded = canonical(value) + "\n"
    require(len(encoded.encode()) <= MAX_RESPONSE, "bootstrap_state_too_large")
    temporary = None
    try:
        with tempfile.NamedTemporaryFile(mode="w", dir=path.parent, prefix=".memory-write-", delete=False) as output:
            temporary = Path(output.name)
            output.write(encoded)
            output.flush()
            os.fsync(output.fileno())
        os.replace(temporary, path)
        temporary = None
        descriptor = os.open(path.parent, os.O_RDONLY | os.O_DIRECTORY)
        try:
            os.fsync(descriptor)
        finally:
            os.close(descriptor)
    finally:
        if temporary is not None:
            temporary.unlink()


@contextmanager
def locked_directory(root):
    directory = root / "memory"
    if not directory.exists():
        directory.mkdir(mode=0o700)
    metadata = directory.lstat()
    require(stat.S_ISDIR(metadata.st_mode) and metadata.st_uid == os.getuid()
            and not metadata.st_mode & 0o077, "invalid_memory_directory")
    descriptor = os.open(directory / ".bootstrap.lock", os.O_RDWR | os.O_CREAT | os.O_NOFOLLOW | os.O_NONBLOCK, 0o600)
    try:
        metadata = os.fstat(descriptor)
        require(stat.S_ISREG(metadata.st_mode) and metadata.st_uid == os.getuid()
                and not metadata.st_mode & 0o077, "invalid_bootstrap_lock")
        fcntl.flock(descriptor, fcntl.LOCK_EX | fcntl.LOCK_NB)
        yield directory
    finally:
        os.close(descriptor)


class Http:
    """Literal loopback only; no ambient proxies, redirects or unbounded response reads."""
    def __init__(self, token):
        require(isinstance(token, str) and 1 <= len(token) <= 16384 and token.isascii()
                and not any(char.isspace() or ord(char) < 32 or ord(char) == 127 for char in token), "invalid_selected_token")
        self.token = token
        self.deadline = time.monotonic() + 20

    def request(self, method, path, body=None):
        remaining = self.deadline - time.monotonic()
        require(remaining > 0, "network_deadline")
        connection = http.client.HTTPConnection("127.0.0.1", 8009, timeout=min(5, remaining))
        headers = {"Authorization": "Bearer " + self.token}
        encoded = None
        if body is not None:
            encoded = canonical(body).encode()
            require(len(encoded) <= 16384, "request_too_large")
            headers["Content-Type"] = "application/json"
        try:
            connection.request(method, path, body=encoded, headers=headers)
            response = connection.getresponse()
            require(response.status in {200, 201}, "memory_service_refused")
            size = response.getheader("Content-Length")
            require(size is None or 0 <= int(size) <= MAX_RESPONSE, "response_too_large")
            chunks, total = [], 0
            while True:
                remaining = self.deadline - time.monotonic()
                require(remaining > 0, "network_deadline")
                if connection.sock is not None:
                    connection.sock.settimeout(min(5, remaining))
                chunk = response.read1(min(4096, MAX_RESPONSE + 1 - total))
                if not chunk:
                    break
                chunks.append(chunk)
                total += len(chunk)
                require(total <= MAX_RESPONSE, "response_too_large")
            return json.loads(b"".join(chunks))
        finally:
            connection.close()


def bootstrap(root, deployment, token_env, http):
    """One attempt only; durable original keys make an explicit retry safe."""
    identities = json.loads(private_file(root / "identities.json"))
    require(identities.get("project") == PROJECT and identities.get("employee_id") == "ada-private",
            "wrong_private_identity")
    company = uuid(identities["company_id"])
    uuid(deployment)
    token_variable(token_env)
    with locked_directory(root) as directory:
        path = directory / "bootstrap.json"
        if path.exists() or path.is_symlink():
            state = json.loads(private_file(path))
            require(isinstance(state, dict) and set(state) == {
                "intent", "resource_receipt", "resource_identity", "roundtrip_receipt", "completed"}, "invalid_bootstrap_state")
            validate_intent(state["intent"], company, deployment, token_env)
        else:
            require(all(child.name == ".bootstrap.lock" for child in directory.iterdir()), "unmarked_memory_state")
            state = {"intent": fresh_intent(company, deployment, token_env), "resource_receipt": None,
                     "resource_identity": None, "roundtrip_receipt": None, "completed": False}
            save(path, state)
        intent = state["intent"]
        require(type(state["completed"]) is bool, "invalid_bootstrap_state")
        require(not state["resource_identity"] or state["resource_receipt"], "invalid_bootstrap_state")
        require(not state["roundtrip_receipt"] or state["resource_identity"], "invalid_bootstrap_state")
        require(not state["completed"] or state["roundtrip_receipt"], "invalid_bootstrap_state")
        if state["resource_receipt"] is not None:
            require(state["resource_receipt"] == expected_resource(intent), "stored_resource_receipt_changed")
        if state["resource_identity"] is not None:
            validate_identity(intent, state["resource_identity"])
        if state["roundtrip_receipt"] is not None:
            validate_write(intent, state["roundtrip_receipt"])
        output = directory / "worker-memory.json"
        if output.exists() or output.is_symlink():
            require(state["completed"] and json.loads(private_file(output)) == worker_config(intent), "worker_config_changed")
        require(http.request("GET", "/v3/ortak/protocol") ==
                {"protocol": PROTOCOL, "honcho_version": "3.1.1"}, "wrong_memory_protocol")
        if state["resource_receipt"] is None:
            received = http.request("POST", "/v3/ortak/resources/create", create_body(intent))
            require(received == expected_resource(intent), "resource_receipt_mismatch")
            state["resource_receipt"] = received
            save(path, state)
        base = f"/v3/ortak/workspaces/{intent['binding']['workspace']}"
        body = {key: create_body(intent)[key] for key in ("company_id", "employee_id", "user_peer", "employee_peer")}
        received = http.request("POST", base + "/resources/inspect", body)
        validate_identity(intent, received)
        require(state["resource_identity"] is None or state["resource_identity"] == received, "native_resource_identity_changed")
        if state["resource_identity"] is None:
            state["resource_identity"] = received
            save(path, state)
        validated_now = not state["completed"]
        if validated_now:
            _, session, write, recall = diagnostic(intent)
            received = http.request("POST", base + f"/sessions/{session}/remember", write)
            record = validate_write(intent, received)
            require(state["roundtrip_receipt"] is None or state["roundtrip_receipt"] == received, "diagnostic_receipt_changed")
            state["roundtrip_receipt"] = received
            save(path, state)
            recalled = http.request("POST", base + f"/sessions/{session}/recall", recall)
            require(isinstance(recalled, dict) and set(recalled) == {"records", "truncated"}
                    and type(recalled["truncated"]) is bool and recalled["records"] == [record], "diagnostic_recall_mismatch")
            state["completed"] = True
            save(path, state)
        if not output.exists():
            save(output, worker_config(intent))
        return {"result": "provisioned", "employee_id": "ada-private", "worker_config": str(output),
                "roundtrip": "verified_now" if validated_now else "previously_verified",
                "employee_activated": False, "worker_started": False}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--state-dir", required=True, type=Path)
    parser.add_argument("--deployment-id", required=True)
    parser.add_argument("--token-env", required=True)
    args = parser.parse_args()
    root = selected_root(args.state_dir)
    token_variable(args.token_env)
    token = os.environ.get(args.token_env)
    transport = Http(token)
    os.umask(0o077)
    def deadline(_signal, _frame):
        raise Refused("network_deadline")
    previous = signal.signal(signal.SIGALRM, deadline)
    signal.setitimer(signal.ITIMER_REAL, 20)
    try:
        result = bootstrap(root, args.deployment_id, args.token_env, transport)
    finally:
        signal.setitimer(signal.ITIMER_REAL, 0)
        signal.signal(signal.SIGALRM, previous)
    print(canonical(result))


if __name__ == "__main__":
    try:
        main()
    except Exception:
        raise SystemExit("Private memory bootstrap failed; durable state retained. No secrets were logged.") from None
