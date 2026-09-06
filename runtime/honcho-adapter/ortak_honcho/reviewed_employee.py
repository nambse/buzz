"""Human-reviewed publication/cleanup; selected reads imply no runtime grant."""

from datetime import timedelta

from sqlalchemy import delete, func, insert, select

from .database import conflict
from .reviewed_employee_models import contents, operations, records, tombstones
from .reviewed_employee_provenance import digest, timestamp, utc, validate
from .reviewed_employee_store import PROTOCOL, body_hash, commitment, finish, matching, pins, prepare, quota, row, scope, target


def record_pins(identity, body):
    return {**pins(identity), **{key: getattr(body, key) for key in (
        "target_id", "destination_channel_id", "content_hash", "source_hash", "sharing_hash"
    )}}


async def pair(db, workspace, employee, record, expected):
    header = await row(db, records, workspace, employee, record)
    dead = await row(db, tombstones, workspace, employee, record)
    matching(header, expected)
    matching(dead, expected)
    return header, dead


async def operation(db, workspace, employee, record, body, action, request, fingerprint):
    found = (await db.execute(select(operations).where(
        *scope(operations, workspace, employee), operations.c.idempotency_key == body.idempotency_key
    ))).mappings().first()
    if found:
        matching(found, {"record_id": record, "action": action, "request_hash": request, "body_hash": fingerprint})
        return True
    if await db.scalar(select(operations.c.idempotency_key).where(
        *target(operations, workspace, employee, record), operations.c.action == action
    )) is not None:
        conflict("employee_operation_identity_changed")
    return False


async def project(db, workspace, employee, record, identity, now, include_text=False):
    header, dead = await pair(db, workspace, employee, record, pins(identity))
    if header is None and dead is None:
        conflict("employee_record_missing")
    content = await db.scalar(select(contents.c.content).where(*target(contents, workspace, employee, record)))
    if dead is not None and content is not None:
        conflict("employee_erasure_not_proven")
    if header is not None and content is not None and digest(content) != header["content_hash"]:
        conflict("employee_content_hash_mismatch")
    status = "withdrawn" if dead is not None else "expired" if header["expires_at"] <= now else "active"
    if status == "active" and content is None:
        conflict("employee_content_missing")
    kept = header if header is not None else dead
    return {"protocol": PROTOCOL, "company_id": identity["company_id"], "employee_id": employee,
        "deployment_id": identity["deployment_id"], "workspace_id": workspace, "record_id": record,
        **{key: kept[key] for key in ("target_id", "destination_channel_id", "namespace_hash", "binding_hash",
                                     "content_hash", "source_hash", "sharing_hash")},
        "status": status, "content": content if include_text and status == "active" else None,
        "provenance": header["provenance"] if header is not None else None,
        "expires_at": utc(header["expires_at"]) if header is not None else None,
        "erased_from_reviewed_store": dead is not None and content is None,
        "tombstone_at": utc(dead["created_at"]) if dead is not None else None}


async def mutate(db, workspace, employee, record, body, action):
    identity = await prepare(db, workspace, employee, body)
    expected = record_pins(identity, body)
    request = commitment(identity, body, record, action)
    fingerprint = body_hash(body, record, action)
    replayed = await operation(db, workspace, employee, record, body, action, request, fingerprint)
    header, dead = await pair(db, workspace, employee, record, expected)
    if not replayed:
        await quota(db, records, tombstones, workspace, employee, record, 1024)
        key = {"workspace_id": workspace, "employee_id": employee, "record_id": record}
        if action == "publish":
            if header is not None:
                conflict("employee_publish_receipt_missing")
            provenance = validate(body)
            expires = timestamp(provenance["approval"]["expires_at"])
            now = await db.scalar(select(func.clock_timestamp()))
            if dead is None and (expires <= now or expires > now + timedelta(days=90)):
                conflict("employee_expiry_out_of_range")
            await db.execute(insert(records).values(**key, **expected,
                provenance=body.provenance, kind=provenance["audience"]["kind"],
                human_public_key=provenance["audience"]["human_public_key"], expires_at=expires,
                publish_key=body.idempotency_key, request_hash=request, body_hash=fingerprint))
            if dead is None:
                await db.execute(insert(contents).values(**key, content=body.content))
        else:
            if dead is not None:
                conflict("employee_withdraw_receipt_missing")
            await db.execute(insert(tombstones).values(**key, **expected,
                withdraw_key=body.idempotency_key, request_hash=request, body_hash=fingerprint))
            await db.execute(delete(contents).where(*target(contents, workspace, employee, record)))
        await db.execute(insert(operations).values(**key, idempotency_key=body.idempotency_key,
            action=action, request_hash=request, body_hash=fingerprint))
    elif (action == "publish" and header is None) or (action == "withdraw" and dead is None):
        conflict("employee_operation_evidence_missing")
    now = await db.scalar(select(func.clock_timestamp()))
    result = await project(db, workspace, employee, record, identity, now)
    if action == "withdraw" and not result["erased_from_reviewed_store"]:
        conflict("employee_erasure_not_proven")
    result = await finish(db, {**result, "request_hash": request})
    return result, not replayed


async def recall_selected(db, workspace, employee, body):
    identity = await prepare(db, workspace, employee, body)
    result, total, truncated = [], 0, False
    # At most eight exact IDs. Filtering uses current DB time on each projection;
    # submitted ID order is retained, with no broad scan or ranking fallback.
    for record in body.record_ids:
        header, dead = await pair(db, workspace, employee, record, pins(identity))
        now = await db.scalar(select(func.clock_timestamp()))
        if (header is None or dead is not None or header["expires_at"] <= now
            or header["destination_channel_id"] != body.destination_channel_id
            or header["kind"] == "relationship" and header["human_public_key"] != body.human_public_key):
            continue
        value = await project(db, workspace, employee, record, identity, now, include_text=True)
        size = len(value["content"].encode())
        if total + size > 8192:
            truncated = True
            break
        result.append(value)
        total += size
    return await finish(db, {"records": result, "truncated": truncated})
