"""Bounded database operations and canonical receipt hashing."""

import hashlib
import json

from fastapi import HTTPException
from sqlalchemy import text


def canonical(value):
    return json.dumps(
        value,
        sort_keys=True,
        separators=(",", ":"),
        ensure_ascii=False,
        allow_nan=False,
    )


def request_hash(value):
    return hashlib.sha256(canonical(value).encode()).hexdigest()


async def bounds(db):
    await db.execute(text("SET LOCAL lock_timeout = '1s'"))
    await db.execute(text("SET LOCAL statement_timeout = '5s'"))
    await db.execute(text("SET LOCAL idle_in_transaction_session_timeout = '10s'"))


async def lock(db, value):
    # Domain-separated single-bigint keys; collisions only serialize extra work.
    number = int.from_bytes(
        hashlib.sha256(value.encode()).digest()[:8], "big", signed=True
    )
    await db.execute(text("SELECT pg_advisory_xact_lock(:key)"), {"key": number})


def conflict(code):
    raise HTTPException(409, detail=code)


def replay(row, fingerprint):
    if row["request_hash"] != fingerprint:
        conflict("idempotency_payload_conflict")
    return row["response"]
