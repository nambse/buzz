"""Scoped full-text recall with real PostgreSQL and canonical receipt checks."""

import asyncio
from uuid import uuid4

from sqlalchemy import select, text, update
from src import models as native
from src.config import settings
from src.db import SessionLocal, engine
from src.security import JWTParams, create_jwt
from src.utils.search import embedding_client

from .conftest import bundle, counts, endpoint, memory_body


def recall_body(owner, scope=None, **overrides):
    return {
        "company_id": owner["company_id"],
        "employee_id": owner["employee_id"],
        "scope": scope or {"scope": "employee_experience"},
        "query": "durable memory",
        "max_records": 32,
        "max_bytes": 16384,
        **overrides,
    }


async def test_scoped_recall_is_nonempty_and_never_crosses_session_or_company(client):
    owner, other = await bundle(client), await bundle(client)
    project = {"scope": "project_context", "project_id": str(uuid4())}
    first = await client.post(endpoint(owner), json=memory_body(owner))
    await client.post(
        endpoint(owner, session="project"),
        json=memory_body(
            owner, scope=project, content="Different project durable memory"
        ),
    )
    await client.post(
        endpoint(other), json=memory_body(other, content="Other company durable memory")
    )
    recalled = await client.post(endpoint(owner, "recall"), json=recall_body(owner))
    assert recalled.status_code == 200, recalled.text
    assert [r["record_ref"] for r in recalled.json()["records"]] == first.json()[
        "record_refs"
    ]
    assert (
        recalled.json()["records"][0]["provenance"]["employee_id"]
        == owner["employee_id"]
    )
    mismatch = await client.post(
        endpoint(owner, "recall", "project"), json=recall_body(owner)
    )
    assert mismatch.status_code == 409
    wrong_company = await client.post(
        endpoint(owner, "recall"), json=recall_body(other)
    )
    assert wrong_company.status_code == 409
    bounded = await client.post(
        endpoint(owner, "recall"), json=recall_body(owner, max_bytes=1)
    )
    assert bounded.json() == {"records": [], "truncated": True}


async def test_missing_session_recall_is_read_only_and_mutated_message_is_refused(
    client,
):
    owner = await bundle(client)
    empty = await client.post(endpoint(owner, "recall"), json=recall_body(owner))
    assert empty.json() == {"records": [], "truncated": False}
    async with SessionLocal() as db:
        assert (
            await db.scalar(
                select(native.Session.id).where(
                    native.Session.workspace_name == owner["workspace_id"]
                )
            )
            is None
        )
    written = await client.post(endpoint(owner), json=memory_body(owner))
    async with engine.begin() as db:
        await db.execute(
            update(native.Message)
            .where(native.Message.public_id == written.json()["record_refs"][0])
            .values(content="Mutated durable memory")
        )
    refused = await client.post(endpoint(owner, "recall"), json=recall_body(owner))
    assert (
        refused.status_code == 409
        and refused.json()["detail"] == "message_provenance_conflict"
    )

    replay = await client.post(endpoint(owner), json=memory_body(owner))
    assert replay.status_code == 409
    assert replay.json()["detail"] == "remembered_message_changed"


async def test_provider_failure_is_an_error_and_provider_wait_holds_no_resource_lock(
    client, monkeypatch
):
    owner = await bundle(client)
    await client.post(endpoint(owner), json=memory_body(owner))
    monkeypatch.setattr(settings, "EMBED_MESSAGES", True)
    entered, release = asyncio.Event(), asyncio.Event()

    async def failed_provider(*args, **kwargs):
        entered.set()
        await release.wait()
        raise RuntimeError("injected provider failure")

    monkeypatch.setattr(embedding_client, "embed", failed_provider)
    request = asyncio.create_task(
        client.post(endpoint(owner, "recall"), json=recall_body(owner))
    )
    try:
        await asyncio.wait_for(entered.wait(), 5)
        async with engine.begin() as db:
            await db.execute(text("SET LOCAL lock_timeout = '500ms'"))
            # Must acquire while embedding is pending, proving no held resource lock.
            await db.execute(
                select(native.Workspace.id)
                .where(native.Workspace.name == owner["workspace_id"])
                .with_for_update()
            )
    finally:
        release.set()
    response = await asyncio.wait_for(request, 5)
    assert response.status_code == 500


async def test_native_auth_and_request_bounds_cannot_be_bypassed(client):
    owner = await bundle(client)
    wrong_workspace = create_jwt(JWTParams(w="foreign_workspace", s="experience"))
    denied = await client.post(
        endpoint(owner),
        json=memory_body(owner),
        headers={"Authorization": "Bearer " + wrong_workspace},
    )
    assert denied.status_code in {401, 403}
    oversized = memory_body(owner, content="x" * 16385)
    assert (await client.post(endpoint(owner), json=oversized)).status_code == 422
    invalid = memory_body(owner)
    invalid["facts"][0]["provenance"]["employee_id"] = "different_employee"
    assert (await client.post(endpoint(owner), json=invalid)).status_code == 422
    invalid = memory_body(owner)
    invalid["facts"][0]["provenance"]["recorded_at"] = "2026-09-05T00:00:00"
    assert (await client.post(endpoint(owner), json=invalid)).status_code == 422
    huge = await client.post(
        endpoint(owner),
        content=b" " * (1152 * 1024 + 1),
        headers={"Content-Type": "application/json"},
    )
    assert huge.status_code == 413
    assert await counts(owner) == (0, 0, 0, 0)
