#!/usr/bin/env python3
"""Bounded read-only observations of the marked private stack, without credentials."""

import argparse
from datetime import datetime, timezone
import http.client
import json
import os
from pathlib import Path
import re
import signal
import socket
import stat
import time
from uuid import UUID

import bootstrap_private_memory as memory
from init_private_stack import PROJECT
from private_native_services import private_file, selected_root

ENDPOINTS = (
    ("relay_liveness", 8089, "/_liveness", "health"),
    ("relay_readiness", 8089, "/_readiness", "health"),
    ("object_store_liveness", 9008, "/minio/health/live", "health"),
    ("object_store_readiness", 9008, "/minio/health/ready", "health"),
    ("product_api_auth", 8787, "/api/v1/employees", "auth"),
    ("hermes_bridge_auth", 8650, "/v1/capabilities", "auth"),
    ("honcho_auth", 8009, "/v3/ortak/protocol", "auth"),
)


class Deadline(Exception):
    """The fixed observation budget ended."""


def public_uuid(value):
    if not isinstance(value, str):
        raise ValueError("public UUID must be a string")
    parsed = UUID(value)
    if str(parsed) != value or parsed.int == 0:
        raise ValueError("canonical public UUID required")
    return value


def public_owner(value):
    if not re.fullmatch(r"[0-9a-f]{64}", value):
        raise ValueError("public owner key must be lowercase hex")
    return value


class HttpProbe:
    """Fixed no-auth GETs only; no proxy, redirect, body read or credential lookup."""

    def __init__(self):
        self.deadline = time.monotonic() + 10

    def __call__(self, endpoint):
        _, port, path, kind = endpoint
        if endpoint not in ENDPOINTS:
            raise ValueError("unselected health endpoint")
        remaining = self.deadline - time.monotonic()
        if remaining <= 0:
            return {"observation": "deadline"}
        connection = http.client.HTTPConnection("127.0.0.1", port, timeout=min(2, remaining))
        try:
            connection.request("GET", path, headers={"Accept": "application/json", "Connection": "close"})
            # Status-only observation. No response-body bytes are read or stored;
            # http.client also bounds each header line and the header count.
            response = connection.getresponse()
            status = response.status
            if kind == "health" and status == 200:
                outcome = "http_health_ok"
            elif kind == "auth" and status in {401, 403}:
                outcome = "authentication_required"
            else:
                outcome = "unexpected_http_status"
            return {"observation": outcome, "http_status": status}
        except Deadline:
            return {"observation": "deadline"}
        except (socket.timeout, TimeoutError):
            return {"observation": "timeout"}
        except (OSError, http.client.HTTPException):
            return {"observation": "unreachable_or_invalid_http"}
        finally:
            connection.close()


def inspect_api(root, expected_owner):
    config = json.loads(private_file(root / "api-config.json", 16384))
    if (set(config) != {"origin", "community_id", "humans", "allowed_web_origins"}
            or config["origin"] != "http://127.0.0.1:8787"
            or config["allowed_web_origins"] != ["http://localhost:1427", "tauri://localhost"]
            or len(config["humans"]) != 1):
        raise ValueError("private API selection differs")
    public_uuid(config["community_id"])
    human = config["humans"][0]
    if (set(human) - {"public_key", "role", "channel_ids", "employee_ids", "can_create_projects"}
            or not isinstance(human.get("can_create_projects", False), bool)
            or human["role"] != "operator" or human["employee_ids"] != ["ada-private"]
            or len(human["channel_ids"]) != 1):
        raise ValueError("private API audience differs")
    owner = public_owner(human["public_key"])
    public_uuid(human["channel_ids"][0])
    if expected_owner is not None and owner != expected_owner:
        return {"observation": "expected_owner_mismatch"}
    return {"observation": "valid_local_config", "expected_owner": "matches" if expected_owner else "not_supplied",
            "live_membership": "not_checked"}


def inspect_memory(root, expected_company):
    directory = root / "memory"
    metadata = directory.lstat()
    if (not stat.S_ISDIR(metadata.st_mode) or metadata.st_uid != os.getuid()
            or metadata.st_mode & 0o077):
        raise ValueError("private memory directory differs")
    config = json.loads(private_file(directory / "worker-memory.json", 16384))
    state = json.loads(private_file(directory / "bootstrap.json", 65536))
    if set(state) != {"intent", "resource_receipt", "resource_identity", "roundtrip_receipt", "completed"}:
        raise ValueError("private memory state differs")
    intent = state["intent"]
    company = public_uuid(intent["company_id"])
    memory.validate_intent(intent, company, intent["deployment_id"], intent["token_env"])
    if expected_company is not None and company != expected_company:
        return {"observation": "expected_company_mismatch"}
    if (state["completed"] is not True or state["resource_receipt"] != memory.expected_resource(intent)
            or config != memory.worker_config(intent)):
        raise ValueError("private memory receipts incomplete or changed")
    memory.validate_identity(intent, state["resource_identity"])
    memory.validate_write(intent, state["roundtrip_receipt"])
    return {"observation": "valid_local_bootstrap_receipts", "roundtrip": "historically_verified",
            "current_resource_identity": "not_checked", "current_execution_witness": "not_checked",
            "expected_company": "matches" if expected_company else "not_supplied"}


def inspect_image(root):
    directory = root / "object-store"
    metadata = directory.lstat()
    if (not stat.S_ISDIR(metadata.st_mode) or metadata.st_uid != os.getuid()
            or metadata.st_mode & 0o077):
        raise ValueError("private object-store directory differs")
    value = private_file(directory / "image.env", 256)
    matched = re.fullmatch(r"ORTAK_MINIO_IMAGE=(sha256:[0-9a-f]{64})\n", value)
    if not matched:
        raise ValueError("unbounded or floating image selection")
    return {"observation": "immutable_local_selection", "image_id": matched[1], "running_image": "not_checked"}


def inspect_worker_selection(root):
    try:
        config = json.loads(private_file(root / "worker-config.json", 16384))
    except FileNotFoundError:
        return {"observation": "not_configured"}
    if (not isinstance(config, dict)
            or set(config) - {"company_slug", "bridge_origin", "memory", "semantic", "office_signers", "office_relays",
                              "poll_interval_ms", "batch_limit"}
            or config.get("company_slug") != PROJECT
            or config.get("bridge_origin") != "http://127.0.0.1:8650"):
        raise ValueError("worker does not select this bounded private bridge origin")
    return {"observation": "local_origin_selected", "execution_configuration": "not_checked"}


def observed(operation):
    try:
        return operation()
    except FileNotFoundError:
        return {"observation": "missing"}
    except (OSError, ValueError, TypeError, KeyError, memory.Refused):
        return {"observation": "invalid_or_unreadable"}


def artifact(path, directory=False):
    metadata = path.lstat()
    correct_type = stat.S_ISDIR(metadata.st_mode) if directory else stat.S_ISREG(metadata.st_mode)
    if not correct_type or metadata.st_uid != os.getuid() or not metadata.st_mode & 0o111:
        raise ValueError("artifact type or ownership differs")
    return {"observation": "present", "build_provenance": "not_checked", "running": "not_checked"}


def collect(root, expected_owner=None, expected_company=None, probe=None):
    repo = Path(__file__).resolve().parents[2]
    binaries = Path("/private/tmp/ortak-root-build-target/debug")
    files = {
        "api_audience": observed(lambda: inspect_api(root, expected_owner)),
        "memory_bootstrap": observed(lambda: inspect_memory(root, expected_company)),
        "object_store_image": observed(lambda: inspect_image(root)),
        "worker_selection": observed(lambda: inspect_worker_selection(root)),
    }
    artifacts = {name: observed(lambda name=name: artifact(binaries / name))
                 for name in ("buzz-relay", "ortak-server", "ortak-worker")}
    artifacts["private_desktop_bundle"] = observed(lambda: artifact(
        repo / "desktop/src-tauri/target/ortak-private-native/debug/bundle/macos/Ortak Private.app", True))
    services = {}
    for entry in ENDPOINTS:
        if entry[0] == "hermes_bridge_auth" and files["worker_selection"]["observation"] != "local_origin_selected":
            services[entry[0]] = {"observation": files["worker_selection"]["observation"]}
        else:
            services[entry[0]] = probe(entry) if probe else {"observation": "not_checked"}
    return {"format": "ortak-private-status/1", "project": PROJECT,
            "observed_at": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
            "private_state": "marker_verified_by_cli", "local_configuration": files,
            "artifacts": artifacts, "services": services,
            "control_authority": {"employee_status": "not_checked", "employee_activation": "not_checked",
                                  "central_routing": "not_checked", "worker_running": "not_checked"},
            "workflow": {"provider_execution": "not_checked", "semantic_scoring": "not_checked", "office_reply": "not_checked",
                         "backup_restore": "not_checked", "upgrade": "not_checked"},
            "actions": {"mutation": False, "credentials_loaded": False, "activation": False}}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--state-dir", type=Path, required=True)
    parser.add_argument("--expected-owner", type=public_owner)
    parser.add_argument("--expected-company", type=public_uuid)
    parser.add_argument("--no-network", action="store_true")
    args = parser.parse_args()
    root = selected_root(args.state_dir)
    def deadline(_signal, _frame):
        raise Deadline()
    previous = signal.signal(signal.SIGALRM, deadline)
    signal.setitimer(signal.ITIMER_REAL, 12)
    try:
        result = collect(root, args.expected_owner, args.expected_company,
                         None if args.no_network else HttpProbe())
    finally:
        signal.setitimer(signal.ITIMER_REAL, 0)
        signal.signal(signal.SIGALRM, previous)
    print(json.dumps(result, sort_keys=True, separators=(",", ":")))


if __name__ == "__main__":
    try:
        main()
    except Exception:
        raise SystemExit("Private status observation failed; no state changed and no secrets logged.") from None
