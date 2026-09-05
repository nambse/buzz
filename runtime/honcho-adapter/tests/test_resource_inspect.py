"""Immutable ownership inspection is read-only and rejects native replacements."""

from uuid import uuid4

from sqlalchemy import delete, func, select
from src import models as native
from src.db import SessionLocal
from src.security import JWTParams, create_jwt

from ortak_honcho.models import resource_receipts, session_ownership, write_receipts
from .conftest import bundle, resource_body


def inspection(owner):
    return {key: owner[key] for key in
            ("company_id", "employee_id", "user_peer", "employee_peer")}


def path(owner):
    return f"/v3/ortak/workspaces/{owner['workspace_id']}/resources/inspect"


async def snapshot(owner):
    """All extension-owned and native effect tables for this fresh workspace."""
    values = []
    async with SessionLocal() as db:
        for table in (native.Workspace.__table__, native.Peer.__table__,
                      native.Session.__table__, native.Message.__table__,
                      native.QueueItem.__table__, native.MessageEmbedding.__table__,
                      resource_receipts, session_ownership, write_receipts):
            column = (table.c.name if table is native.Workspace.__table__ else
                      table.c.workspace_id if "workspace_id" in table.c else table.c.workspace_name)
            values.append(await db.scalar(select(func.count()).select_from(table)
                                          .where(column == owner["workspace_id"])))
    return tuple(values)


async def test_resource_inspection_is_exact_scoped_and_never_creates(client):
    owner = await bundle(client)
    initial = await snapshot(owner)
    first = await client.post(path(owner), json=inspection(owner))
    second = await client.post(path(owner), json=inspection(owner))
    assert first.status_code == second.status_code == 200
    assert first.json() == second.json()
    result = first.json()
    assert result["ownership"] == "created"
    assert result["company_id"] == owner["company_id"]
    assert result["employee_id"] == owner["employee_id"]
    async with SessionLocal() as db:
        receipt = (await db.execute(select(resource_receipts).where(
            resource_receipts.c.workspace_id == owner["workspace_id"]))).mappings().one()
        assert result["request_hash"] == receipt["request_hash"]
        assert result["native_ids"] == receipt["native_ids"]
    assert await snapshot(owner) == initial
    missing = resource_body()
    before = await snapshot(missing)
    assert (await client.post(path(missing), json=inspection(missing))).status_code == 409
    assert await snapshot(missing) == before
    wrong = {**inspection(owner), "company_id": str(uuid4())}
    assert (await client.post(path(owner), json=wrong)).status_code == 409
    wrong_peer = {**inspection(owner), "employee_peer": "different_peer"}
    assert (await client.post(path(owner), json=wrong_peer)).status_code == 409
    foreign_token = create_jwt(JWTParams(w="foreign_workspace"))
    response = await client.post(path(owner), json=inspection(owner),
                                headers={"Authorization": "Bearer " + foreign_token})
    assert response.status_code in {401, 403}
    assert await snapshot(owner) == initial


async def test_native_peer_replacement_cannot_inherit_identical_metadata(client):
    owner = await bundle(client)
    before = await client.post(path(owner), json=inspection(owner))
    assert before.status_code == 200
    old_identity = before.json()["native_ids"]["peers"][owner["employee_peer"]]
    # No sessions/messages yet; replace only this test-owned peer through actual SQL.
    async with SessionLocal() as db:
        peer = await db.scalar(select(native.Peer).where(
            native.Peer.workspace_name == owner["workspace_id"],
            native.Peer.name == owner["employee_peer"]))
        metadata = dict(peer.h_metadata)
        await db.execute(delete(native.Peer).where(native.Peer.id == peer.id))
        replacement = native.Peer(name=owner["employee_peer"],
                                  workspace_name=owner["workspace_id"], h_metadata=metadata)
        db.add(replacement)
        await db.flush()
        assert replacement.id != old_identity
        await db.commit()
    initial = await snapshot(owner)
    rejected = await client.post(path(owner), json=inspection(owner))
    assert rejected.status_code == 409
    assert rejected.json()["detail"] == "resource_identity_changed"
    assert await snapshot(owner) == initial
    async with SessionLocal() as db:
        receipt = (await db.execute(select(resource_receipts).where(
            resource_receipts.c.workspace_id == owner["workspace_id"]))).mappings().one()
        assert receipt["native_ids"]["peers"][owner["employee_peer"]] == old_identity
