"""Fresh native ownership and independently constructed employee wire fixtures."""

import hashlib
import json
from datetime import datetime, timedelta, timezone
from uuid import uuid4

from sqlalchemy import func, select
from src.db import SessionLocal
from ortak_honcho.reviewed_employee_models import TABLES

from .conftest import bundle


def canonical(value):
    return json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=False, allow_nan=False)


def sha(value):
    return hashlib.sha256(value.encode()).hexdigest()


def stamp(value):
    return value.isoformat(timespec="microseconds").replace("+00:00", "Z")


async def selected(client):
    owner = await bundle(client)
    inspected = await client.post(
        f"/v3/ortak/workspaces/{owner['workspace_id']}/resources/inspect",
        json={k: owner[k] for k in ("company_id", "employee_id", "user_peer", "employee_peer")},
    )
    assert inspected.status_code == 200, inspected.text
    receipt = inspected.json()
    common = {"company_id": owner["company_id"], "employee_id": owner["employee_id"],
        "deployment_id": str(uuid4()), "binding": {"adapter": "honcho", "endpoint_ref": "honcho-test",
            "workspace": owner["workspace_id"], "user_peer": owner["user_peer"],
            "employee_peer": owner["employee_peer"], "options": {}},
        "ownership": {k: receipt[k] for k in ("request_hash", "native_ids")}}
    return owner, common


def base(owner):
    return f"/v3/ortak/workspaces/{owner['workspace_id']}/reviewed-employees/{owner['employee_id']}"


def url(owner, record, action="publish"):
    return f"{base(owner)}/records/{record}/{action}"


def namespace(common):
    value = canonical({"company_id": common["company_id"], "employee_id": common["employee_id"],
                       "format": "ortak-reviewed-employee-namespace/1"})
    namespace_hash = sha(value)
    return {**common, "protocol": "reviewed-employee/1", "namespace": value,
            "namespace_hash": namespace_hash,
            "binding_hash": sha(canonical({"binding": common["binding"], "namespace_hash": namespace_hash,
                                           "protocol": "reviewed-employee/1"}))}


def publish(common, record, *, content="A human-reviewed employee fact", kind="experience",
            destination=None, expires=None):
    community, destination = str(uuid4()), destination or str(uuid4())
    audience = {"company_id": common["company_id"], "employee_id": common["employee_id"],
        "format": "ortak-reviewed-employee-audience/1", "kind": kind,
        "destination_community_id": community, "destination_channel_id": destination,
        "human_public_key": "b" * 64 if kind == "relationship" else None}
    source = {"community_id": community, "channel_id": str(uuid4()), "event_id": "a" * 64,
        "event_created_at": "2026-09-06T00:01:02.123456Z", "author_public_key": "b" * 64,
        "evidence_hash": "d" * 64}
    audience_hash = sha(canonical(audience))
    source_hash = sha(canonical({"format": "ortak-reviewed-employee-source/1",
                                "audience_hash": audience_hash, "source": source}))
    provenance = canonical({"format": "ortak-reviewed-employee-provenance/1", "audience": audience,
        "audience_hash": audience_hash, "source": source, "source_hash": source_hash,
        "approval": {"format": "ortak-reviewed-employee-sharing/1", "approval_id": str(uuid4()),
            "approved_by": "b" * 64, "content_hash": sha(content),
            "expires_at": stamp(expires or datetime.now(timezone.utc) + timedelta(days=1))}})
    return {**common, "target_id": str(uuid4()), "destination_channel_id": destination,
        "idempotency_key": f"employee-reviewed:publish:{common['company_id']}:{record}",
        "content": content, "content_hash": sha(content), "source_hash": source_hash,
        "provenance": provenance, "sharing_hash": sha(provenance)}


def withdraw(request, record):
    return {**{k: v for k, v in request.items() if k not in {"content", "provenance", "idempotency_key"}},
            "idempotency_key": f"employee-reviewed:withdraw:{request['company_id']}:{record}"}


def remote_hash(common, request, record, action):
    identity = namespace(common)
    return sha(canonical({"action": action, "binding_hash": identity["binding_hash"],
        "company_id": common["company_id"], "content_hash": request["content_hash"],
        "employee_id": common["employee_id"], "fact_id": record,
        "format": "ortak-reviewed-employee-remote-request/1", "namespace_hash": identity["namespace_hash"],
        "sharing_hash": request["sharing_hash"], "source_hash": request["source_hash"],
        "target_id": request["target_id"]}))


async def stored(owner):
    async with SessionLocal() as db:
        return tuple([await db.scalar(select(func.count()).select_from(table).where(
            table.c.workspace_id == owner["workspace_id"])) for table in TABLES])


def recall(common, destination, ids, human=None):
    return {**common, "destination_channel_id": destination, "human_public_key": human, "record_ids": ids}


def diagnostic(common):
    return {**common, "employee_revision_id": str(uuid4()), "employee_lifecycle_epoch": 3,
            "challenge": "1" * 64}


def diagnostic_read(request):
    return {**{k: v for k, v in request.items() if k != "challenge"},
            "challenge_hash": sha(request["challenge"])}
