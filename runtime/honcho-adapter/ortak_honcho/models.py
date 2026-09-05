"""Receipts intentionally outlive native resource deletion; no cascading FKs."""

from sqlalchemy import Column, DateTime, String, Table, func
from sqlalchemy.dialects.postgresql import JSONB
from src.db import Base

resource_receipts = Table(
    "ortak_resource_receipts",
    Base.metadata,
    Column("company_id", String(36), primary_key=True),
    Column("employee_id", String(64), primary_key=True),
    Column("idempotency_key", String(200), primary_key=True),
    Column("workspace_id", String(128), nullable=False, unique=True),
    Column("request_hash", String(64), nullable=False),
    Column("response", JSONB, nullable=False),
    Column("native_ids", JSONB, nullable=False),
    Column(
        "created_at", DateTime(timezone=True), nullable=False, server_default=func.now()
    ),
)

session_ownership = Table(
    "ortak_session_ownership",
    Base.metadata,
    Column("workspace_id", String(128), primary_key=True),
    Column("session_id", String(128), primary_key=True),
    Column("native_id", String(21), nullable=False, unique=True),
    Column("context", JSONB, nullable=False),
)

write_receipts = Table(
    "ortak_write_receipts",
    Base.metadata,
    Column("workspace_id", String(128), primary_key=True),
    Column("session_id", String(128), primary_key=True),
    Column("idempotency_key", String(200), primary_key=True),
    Column("request_hash", String(64), nullable=False),
    Column("response", JSONB, nullable=False),
    Column(
        "created_at", DateTime(timezone=True), nullable=False, server_default=func.now()
    ),
)

TABLES = [resource_receipts, session_ownership, write_receipts]
