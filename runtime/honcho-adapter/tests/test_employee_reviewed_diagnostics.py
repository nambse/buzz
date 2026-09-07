"""Real finite write/read/cleanup evidence, with irreversible pre-write erasure."""

import asyncio
import json
from uuid import uuid4

from sqlalchemy import select
from src.db import SessionLocal, engine
from ortak_honcho.reviewed_employee_models import diagnostics, diagnostic_tombstones

from .conftest import counts
from .employee_reviewed_fixture import base, canonical, diagnostic, diagnostic_read, namespace, selected, sha, stored


def path(owner, operation, action):
    return f"{base(owner)}/diagnostics/{operation}/{action}"


def commitment(common, request, operation, *, withdraw=False):
    identity = namespace(common)
    field = "challenge_hash" if withdraw else "challenge"
    return sha(canonical({"format": "ortak-reviewed-employee-diagnostic-withdraw/1" if withdraw
                         else "ortak-reviewed-employee-diagnostic/1",
        "operation_id": operation, "namespace_hash": identity["namespace_hash"],
        "binding_hash": identity["binding_hash"], "employee_revision_id": request["employee_revision_id"],
        "employee_lifecycle_epoch": request["employee_lifecycle_epoch"], field: request[field]}))


async def test_employee_diagnostic_one_exact_readback_then_confirmed_cleanup_replay(client):
    owner, common = await selected(client)
    operation, request = str(uuid4()), diagnostic(common)
    read = diagnostic_read(request)
    assert (await client.post(path(owner, operation, "read"), json=read)).status_code == 409
    assert await stored(owner) == (0, 0, 0, 0, 0, 0, 0)
    first = await client.post(path(owner, operation, "write"), json=request)
    assert first.status_code == 201, first.text
    written = first.json()
    assert written["challenge"] is None and not written["erased"]
    assert written["write_request_hash"] == commitment(common, request, operation)
    assert written["withdraw_request_hash"] is None
    observed = await client.post(path(owner, operation, "read"), json=read)
    assert observed.status_code == 200
    assert observed.json() == {**written, "challenge": request["challenge"]}
    assert len(observed.content) <= 65536
    erased = await client.post(path(owner, operation, "withdraw"), json=read)
    assert erased.status_code == 200 and erased.json()["erased"]
    assert erased.json()["challenge"] is None
    assert erased.json()["withdraw_request_hash"] == commitment(common, read, operation, withdraw=True)
    await engine.dispose()
    for action, body in (("write", request), ("withdraw", read), ("read", read)):
        replay = await client.post(path(owner, operation, action), json=body)
        assert replay.status_code == 200 and replay.json() == erased.json()
    assert await stored(owner) == (0, 0, 0, 0, 1, 0, 1)
    assert await counts(owner) == (0, 0, 0, 0)
    async with SessionLocal() as db:
        for table in (diagnostics, diagnostic_tombstones):
            rows = (await db.execute(select(table).where(table.c.workspace_id == owner["workspace_id"]))).mappings().all()
            assert request["challenge"] not in json.dumps([dict(row) for row in rows], default=str)


async def test_employee_diagnostic_cleanup_before_write_and_concurrent_arrival_keep_absence(client):
    owner, common = await selected(client)
    operation, request = str(uuid4()), diagnostic(common)
    read = diagnostic_read(request)
    erased = await client.post(path(owner, operation, "withdraw"), json=read)
    assert erased.status_code == 200 and erased.json()["erased"]
    assert erased.json()["write_request_hash"] is None
    assert await stored(owner) == (0, 0, 0, 0, 0, 0, 1)
    for key, value in (("employee_revision_id", str(uuid4())), ("employee_lifecycle_epoch", 4),
                       ("deployment_id", str(uuid4())), ("challenge_hash", "f" * 64)):
        bad = await client.post(path(owner, operation, "withdraw"), json={**read, key: value})
        assert bad.status_code == 409
    late = await client.post(path(owner, operation, "write"), json=request)
    assert late.status_code == 201 and late.json()["erased"] and late.json()["challenge"] is None
    assert late.json()["tombstone_at"] == erased.json()["tombstone_at"]
    observed = await client.post(path(owner, operation, "read"), json=read)
    assert observed.json() == late.json()  # never a fresh readback witness
    assert (await client.post(path(owner, operation, "write"), json={**request, "challenge": "2" * 64})).status_code == 409
    new_operation = str(uuid4())
    replies = await asyncio.gather(client.post(path(owner, new_operation, "write"), json=request),
                                  client.post(path(owner, new_operation, "withdraw"), json=read))
    assert sorted(r.status_code for r in replies) == [200, 201], [r.text for r in replies]
    current = await client.post(path(owner, new_operation, "read"), json=read)
    assert current.json()["erased"] and current.json()["challenge"] is None
    assert await stored(owner) == (0, 0, 0, 0, 2, 0, 2)


async def test_employee_diagnostic_rejects_unbounded_or_coerced_epoch_and_user_text(client):
    owner, common = await selected(client)
    operation, request = str(uuid4()), diagnostic(common)
    for epoch in (-1, 9223372036854775808, True, 1.0, "1"):
        assert (await client.post(path(owner, operation, "write"),
            json={**request, "employee_lifecycle_epoch": epoch})).status_code == 422
    assert (await client.post(path(owner, operation, "write"),
        json={**request, "challenge": "private user prose"})).json() == {"detail": "employee_request_invalid"}
    assert (await client.post(path(owner, operation, "write"),
        json={**request, "employee_revision_id": "00000000-0000-0000-0000-000000000000"})).status_code == 422
    assert await stored(owner) == (0, 0, 0, 0, 0, 0, 0)
