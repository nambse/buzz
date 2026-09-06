"""Reviewed metadata/tombstones retain hashes; only the content table holds text."""

from sqlalchemy import (
    CheckConstraint,
    Column,
    DateTime,
    ForeignKeyConstraint,
    String,
    Table,
    Text,
    UniqueConstraint,
    func,
)
from src.db import Base


def scope_columns():
    return [
        Column("workspace_id", String(128), primary_key=True),
        Column("project_id", String(36), primary_key=True),
        Column("record_id", String(36), primary_key=True),
    ]


records = Table(
    "ortak_reviewed_records",
    Base.metadata,
    *scope_columns(),
    Column("company_id", String(36), nullable=False),
    Column("employee_id", String(64), nullable=False),
    Column("binding_hash", String(64), nullable=False),
    Column("content_hash", String(64), nullable=False),
    Column("source_hash", String(64), nullable=False),
    Column("approval_id", String(36), nullable=False),
    Column("approved_by", String(64), nullable=False),
    Column("expires_at", DateTime(timezone=True), nullable=False),
    Column(
        "created_at", DateTime(timezone=True), nullable=False, server_default=func.now()
    ),
    Column("publish_key", String(200), nullable=False),
    Column("request_hash", String(64), nullable=False),
    CheckConstraint(
        "content_hash ~ '^[0-9a-f]{64}$' AND source_hash ~ '^[0-9a-f]{64}$' "
        "AND binding_hash ~ '^[0-9a-f]{64}$' AND request_hash ~ '^[0-9a-f]{64}$' "
        "AND approved_by ~ '^[0-9a-f]{64}$'",
        name="ortak_reviewed_hashes",
    ),
)

contents = Table(
    "ortak_reviewed_record_content",
    Base.metadata,
    *scope_columns(),
    Column("content", Text, nullable=False),
    ForeignKeyConstraint(
        ["workspace_id", "project_id", "record_id"],
        [
            "ortak_reviewed_records.workspace_id",
            "ortak_reviewed_records.project_id",
            "ortak_reviewed_records.record_id",
        ],
    ),
    CheckConstraint(
        "octet_length(content) BETWEEN 1 AND 4096 AND btrim(content) <> ''",
        name="ortak_reviewed_text_bound",
    ),
)

tombstones = Table(
    "ortak_reviewed_tombstones",
    Base.metadata,
    *scope_columns(),
    Column("company_id", String(36), nullable=False),
    Column("employee_id", String(64), nullable=False),
    Column("binding_hash", String(64), nullable=False),
    Column("reason", String(16), nullable=False),
    Column(
        "created_at", DateTime(timezone=True), nullable=False, server_default=func.now()
    ),
    CheckConstraint(
        "reason IN ('withdrawn','expired')", name="ortak_reviewed_tombstone_reason"
    ),
)

operations = Table(
    "ortak_reviewed_operations",
    Base.metadata,
    Column("workspace_id", String(128), primary_key=True),
    Column("project_id", String(36), primary_key=True),
    Column("idempotency_key", String(200), primary_key=True),
    Column("record_id", String(36), nullable=False),
    Column("action", String(16), nullable=False),
    Column("request_hash", String(64), nullable=False),
    Column(
        "created_at", DateTime(timezone=True), nullable=False, server_default=func.now()
    ),
    CheckConstraint(
        "action IN ('publish','withdraw','expire')",
        name="ortak_reviewed_operation_action",
    ),
    CheckConstraint(
        "request_hash ~ '^[0-9a-f]{64}$'", name="ortak_reviewed_operation_hash"
    ),
    UniqueConstraint(
        "workspace_id",
        "project_id",
        "record_id",
        "action",
        name="ortak_reviewed_one_operation_per_action",
    ),
)

TABLES = [records, contents, tombstones, operations]
