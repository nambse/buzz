"""Shared exact ownership and finite reviewed-record storage operations."""

from sqlalchemy import func, insert, select, union

from . import PROTOCOL
from .database import bounds, conflict, lock, request_hash
from .resources import owned_bundle
from .reviewed_models import contents, operations, records, tombstones

FAMILY = "reviewed-project/1"


def scope(table, workspace, project):
    return (table.c.workspace_id == workspace, table.c.project_id == str(project))


def target(table, workspace, project, record):
    return (*scope(table, workspace, project), table.c.record_id == str(record))


async def prepare(db, workspace, project, body):
    await bounds(db)
    receipt, _ = await owned_bundle(
        db, workspace, str(body.company_id), body.employee_id
    )
    # One finite per-project transaction; no provider work crosses this lock.
    await lock(db, f"ortak-reviewed:{workspace}:{project}")
    binding_hash = request_hash(
        {"request_hash": receipt["request_hash"], "native_ids": receipt["native_ids"]}
    )
    now = await db.scalar(select(func.clock_timestamp()))
    return binding_hash, now


def validate_owner(row, body, binding_hash):
    if row and (
        row["company_id"] != str(body.company_id)
        or row["employee_id"] != body.employee_id
        or row["binding_hash"] != binding_hash
    ):
        conflict("reviewed_record_ownership_changed")


async def pair(db, workspace, project, record, body, binding_hash):
    header = (
        (
            await db.execute(
                select(records).where(*target(records, workspace, project, record))
            )
        )
        .mappings()
        .first()
    )
    dead = (
        (
            await db.execute(
                select(tombstones).where(
                    *target(tombstones, workspace, project, record)
                )
            )
        )
        .mappings()
        .first()
    )
    validate_owner(header, body, binding_hash)
    validate_owner(dead, body, binding_hash)
    return header, dead


async def quota(db, workspace, project, record):
    ids = union(
        select(records.c.record_id).where(*scope(records, workspace, project)),
        select(tombstones.c.record_id).where(*scope(tombstones, workspace, project)),
    ).subquery()
    exists = await db.scalar(
        select(ids.c.record_id).where(ids.c.record_id == str(record))
    )
    if (
        exists is None
        and await db.scalar(select(func.count()).select_from(ids)) >= 1024
    ):
        conflict("reviewed_scope_record_limit")


async def operation(db, workspace, project, record, body, action):
    fingerprint = request_hash(
        {
            "family": FAMILY,
            "workspace_id": workspace,
            "project_id": str(project),
            "record_id": str(record),
            "action": action,
            **body.model_dump(mode="json", exclude_none=True),
        }
    )
    existing = (
        (
            await db.execute(
                select(operations).where(
                    *scope(operations, workspace, project),
                    operations.c.idempotency_key == body.idempotency_key,
                )
            )
        )
        .mappings()
        .first()
    )
    if existing:
        if (
            existing["request_hash"] != fingerprint
            or existing["record_id"] != str(record)
            or existing["action"] != action
        ):
            conflict("idempotency_payload_conflict")
        return fingerprint, True
    other = await db.scalar(
        select(operations.c.idempotency_key).where(
            *target(operations, workspace, project, record),
            operations.c.action == action,
        )
    )
    if other is not None:
        conflict("reviewed_operation_identity_changed")
    return fingerprint, False


async def record_operation(db, workspace, project, record, body, action, fingerprint):
    await db.execute(
        insert(operations).values(
            workspace_id=workspace,
            project_id=str(project),
            record_id=str(record),
            action=action,
            idempotency_key=body.idempotency_key,
            request_hash=fingerprint,
        )
    )


async def project_record(
    db, workspace, project, record, body, binding_hash, now, *, include_text
):
    header, dead = await pair(db, workspace, project, record, body, binding_hash)
    if not header and not dead:
        conflict("reviewed_record_not_found")
    content = await db.scalar(
        select(contents.c.content).where(*target(contents, workspace, project, record))
    )
    status = (
        dead["reason"]
        if dead
        else ("expired" if header["expires_at"] <= now else "active")
    )
    if dead and content is not None:
        conflict("reviewed_tombstone_content_conflict")
    if content is not None and request_hash_text(content) != header["content_hash"]:
        conflict("reviewed_content_hash_conflict")
    if status == "active" and content is None:
        conflict("reviewed_content_missing")
    return {
        "protocol": PROTOCOL,
        "record_family": FAMILY,
        "workspace_id": workspace,
        "project_id": str(project),
        "record_id": str(record),
        "company_id": str(body.company_id),
        "employee_id": body.employee_id,
        "binding_hash": binding_hash,
        "status": status,
        "content": content if include_text and status == "active" else None,
        "content_hash": header["content_hash"] if header else None,
        "expires_at": header["expires_at"] if header else None,
        "provenance": {
            key: header[key]
            for key in ("approval_id", "approved_by", "source_hash", "created_at")
        }
        if header
        else None,
        "erased_from_reviewed_store": bool(dead and content is None),
        "tombstone_at": dead["created_at"] if dead else None,
    }


def request_hash_text(content):
    import hashlib

    return hashlib.sha256(content.encode()).hexdigest()
