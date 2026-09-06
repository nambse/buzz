"""Employee-only metadata; text and diagnostic challenge have separate lifetimes."""

from sqlalchemy import CheckConstraint, Column, DateTime, ForeignKeyConstraint, String, Table, Text, BigInteger, UniqueConstraint, func
from sqlalchemy.dialects.postgresql import JSONB
from src.db import Base


def scope_columns(identifier):
    return [Column("workspace_id", String(128), primary_key=True),
            Column("employee_id", String(64), primary_key=True),
            Column(identifier, String(36), primary_key=True)]


def pins():
    return [Column("company_id", String(36), nullable=False),
            Column("deployment_id", String(36), nullable=False),
            Column("namespace_hash", String(64), nullable=False),
            Column("binding_hash", String(64), nullable=False),
            Column("ownership", JSONB, nullable=False)]


def digest_column(name):
    return Column(name, String(64), nullable=False)


def created():
    return Column("created_at", DateTime(timezone=True), nullable=False, server_default=func.now())


def record_pins():
    return [*pins(), Column("target_id", String(36), nullable=False),
            Column("destination_channel_id", String(36), nullable=False),
            *[digest_column(k) for k in ("content_hash", "source_hash", "sharing_hash")]]


def digest_check(*names):
    # Native Base.metadata's convention requires explicit check names. The
    # first field distinguishes each digest group within its table.
    return CheckConstraint(" AND ".join(f"{name} ~ '^[0-9a-f]{{64}}$'" for name in names),
                           name="hex_" + names[0])


records = Table(
    "ortak_employee_reviewed_records", Base.metadata, *scope_columns("record_id"),
    *record_pins(), Column("provenance", Text, nullable=False),
    Column("kind", String(16), nullable=False), Column("human_public_key", String(64)),
    Column("expires_at", DateTime(timezone=True), nullable=False),
    Column("publish_key", String(200), nullable=False),
    digest_column("request_hash"), digest_column("body_hash"), created(),
    CheckConstraint("octet_length(provenance) BETWEEN 1 AND 4096", name="provenance_bound"),
    CheckConstraint("(kind='experience' AND human_public_key IS NULL) OR "
                    "(kind='relationship' AND human_public_key IS NOT NULL "
                    "AND human_public_key ~ '^[0-9a-f]{64}$')", name="kind_human"),
    digest_check("namespace_hash", "binding_hash", "content_hash", "source_hash", "sharing_hash", "request_hash", "body_hash"),
)
contents = Table(
    "ortak_employee_reviewed_content", Base.metadata, *scope_columns("record_id"),
    Column("content", Text, nullable=False),
    ForeignKeyConstraint(["workspace_id", "employee_id", "record_id"],
        [f"{records.name}.{key}" for key in ("workspace_id", "employee_id", "record_id")]),
    CheckConstraint("octet_length(content) BETWEEN 1 AND 4096 AND btrim(content)<>''", name="content_bound"),
)
tombstones = Table(
    "ortak_employee_reviewed_tombstones", Base.metadata, *scope_columns("record_id"),
    *record_pins(), Column("withdraw_key", String(200), nullable=False),
    digest_column("request_hash"), digest_column("body_hash"), created(),
    digest_check("namespace_hash", "binding_hash", "content_hash", "source_hash", "sharing_hash", "request_hash", "body_hash"),
)
operations = Table(
    "ortak_employee_reviewed_operations", Base.metadata,
    Column("workspace_id", String(128), primary_key=True),
    Column("employee_id", String(64), primary_key=True),
    Column("idempotency_key", String(200), primary_key=True),
    Column("record_id", String(36), nullable=False), Column("action", String(16), nullable=False),
    digest_column("request_hash"), digest_column("body_hash"), created(),
    CheckConstraint("action IN ('publish','withdraw')", name="action_kind"), digest_check("request_hash", "body_hash"),
    UniqueConstraint("workspace_id", "employee_id", "record_id", "action"),
)


def diagnostic_pins():
    return [*pins(), Column("employee_revision_id", String(36), nullable=False),
            Column("employee_lifecycle_epoch", BigInteger, nullable=False),
            digest_column("challenge_hash"), digest_column("body_hash"),
            CheckConstraint("employee_lifecycle_epoch>=0", name="lifecycle_nonnegative"),
            digest_check("namespace_hash", "binding_hash", "challenge_hash", "body_hash")]


diagnostics = Table(
    "ortak_employee_diagnostics", Base.metadata, *scope_columns("operation_id"),
    *diagnostic_pins(), digest_column("write_request_hash"), created(),
    digest_check("write_request_hash"),
)
diagnostic_content = Table(
    "ortak_employee_diagnostic_content", Base.metadata, *scope_columns("operation_id"),
    Column("challenge", String(64), nullable=False), digest_check("challenge"),
    ForeignKeyConstraint(["workspace_id", "employee_id", "operation_id"],
        [f"{diagnostics.name}.{key}" for key in ("workspace_id", "employee_id", "operation_id")]),
)
diagnostic_tombstones = Table(
    "ortak_employee_diagnostic_tombstones", Base.metadata, *scope_columns("operation_id"),
    *diagnostic_pins(), digest_column("withdraw_request_hash"), created(),
    digest_check("withdraw_request_hash"),
)

TABLES = [records, contents, tombstones, operations, diagnostics, diagnostic_content, diagnostic_tombstones]
