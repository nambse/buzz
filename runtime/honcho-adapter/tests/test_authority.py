"""Authority mutations during provider I/O must survive ORM identity caching."""

import asyncio

from sqlalchemy import text, update
from src import models as native
from src.config import settings
from src.db import engine
from src.utils.search import embedding_client

from .conftest import bundle, endpoint, memory_body


async def test_ownership_change_while_provider_waits_is_revalidated(
    client, monkeypatch
):
    owner = await bundle(client)
    written = await client.post(endpoint(owner), json=memory_body(owner))
    assert written.status_code == 201
    monkeypatch.setattr(settings, "EMBED_MESSAGES", True)
    entered, release = asyncio.Event(), asyncio.Event()

    async def paused_provider(*args, **kwargs):
        entered.set()
        await release.wait()
        return [1.0] + [0.0] * (settings.EMBEDDING.VECTOR_DIMENSIONS - 1)

    monkeypatch.setattr(embedding_client, "embed", paused_provider)
    query = {
        "company_id": owner["company_id"],
        "employee_id": owner["employee_id"],
        "scope": {"scope": "employee_experience"},
        "query": "durable memory",
    }
    request = asyncio.create_task(client.post(endpoint(owner, "recall"), json=query))
    try:
        await asyncio.wait_for(entered.wait(), 5)
        async with engine.begin() as db:
            await db.execute(text("SET LOCAL lock_timeout = '500ms'"))
            await db.execute(
                update(native.Workspace)
                .where(native.Workspace.name == owner["workspace_id"])
                .values(
                    h_metadata={
                        "ortak": {
                            "protocol": "ortak-honcho/1",
                            "company_id": owner["company_id"],
                            "employee_id": "changed-owner",
                        }
                    }
                )
            )
    finally:
        release.set()
    result = await asyncio.wait_for(request, 5)
    assert result.status_code == 409, result.text
    assert result.json()["detail"] == "resource_identity_changed"
