"""Real HTTP handlers/transactions: replay, races, and rollback after message flush."""

import asyncio
import json
import subprocess
import sys
import tempfile
from uuid import uuid4

from sqlalchemy import select, text
from src import models as native
from src.config import settings
from src.db import SessionLocal, engine

from .conftest import bundle, counts, endpoint, memory_body, resource_body


async def test_lost_ack_replay_after_pool_restart_and_conflict(client):
    owner = await bundle(client)
    body = memory_body(owner)
    first = await client.post(endpoint(owner), json=body)
    assert first.status_code == 201, first.text
    initial = await counts(owner)
    assert initial[0:3] == (1, 1, 1)
    # Fresh interpreter + connection: no process-local replay cache survives.
    await engine.dispose()
    with tempfile.TemporaryFile() as output:
        await asyncio.to_thread(
            subprocess.run,
            [sys.executable, "-m", "ortak_tests.restart_probe"],
            input=json.dumps({"path": endpoint(owner), "body": body}).encode(),
            stdout=output,
            stderr=subprocess.DEVNULL,
            timeout=15,
            check=True,
        )
        assert output.tell() < 32768
        output.seek(0)
        second = json.load(output)
    assert second["status"] == 200 and second["body"] == first.json()
    assert await counts(owner) == initial
    body["facts"][0]["content"] = "different payload"
    conflict = await client.post(endpoint(owner), json=body)
    assert conflict.status_code == 409
    assert await counts(owner) == initial
    distinct_key = memory_body(owner, key="different_key_same_content")
    assert (await client.post(endpoint(owner), json=distinct_key)).status_code == 201
    assert (await counts(owner))[0:3] == (2, 2, 2)


async def test_concurrent_identical_writes_have_one_message_and_queue(client):
    owner = await bundle(client)
    body = memory_body(owner)
    responses = await asyncio.gather(
        *(client.post(endpoint(owner), json=body) for _ in range(6))
    )
    assert sorted(r.status_code for r in responses) == [200, 200, 200, 200, 200, 201]
    assert all(r.json() == responses[0].json() for r in responses)
    assert (await counts(owner))[0:3] == (1, 1, 1)


async def test_database_queue_failure_rolls_back_messages_embeddings_session_and_receipt(
    client, monkeypatch
):
    owner = await bundle(client)
    monkeypatch.setattr(settings, "EMBED_MESSAGES", True)
    name = "ortak_test_" + uuid4().hex
    async with engine.begin() as db:
        # Test-owned trigger fails the real queue INSERT after native message flush.
        await db.execute(
            text(
                f"CREATE FUNCTION {name}() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'injected queue failure'; END $$"
            )
        )
        await db.execute(
            text(
                f"CREATE TRIGGER {name} BEFORE INSERT ON queue FOR EACH ROW EXECUTE FUNCTION {name}()"
            )
        )
    try:
        failure = await client.post(endpoint(owner), json=memory_body(owner))
        assert failure.status_code == 503, failure.text
        assert await counts(owner) == (0, 0, 0, 0)
        async with SessionLocal() as db:
            assert (
                await db.scalar(
                    select(native.Session.id).where(
                        native.Session.workspace_name == owner["workspace_id"]
                    )
                )
                is None
            )
    finally:
        async with engine.begin() as db:
            await db.execute(text(f"DROP TRIGGER {name} ON queue"))
            await db.execute(text(f"DROP FUNCTION {name}()"))
    retried = await client.post(endpoint(owner), json=memory_body(owner))
    assert retried.status_code == 201, retried.text
    assert await counts(owner) == (1, 1, 1, 1)


async def test_create_replay_collision_and_concurrent_conflict_preserve_resources(
    client,
):
    body = resource_body()
    created = await client.post("/v3/ortak/resources/create", json=body)
    assert created.status_code == 201
    replay = await client.post("/v3/ortak/resources/create", json=body)
    assert replay.status_code == 200 and replay.json() == created.json()
    collision = {**body, "idempotency_key": "other", "employee_peer": "intruder"}
    results = await asyncio.gather(
        *(client.post("/v3/ortak/resources/create", json=collision) for _ in range(3))
    )
    assert all(r.status_code == 409 for r in results)
    async with SessionLocal() as db:
        peers = (
            await db.scalars(
                select(native.Peer).where(
                    native.Peer.workspace_name == body["workspace_id"]
                )
            )
        ).all()
        assert {p.name for p in peers} == {"operator", "employee"}
        assert all(
            p.h_metadata["ortak"]["employee_id"] == body["employee_id"] for p in peers
        )
    changed = {**body, "employee_peer": "different"}
    assert (
        await client.post("/v3/ortak/resources/create", json=changed)
    ).status_code == 409


async def test_foreign_native_workspace_is_never_modified(client):
    body = resource_body()
    async with SessionLocal() as db:
        ws = native.Workspace(
            name=body["workspace_id"], h_metadata={"preserve": "workspace"}
        )
        db.add(ws)
        await db.flush()
        db.add(
            native.Peer(
                workspace_name=body["workspace_id"],
                name="employee",
                h_metadata={"preserve": "peer"},
            )
        )
        await db.commit()
    assert (
        await client.post("/v3/ortak/resources/create", json=body)
    ).status_code == 409
    async with SessionLocal() as db:
        ws = await db.scalar(
            select(native.Workspace).where(
                native.Workspace.name == body["workspace_id"]
            )
        )
        peer = await db.scalar(
            select(native.Peer).where(
                native.Peer.workspace_name == body["workspace_id"]
            )
        )
        assert ws.h_metadata == {"preserve": "workspace"} and peer.h_metadata == {
            "preserve": "peer"
        }


async def test_concurrent_resource_replay_has_one_created_bundle(client):
    body = resource_body()
    responses = await asyncio.gather(
        *(client.post("/v3/ortak/resources/create", json=body) for _ in range(4))
    )
    assert sorted(r.status_code for r in responses) == [200, 200, 200, 201]
    assert all(r.json() == responses[0].json() for r in responses)


async def test_create_peer_failure_rolls_back_workspace_and_receipt(client):
    body = resource_body()
    name = "ortak_test_" + uuid4().hex
    async with engine.begin() as db:
        await db.execute(
            text(
                f"CREATE FUNCTION {name}() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'injected peer failure'; END $$"
            )
        )
        await db.execute(
            text(
                f"CREATE TRIGGER {name} BEFORE INSERT ON peers FOR EACH ROW EXECUTE FUNCTION {name}()"
            )
        )
    try:
        failure = await client.post("/v3/ortak/resources/create", json=body)
        assert failure.status_code == 503, failure.text
        async with SessionLocal() as db:
            assert (
                await db.scalar(
                    select(native.Workspace.id).where(
                        native.Workspace.name == body["workspace_id"]
                    )
                )
                is None
            )
    finally:
        async with engine.begin() as db:
            await db.execute(text(f"DROP TRIGGER {name} ON peers"))
            await db.execute(text(f"DROP FUNCTION {name}()"))
    assert (
        await client.post("/v3/ortak/resources/create", json=body)
    ).status_code == 201
