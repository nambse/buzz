"""Actual employee-family HTTP/PG admission, replay, withdrawal and selected reads."""

import asyncio
import importlib
import json
from datetime import datetime, timedelta, timezone
from uuid import uuid4

from sqlalchemy import select
from src.config import settings
from src.db import SessionLocal, engine

from ortak_honcho.reviewed_employee_models import records, operations, tombstones
from .conftest import counts
from .employee_reviewed_fixture import base, namespace, publish, recall, remote_hash, selected, stored, url, withdraw


async def test_employee_publish_actual_replay_restart_and_no_native_derivation(client, monkeypatch):
    owner, common = await selected(client)
    record = str(uuid4())
    request = publish(common, record)
    monkeypatch.setattr(settings, "EMBED_MESSAGES", True)
    from src import crud
    from ortak_honcho import service
    enqueue = importlib.import_module("src.deriver.enqueue")

    async def forbidden(*_args, **_kwargs):
        raise AssertionError("employee reviewed records must not enter native message/embedding queues")

    monkeypatch.setattr(crud, "create_messages", forbidden)
    monkeypatch.setattr(enqueue, "handle_session", forbidden)
    monkeypatch.setattr(service, "handle_session", forbidden)
    responses = await asyncio.gather(*(client.post(url(owner, record), json=request) for _ in range(4)))
    assert sorted(r.status_code for r in responses) == [200, 200, 200, 201], [r.text for r in responses]
    value = responses[0].json()
    assert all(r.json() == value for r in responses)
    assert value["content"] is None and value["status"] == "active"
    assert value["provenance"] == request["provenance"]
    assert value["request_hash"] == remote_hash(common, request, record, "publish")
    assert value["binding_hash"] == namespace(common)["binding_hash"]
    assert await stored(owner) == (1, 1, 0, 1, 0, 0, 0)
    assert await counts(owner) == (0, 0, 0, 0)
    await engine.dispose()
    retried = await client.post(url(owner, record), json=request)
    assert retried.status_code == 200 and retried.json() == value
    changed = publish(common, record, content="A different fact on the same immutable key")
    assert (await client.post(url(owner, record), json=changed)).status_code == 409
    # Deployment is deliberately not in the public typed request commitment;
    # this catches a missing independent full-body/pin comparison on replay.
    changed = {**request, "deployment_id": str(uuid4())}
    assert remote_hash(changed, changed, record, "publish") == value["request_hash"]
    assert (await client.post(url(owner, record), json=changed)).status_code == 409
    assert (await client.post(url(owner, record), json={**request, "idempotency_key": "fresh-key"})).status_code == 409
    async with SessionLocal() as db:
        for table in (records, operations, tombstones):
            rows = (await db.execute(select(table).where(table.c.workspace_id == owner["workspace_id"]))).mappings().all()
            assert request["content"] not in json.dumps([dict(row) for row in rows], default=str)


async def test_employee_withdraw_before_expired_delayed_publish_and_changed_cleanup_pins(client):
    owner, common = await selected(client)
    record = str(uuid4())
    request = publish(common, record, expires=datetime.now(timezone.utc) - timedelta(days=1))
    assert (await client.post(url(owner, record), json=request)).status_code == 409
    assert await stored(owner) == (0, 0, 0, 0, 0, 0, 0)
    stop = withdraw(request, record)
    first = await client.post(url(owner, record, "withdraw"), json=stop)
    assert first.status_code == 200, first.text
    value = first.json()
    assert value["erased_from_reviewed_store"] and value["provenance"] is None and value["expires_at"] is None
    assert value["request_hash"] == remote_hash(common, stop, record, "withdraw")
    assert await stored(owner) == (0, 0, 1, 1, 0, 0, 0)
    for key in ("target_id", "destination_channel_id", "deployment_id", "content_hash", "source_hash", "sharing_hash"):
        replacement = "f" * 64 if key.endswith("hash") else str(uuid4())
        changed = {**stop, key: replacement}
        assert (await client.post(url(owner, record, "withdraw"), json=changed)).status_code == 409
    late = await client.post(url(owner, record), json=request)
    assert late.status_code == 201, late.text
    assert late.json()["status"] == "withdrawn" and late.json()["erased_from_reviewed_store"]
    assert late.json()["provenance"] == request["provenance"] and late.json()["content"] is None
    await engine.dispose()
    assert (await client.post(url(owner, record), json=request)).json() == late.json()
    replay = await client.post(url(owner, record, "withdraw"), json=stop)
    assert replay.json() == {**late.json(), "request_hash": value["request_hash"]}
    assert await stored(owner) == (1, 0, 1, 2, 0, 0, 0)
    assert (await client.post(base(owner) + "/recall-selected",
        json=recall(common, request["destination_channel_id"], [record]))).json() == {"records": [], "truncated": False}


async def test_employee_concurrent_publish_withdraw_never_resurrects(client):
    owner, common = await selected(client)
    record = str(uuid4())
    request = publish(common, record)
    replies = await asyncio.gather(client.post(url(owner, record), json=request),
        client.post(url(owner, record, "withdraw"), json=withdraw(request, record)))
    assert sorted(r.status_code for r in replies) == [200, 201], [r.text for r in replies]
    assert await stored(owner) == (1, 0, 1, 2, 0, 0, 0)
    assert (await client.post(url(owner, record), json=request)).json()["erased_from_reviewed_store"]
    assert await counts(owner) == (0, 0, 0, 0)


async def test_employee_selected_ids_filter_destination_human_and_original_order_before_budget(client):
    owner, common = await selected(client)
    destination = str(uuid4())
    ids = [str(uuid4()) for _ in range(4)]
    for index, record in enumerate(ids):
        request = publish(common, record, content=str(index) + "ç" * 1499,
            kind="relationship" if index == 1 else "experience",
            destination=str(uuid4()) if index == 3 else destination)
        assert (await client.post(url(owner, record), json=request)).status_code == 201
    before = await stored(owner)
    result = await client.post(base(owner) + "/recall-selected",
        json=recall(common, destination, [ids[3], ids[1], ids[2], ids[0]], "b" * 64))
    assert result.status_code == 200 and len(result.content) <= 65536
    assert [r["record_id"] for r in result.json()["records"]] == [ids[1], ids[2]]
    assert result.json()["truncated"]
    assert sum(len(r["content"].encode()) for r in result.json()["records"]) == 5998
    assert all("request_hash" not in r for r in result.json()["records"])
    wrong_human = await client.post(base(owner) + "/recall-selected",
        json=recall(common, destination, [ids[1], ids[0]], "c" * 64))
    assert [r["record_id"] for r in wrong_human.json()["records"]] == [ids[0]]
    assert (await client.post(base(owner) + "/recall-selected",
        json=recall(common, destination, [ids[1]]))).json() == {"records": [], "truncated": False}
    assert await stored(owner) == before and await counts(owner) == (0, 0, 0, 0)


async def test_employee_current_expiry_hides_text_without_claiming_erasure(client):
    owner, common = await selected(client)
    record = str(uuid4())
    request = publish(common, record, expires=datetime.now(timezone.utc) + timedelta(seconds=2))
    first = await client.post(url(owner, record), json=request)
    assert first.status_code == 201, first.text
    await asyncio.sleep(2.1)
    selected_reply = await client.post(base(owner) + "/recall-selected",
        json=recall(common, request["destination_channel_id"], [record]))
    assert selected_reply.json() == {"records": [], "truncated": False}
    expired = await client.post(url(owner, record), json=request)
    assert expired.json()["status"] == "expired" and not expired.json()["erased_from_reviewed_store"]
    assert await stored(owner) == (1, 1, 0, 1, 0, 0, 0)
    removed = await client.post(url(owner, record, "withdraw"), json=withdraw(request, record))
    assert removed.status_code == 200 and removed.json()["erased_from_reviewed_store"]
    assert await stored(owner) == (1, 0, 1, 2, 0, 0, 0)
