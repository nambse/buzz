"""Only explicit disposable PostgreSQL; no provider requests or existing secrets."""

# ruff: noqa: E402 -- Set isolated configuration before importing Honcho.

import os
from urllib.parse import urlparse
from uuid import uuid4

import pytest

url = os.environ.get("ORTAK_HONCHO_TEST_DATABASE_URL")
if not url:
    raise pytest.UsageError(
        "Set explicit ORTAK_HONCHO_TEST_DATABASE_URL to a disposable Honcho DB"
    )
parsed = urlparse(url)
if parsed.hostname not in {
    "127.0.0.1",
    "localhost",
    "host.docker.internal",
    "honcho-test-db",
} or not parsed.path.startswith("/ortak_honcho_"):
    raise pytest.UsageError(
        "Honcho tests require an explicit local database named ortak_honcho_*"
    )
os.environ["DB_CONNECTION_URI"] = url.replace(
    "postgres://", "postgresql+psycopg://", 1
).replace("postgresql://", "postgresql+psycopg://", 1)
os.environ["AUTH_USE_AUTH"] = "true"
os.environ["AUTH_JWT_SECRET"] = uuid4().hex + uuid4().hex
os.environ["LLM_OPENAI_API_KEY"] = "test-only-" + uuid4().hex
os.environ["CACHE_ENABLED"] = "false"
os.environ["EMBED_MESSAGES"] = "false"
os.environ["METRICS_ENABLED"] = "false"
os.environ["TELEMETRY_ENABLED"] = "false"
os.environ["SENTRY_ENABLED"] = "false"

from httpx import ASGITransport, AsyncClient
from ortak_honcho.app import app
from ortak_honcho.models import TABLES, write_receipts
from ortak_honcho.reviewed_guards import install as install_reviewed_guards
from ortak_honcho.reviewed_employee_guards import install as install_employee_reviewed_guards
from sqlalchemy import func, select
from src import models as native
from src.db import Base, SessionLocal, engine
from src.security import create_admin_jwt


@pytest.fixture
async def client():
    async with engine.begin() as connection:
        await connection.run_sync(
            lambda sync: Base.metadata.create_all(sync, tables=TABLES)
        )
        await install_reviewed_guards(connection)
        await install_employee_reviewed_guards(connection)
    async with AsyncClient(
        transport=ASGITransport(app=app, raise_app_exceptions=False),
        base_url="http://honcho.test",
        headers={"Authorization": "Bearer " + create_admin_jwt()},
    ) as value:
        yield value
    await engine.dispose()


def resource_body():
    return {
        "idempotency_key": "create_" + uuid4().hex,
        "company_id": str(uuid4()),
        "employee_id": "employee_" + uuid4().hex,
        "workspace_id": "ws_" + uuid4().hex,
        "user_peer": "operator",
        "employee_peer": "employee",
    }


async def bundle(client):
    body = resource_body()
    response = await client.post("/v3/ortak/resources/create", json=body)
    assert response.status_code == 201, response.text
    return body


def memory_body(
    owner, *, key="write_1", scope=None, content="Ortak scoped durable memory fact"
):
    return {
        "idempotency_key": key,
        "company_id": owner["company_id"],
        "employee_id": owner["employee_id"],
        "scope": scope or {"scope": "employee_experience"},
        "facts": [
            {
                "content": content,
                "provenance": {
                    "employee_id": owner["employee_id"],
                    "source": "test_fixture",
                    "recorded_at": "2026-09-05T00:00:00Z",
                },
            }
        ],
    }


def endpoint(owner, operation="remember", session="experience"):
    return (
        f"/v3/ortak/workspaces/{owner['workspace_id']}/sessions/{session}/{operation}"
    )


async def counts(owner):
    async with SessionLocal() as db:
        result = []
        for table in (
            native.Message.__table__,
            native.QueueItem.__table__,
            write_receipts,
            native.MessageEmbedding.__table__,
        ):
            column = (
                table.c.workspace_id
                if "workspace_id" in table.c
                else table.c.workspace_name
            )
            result.append(
                await db.scalar(
                    select(func.count())
                    .select_from(table)
                    .where(column == owner["workspace_id"])
                )
            )
        return tuple(result)
