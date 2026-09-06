"""Actual extension HTTP/PG lifecycle; no provider, native queue or secret access."""

import asyncio
import hashlib
import importlib
import json
from datetime import datetime, timedelta, timezone
from uuid import uuid4

import pytest
from sqlalchemy import func, select, text, update
from sqlalchemy.exc import DBAPIError
from src.config import settings
from src.db import SessionLocal, engine
from ortak_honcho.reviewed_models import contents, operations, records, tombstones

from .conftest import bundle, counts


def audience(owner):
    return {"company_id": owner["company_id"], "employee_id": owner["employee_id"]}


def body(owner, content="Reviewed project deployment fact"):
    return {
        **audience(owner),
        "idempotency_key": "publish_" + uuid4().hex,
        "content": content,
        "content_hash": hashlib.sha256(content.encode()).hexdigest(),
        "source_hash": "a" * 64,
        "approved_by": "b" * 64,
        "approval_id": str(uuid4()),
        "expires_at": (datetime.now(timezone.utc) + timedelta(days=1)).isoformat(),
    }


def base(owner, project):
    return f"/v3/ortak/workspaces/{owner['workspace_id']}/reviewed-projects/{project}"


def record_url(owner, project, record, action="publish"):
    return f"{base(owner, project)}/records/{record}/{action}"


def mutation(owner, key=None):
    return {**audience(owner), "idempotency_key": key or "withdraw_" + uuid4().hex}


async def stored(owner):
    async with SessionLocal() as db:
        return tuple(
            [
                await db.scalar(
                    select(func.count())
                    .select_from(table)
                    .where(table.c.workspace_id == owner["workspace_id"])
                )
                for table in (records, contents, tombstones, operations)
            ]
        )


async def test_reviewed_publish_restart_replay_is_one_hash_receipt_and_never_derives(
    client, monkeypatch
):
    owner, project, record = await bundle(client), uuid4(), uuid4()
    request = body(owner)
    monkeypatch.setattr(settings, "EMBED_MESSAGES", True)
    from src import crud
    from ortak_honcho import service

    enqueue = importlib.import_module("src.deriver.enqueue")

    async def forbidden(*_args, **_kwargs):
        raise AssertionError(
            "reviewed records must never enter native derivation or embedding"
        )

    monkeypatch.setattr(crud, "create_messages", forbidden)
    monkeypatch.setattr(enqueue, "handle_session", forbidden)
    monkeypatch.setattr(service, "handle_session", forbidden)
    replies = await asyncio.gather(
        *(
            client.post(record_url(owner, project, record), json=request)
            for _ in range(4)
        )
    )
    assert sorted(reply.status_code for reply in replies) == [200, 200, 200, 201]
    assert all(reply.json() == replies[0].json() for reply in replies)
    assert replies[0].json()["content"] is None  # acknowledgements contain no text
    assert await stored(owner) == (1, 1, 0, 1)
    assert await counts(owner) == (0, 0, 0, 0)
    await engine.dispose()
    retry = await client.post(record_url(owner, project, record), json=request)
    assert retry.status_code == 200 and retry.json() == replies[0].json()
    async with SessionLocal() as db:
        for table in (records, operations):
            rows = (
                (
                    await db.execute(
                        select(table).where(
                            table.c.workspace_id == owner["workspace_id"]
                        )
                    )
                )
                .mappings()
                .all()
            )
            assert request["content"] not in json.dumps(
                [dict(row) for row in rows], default=str
            )
    changed = body(owner, "different reviewed text")
    changed["idempotency_key"] = request["idempotency_key"]
    assert (
        await client.post(record_url(owner, project, record), json=changed)
    ).status_code == 409
    assert (
        await client.post(
            record_url(owner, project, record),
            json={**request, "idempotency_key": "new_key"},
        )
    ).status_code == 409
    recall = await client.post(
        base(owner, project) + "/recall",
        json={**audience(owner), "query": "deployment"},
    )
    assert recall.status_code == 200
    assert recall.json()["records"][0]["content"] == request["content"]
    assert await stored(owner) == (1, 1, 0, 1)


async def test_reviewed_withdrawal_before_delayed_publish_and_retries_never_resurrect(
    client,
):
    owner, project, record = await bundle(client), uuid4(), uuid4()
    request, stop = body(owner), mutation(owner)
    endpoint = record_url(owner, project, record, "withdraw")
    removed = await client.post(endpoint, json=stop)
    assert removed.status_code == 200 and removed.json()["erased_from_reviewed_store"]
    assert await stored(owner) == (0, 0, 1, 1)
    published = await client.post(record_url(owner, project, record), json=request)
    assert published.status_code == 201 and published.json()["status"] == "withdrawn"
    assert published.json()["content"] is None
    assert await stored(owner) == (1, 0, 1, 2)
    await engine.dispose()
    assert (await client.post(endpoint, json=stop)).json() == published.json() | {
        "request_hash": removed.json()["request_hash"]
    }
    assert (
        await client.post(record_url(owner, project, record), json=request)
    ).json() == published.json()
    assert await stored(owner) == (1, 0, 1, 2)
    assert await counts(owner) == (0, 0, 0, 0)


async def test_reviewed_concurrent_publish_and_withdraw_has_no_remaining_text(client):
    owner, project, record = await bundle(client), uuid4(), uuid4()
    results = await asyncio.gather(
        client.post(record_url(owner, project, record), json=body(owner)),
        client.post(
            record_url(owner, project, record, "withdraw"), json=mutation(owner)
        ),
    )
    assert sorted(result.status_code for result in results) == [200, 201]
    assert await stored(owner) == (1, 0, 1, 2)
    page = await client.post(base(owner, project) + "/inspect", json=audience(owner))
    assert page.json()["records"][0]["content"] is None
    assert page.json()["records"][0]["erased_from_reviewed_store"]


async def test_reviewed_expiry_excludes_text_without_sweeper_then_proves_removal(
    client,
):
    owner, project, record = await bundle(client), uuid4(), uuid4()
    request = body(owner)
    request["expires_at"] = (
        datetime.now(timezone.utc) + timedelta(seconds=1.5)
    ).isoformat()
    assert (
        await client.post(record_url(owner, project, record), json=request)
    ).status_code == 201
    expire = mutation(owner)
    endpoint = record_url(owner, project, record, "expire")
    assert (await client.post(endpoint, json=expire)).status_code == 409
    await asyncio.sleep(1.6)
    page = await client.post(base(owner, project) + "/inspect", json=audience(owner))
    value = page.json()["records"][0]
    assert value["status"] == "expired" and value["content"] is None
    assert not value["erased_from_reviewed_store"] and await stored(owner) == (
        1,
        1,
        0,
        1,
    )
    recall = await client.post(
        base(owner, project) + "/recall",
        json={**audience(owner), "query": "deployment"},
    )
    assert recall.json() == {"records": [], "truncated": False}
    removed = await client.post(endpoint, json=expire)
    assert removed.status_code == 200 and removed.json()["erased_from_reviewed_store"]
    assert await stored(owner) == (1, 0, 1, 2)
    assert (await client.post(record_url(owner, project, record), json=request)).json()[
        "status"
    ] == "expired"
    assert await stored(owner) == (1, 0, 1, 2)


async def test_reviewed_receipt_failure_rolls_back_actual_text_and_record(client):
    owner, project, record = await bundle(client), uuid4(), uuid4()
    name = "ortak_test_" + uuid4().hex
    async with engine.begin() as db:
        await db.execute(
            text(
                f"CREATE FUNCTION {name}() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'injected reviewed receipt failure'; END $$"
            )
        )
        await db.execute(
            text(
                f"CREATE TRIGGER {name} BEFORE INSERT ON ortak_reviewed_operations FOR EACH ROW EXECUTE FUNCTION {name}()"
            )
        )
    request = body(owner)
    try:
        result = await client.post(record_url(owner, project, record), json=request)
        assert result.status_code == 503
        assert await stored(owner) == (0, 0, 0, 0)
    finally:
        async with engine.begin() as db:
            await db.execute(text(f"DROP TRIGGER {name} ON ortak_reviewed_operations"))
            await db.execute(text(f"DROP FUNCTION {name}()"))
    assert (
        await client.post(record_url(owner, project, record), json=request)
    ).status_code == 201
    assert await stored(owner) == (1, 1, 0, 1)


async def test_reviewed_current_binding_scope_and_text_integrity_are_not_caller_claims(
    client,
):
    owner, other, project, record = (
        await bundle(client),
        await bundle(client),
        uuid4(),
        uuid4(),
    )
    request = body(owner)
    assert (
        await client.post(record_url(owner, project, record), json=request)
    ).status_code == 201
    for wrong in (
        {**audience(owner), "company_id": str(uuid4())},
        {**audience(owner), "employee_id": other["employee_id"]},
    ):
        assert (
            await client.post(base(owner, project) + "/inspect", json=wrong)
        ).status_code == 409
    for destination in (base(other, project), base(owner, uuid4())):
        target_audience = (
            audience(other)
            if destination.startswith(base(other, project))
            else audience(owner)
        )
        assert (
            await client.post(destination + "/inspect", json=target_audience)
        ).json()["records"] == []
    for invalid in (
        {**request, "scope": "employee_experience"},
        {**request, "content_hash": "c" * 64},
    ):
        assert (
            await client.post(record_url(owner, project, uuid4()), json=invalid)
        ).status_code == 422
    with pytest.raises(DBAPIError):
        async with engine.begin() as db:
            await db.execute(
                update(contents)
                .where(contents.c.workspace_id == owner["workspace_id"])
                .values(content="tampered deployment fact")
            )
    # Explicit disposable-DB corruption checks the independent HTTP hash guard.
    async with engine.begin() as db:
        await db.execute(
            text(
                "ALTER TABLE ortak_reviewed_record_content DISABLE TRIGGER ortak_reviewed_content_guard"
            )
        )
        await db.execute(
            update(contents)
            .where(contents.c.workspace_id == owner["workspace_id"])
            .values(content="tampered deployment fact")
        )
        await db.execute(
            text(
                "ALTER TABLE ortak_reviewed_record_content ENABLE TRIGGER ortak_reviewed_content_guard"
            )
        )
    assert (
        await client.post(
            base(owner, project) + "/recall",
            json={**audience(owner), "query": "deployment"},
        )
    ).status_code == 409
    # Erasure removes the exact scoped store even when its text was tampered.
    assert (
        await client.post(
            record_url(owner, project, record, "withdraw"), json=mutation(owner)
        )
    ).json()["erased_from_reviewed_store"]


async def test_reviewed_inspection_keyset_and_recall_have_separate_finite_budgets(
    client,
):
    owner, project = await bundle(client), uuid4()
    for _ in range(26):
        response = await client.post(
            record_url(owner, project, uuid4()), json=body(owner, "deployment " * 150)
        )
        assert response.status_code == 201
    first = await client.post(base(owner, project) + "/inspect", json=audience(owner))
    page = first.json()
    assert len(page["records"]) == 25 and page["next_after"]
    second = await client.post(
        base(owner, project) + "/inspect",
        json={**audience(owner), "after": page["next_after"]},
    )
    assert len(second.json()["records"]) == 1 and second.json()["next_after"] is None
    assert not (
        {record["record_id"] for record in page["records"]}
        & {record["record_id"] for record in second.json()["records"]}
    )
    assert (
        await client.post(
            base(owner, project) + "/inspect", json={**audience(owner), "limit": 26}
        )
    ).status_code == 422
    recalled = (
        await client.post(
            base(owner, project) + "/recall",
            json={**audience(owner), "query": "deployment"},
        )
    ).json()
    assert len(recalled["records"]) == 4 and recalled["truncated"]
    assert sum(len(value["content"].encode()) for value in recalled["records"]) == 6600
    assert await counts(owner) == (0, 0, 0, 0)


async def test_reviewed_failed_erasure_receipt_preserves_text_for_durable_retry(client):
    owner, project, record = await bundle(client), uuid4(), uuid4()
    assert (
        await client.post(record_url(owner, project, record), json=body(owner))
    ).status_code == 201
    name = "ortak_test_" + uuid4().hex
    async with engine.begin() as db:
        await db.execute(
            text(
                f"CREATE FUNCTION {name}() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN IF NEW.action='withdraw' THEN RAISE EXCEPTION 'injected withdrawal receipt failure'; END IF; RETURN NEW; END $$"
            )
        )
        await db.execute(
            text(
                f"CREATE TRIGGER {name} BEFORE INSERT ON ortak_reviewed_operations FOR EACH ROW EXECUTE FUNCTION {name}()"
            )
        )
    stop = mutation(owner)
    try:
        failed = await client.post(
            record_url(owner, project, record, "withdraw"), json=stop
        )
        assert failed.status_code == 503
        assert await stored(owner) == (1, 1, 0, 1)
    finally:
        async with engine.begin() as db:
            await db.execute(text(f"DROP TRIGGER {name} ON ortak_reviewed_operations"))
            await db.execute(text(f"DROP FUNCTION {name}()"))
    removed = await client.post(
        record_url(owner, project, record, "withdraw"), json=stop
    )
    assert removed.status_code == 200 and removed.json()["erased_from_reviewed_store"]
    assert await stored(owner) == (1, 0, 1, 2)


async def test_reviewed_database_guards_retain_tombstones_and_refuse_orphan_receipts(
    client,
):
    owner, project, record = await bundle(client), uuid4(), uuid4()
    request = body(owner)
    assert (
        await client.post(record_url(owner, project, record), json=request)
    ).status_code == 201
    params = {
        "workspace": owner["workspace_id"],
        "project": str(project),
        "record": str(record),
    }
    statements = [
        "DELETE FROM ortak_reviewed_record_content WHERE workspace_id=:workspace AND project_id=:project AND record_id=:record",
        "DELETE FROM ortak_reviewed_records WHERE workspace_id=:workspace AND project_id=:project AND record_id=:record",
        "UPDATE ortak_reviewed_records SET expires_at=expires_at+interval '1 day' WHERE workspace_id=:workspace AND project_id=:project AND record_id=:record",
        "DELETE FROM ortak_reviewed_operations WHERE workspace_id=:workspace AND project_id=:project AND record_id=:record",
        "INSERT INTO ortak_reviewed_operations(workspace_id,project_id,record_id,action,idempotency_key,request_hash) VALUES(:workspace,:project,:record,'withdraw','orphan',repeat('a',64))",
    ]
    for statement in statements:
        with pytest.raises(DBAPIError):
            async with engine.begin() as db:
                await db.execute(text(statement), params)
    assert await stored(owner) == (1, 1, 0, 1)
    assert (
        await client.post(
            record_url(owner, project, record, "withdraw"), json=mutation(owner)
        )
    ).status_code == 200
    for statement in (
        "DELETE FROM ortak_reviewed_tombstones WHERE workspace_id=:workspace AND project_id=:project AND record_id=:record",
        "INSERT INTO ortak_reviewed_record_content(workspace_id,project_id,record_id,content) VALUES(:workspace,:project,:record,'resurrection')",
        "TRUNCATE ortak_reviewed_record_content",
    ):
        with pytest.raises(DBAPIError):
            async with engine.begin() as db:
                await db.execute(text(statement), params)
    assert await stored(owner) == (1, 0, 1, 2)
