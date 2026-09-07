"""Real deferred guards and injected failures: no partial publication or cleanup."""

from contextlib import asynccontextmanager
from uuid import uuid4

import pytest
from sqlalchemy import delete, insert, select, text, update
from sqlalchemy.exc import DBAPIError
from src.db import engine
from ortak_honcho.reviewed_employee_models import (
    TABLES, contents, diagnostic_content, diagnostics, diagnostic_tombstones, operations, records, tombstones,
)

from .employee_reviewed_fixture import (
    canonical, diagnostic, diagnostic_read, publish, remote_hash, selected, sha, stored, url, withdraw,
)
from .test_employee_reviewed_diagnostics import commitment, path


@asynccontextmanager
async def fault(table, timing, condition="true"):
    """Only this disposable DB; existing production guards remain enabled."""
    name = "employee_fault_" + uuid4().hex
    async with engine.begin() as db:
        await db.execute(text(f"CREATE FUNCTION {name}() RETURNS trigger LANGUAGE plpgsql AS $$ "
            f"BEGIN IF {condition} THEN RAISE EXCEPTION 'injected employee failure'; END IF; "
            "IF TG_OP='DELETE' THEN RETURN OLD; END IF; RETURN NEW; END $$"))
        await db.execute(text(f"CREATE TRIGGER {name} {timing} ON {table.name} FOR EACH ROW EXECUTE FUNCTION {name}()"))
    try:
        yield
    finally:
        async with engine.begin() as db:
            await db.execute(text(f"DROP TRIGGER {name} ON {table.name}"))
            await db.execute(text(f"DROP FUNCTION {name}()"))


async def test_employee_actual_receipt_and_erasure_faults_preserve_same_key_retry(client):
    owner, common = await selected(client)
    record = str(uuid4())
    request = publish(common, record)
    async with fault(operations, "BEFORE INSERT", "NEW.action='publish'"):
        failed = await client.post(url(owner, record), json=request)
        assert failed.status_code == 503, failed.text
        assert await stored(owner) == (0, 0, 0, 0, 0, 0, 0)
    assert (await client.post(url(owner, record), json=request)).status_code == 201
    stop = withdraw(request, record)
    async with fault(operations, "BEFORE INSERT", "NEW.action='withdraw'"):
        failed = await client.post(url(owner, record, "withdraw"), json=stop)
        assert failed.status_code == 503, failed.text
        assert await stored(owner) == (1, 1, 0, 1, 0, 0, 0)
    success = await client.post(url(owner, record, "withdraw"), json=stop)
    assert success.status_code == 200 and success.json()["erased_from_reviewed_store"]
    assert await stored(owner) == (1, 0, 1, 2, 0, 0, 0)


async def test_employee_diagnostic_failed_write_and_cleanup_do_not_issue_false_erasure(client):
    owner, common = await selected(client)
    operation, request = str(uuid4()), diagnostic(common)
    read = diagnostic_read(request)
    async with fault(diagnostic_content, "BEFORE INSERT"):
        failed = await client.post(path(owner, operation, "write"), json=request)
        assert failed.status_code == 503, failed.text
        assert await stored(owner) == (0, 0, 0, 0, 0, 0, 0)
    assert (await client.post(path(owner, operation, "write"), json=request)).status_code == 201
    async with fault(diagnostic_content, "BEFORE DELETE"):
        failed = await client.post(path(owner, operation, "withdraw"), json=read)
        assert failed.status_code == 503, failed.text
        assert await stored(owner) == (0, 0, 0, 0, 1, 1, 0)
        current = await client.post(path(owner, operation, "read"), json=read)
        assert current.json()["challenge"] == request["challenge"] and not current.json()["erased"]
    success = await client.post(path(owner, operation, "withdraw"), json=read)
    assert success.status_code == 200 and success.json()["erased"]


async def test_employee_db_retention_and_atomic_header_content_and_tombstone_guards(client):
    owner, common = await selected(client)
    record, operation = str(uuid4()), str(uuid4())
    request, probe = publish(common, record), diagnostic(common)
    assert (await client.post(url(owner, record), json=request)).status_code == 201
    assert (await client.post(path(owner, operation, "write"), json=probe)).status_code == 201
    for table in (records, operations, diagnostics):
        for statement in (delete(table), update(table).values(employee_id="changed-employee")):
            with pytest.raises(DBAPIError):
                async with engine.begin() as db:
                    await db.execute(statement.where(table.c.workspace_id == owner["workspace_id"]))
    for table in (contents, diagnostic_content):
        with pytest.raises(DBAPIError):
            async with engine.begin() as db:
                await db.execute(delete(table).where(table.c.workspace_id == owner["workspace_id"]))
    with pytest.raises(DBAPIError):
        async with engine.begin() as db:
            await db.execute(insert(operations).values(workspace_id=owner["workspace_id"],
                employee_id=owner["employee_id"], record_id=record, action="withdraw",
                idempotency_key="orphan", request_hash="a" * 64, body_hash="b" * 64))
    # Exact header copies without a matching operation/challenge must fail at
    # COMMIT, not merely a not-null or unique constraint during INSERT.
    for table, key in ((records, "record_id"), (diagnostics, "operation_id")):
        async with engine.connect() as db:
            original = dict((await db.execute(select(table).where(
                table.c.workspace_id == owner["workspace_id"]))).mappings().one())
        with pytest.raises(DBAPIError):
            async with engine.begin() as db:
                await db.execute(insert(table).values(**{**original, key: str(uuid4())}))
    assert (await client.post(url(owner, record, "withdraw"), json=withdraw(request, record))).status_code == 200
    assert (await client.post(path(owner, operation, "withdraw"), json=diagnostic_read(probe))).status_code == 200
    for table in (tombstones, diagnostic_tombstones):
        with pytest.raises(DBAPIError):
            async with engine.begin() as db:
                await db.execute(delete(table).where(table.c.workspace_id == owner["workspace_id"]))
    for table, payload in (
        (contents, {"record_id": record, "content": request["content"]}),
        (diagnostic_content, {"operation_id": operation, "challenge": probe["challenge"]}),
    ):
        with pytest.raises(DBAPIError):
            async with engine.begin() as db:
                await db.execute(insert(table).values(workspace_id=owner["workspace_id"], employee_id=owner["employee_id"], **payload))
    for table in TABLES:
        with pytest.raises(DBAPIError):
            async with engine.begin() as db:
                await db.execute(text(f"TRUNCATE {table.name} CASCADE"))
    assert await stored(owner) == (1, 0, 1, 2, 1, 0, 1)


async def test_employee_scope_quotas_count_cleanup_only_history_without_disabling_guards(client):
    owner, common = await selected(client)
    record, operation = str(uuid4()), str(uuid4())
    request, probe = publish(common, record), diagnostic(common)
    stop, read = withdraw(request, record), diagnostic_read(probe)
    assert (await client.post(url(owner, record, "withdraw"), json=stop)).status_code == 200
    assert (await client.post(path(owner, operation, "withdraw"), json=read)).status_code == 200
    async with engine.connect() as db:
        frozen_record = dict((await db.execute(select(tombstones).where(
            tombstones.c.workspace_id == owner["workspace_id"]))).mappings().one())
        frozen_diagnostic = dict((await db.execute(select(diagnostic_tombstones).where(
            diagnostic_tombstones.c.workspace_id == owner["workspace_id"]))).mappings().one())
    # Bounded synthetic quota seed, with the real immutable/deferred guards on.
    # Every new record has an exact same-transaction operation receipt; no HTTP
    # ownership or authorization proof is inferred from this SQL fixture.
    headers, receipts = [], []
    for _ in range(1023):
        identifier = str(uuid4())
        body = withdraw(request, identifier)
        typed = remote_hash(common, body, identifier, "withdraw")
        fingerprint = sha(canonical({"protocol": "reviewed-employee/1", "identifier": identifier,
                                      "action": "withdraw", "body": body}))
        headers.append({**frozen_record, "record_id": identifier, "withdraw_key": body["idempotency_key"],
                        "request_hash": typed, "body_hash": fingerprint})
        receipts.append({"workspace_id": owner["workspace_id"], "employee_id": owner["employee_id"],
            "record_id": identifier, "idempotency_key": body["idempotency_key"], "action": "withdraw",
            "request_hash": typed, "body_hash": fingerprint})
    probes = []
    for _ in range(127):
        identifier = str(uuid4())
        probes.append({**frozen_diagnostic, "operation_id": identifier,
            "withdraw_request_hash": commitment(common, read, identifier, withdraw=True),
            "body_hash": sha(canonical({"protocol": "reviewed-employee/1", "identifier": identifier,
                                         "action": "withdraw", "body": read}))})
    async with engine.begin() as db:
        await db.execute(text("SET LOCAL statement_timeout='15s'"))
        await db.execute(insert(tombstones), headers)
        await db.execute(insert(operations), receipts)
        await db.execute(insert(diagnostic_tombstones), probes)
    assert await stored(owner) == (0, 0, 1024, 1024, 0, 0, 128)
    new_record, new_operation = str(uuid4()), str(uuid4())
    overflow = await client.post(url(owner, new_record, "withdraw"), json=withdraw(request, new_record))
    assert overflow.status_code == 409 and overflow.json()["detail"] == "employee_scope_limit"
    overflow_probe = await client.post(path(owner, new_operation, "write"), json=probe)
    assert overflow_probe.status_code == 409 and overflow_probe.json()["detail"] == "employee_scope_limit"
    # Already admitted identities remain recoverable at the full retained cap.
    assert (await client.post(url(owner, record), json=request)).status_code == 201
    assert (await client.post(path(owner, operation, "write"), json=probe)).status_code == 201
    assert await stored(owner) == (1, 0, 1024, 1025, 1, 0, 128)
