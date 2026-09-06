"""One transaction per explicit publish, withdrawal, expiry or bounded read."""

from datetime import timedelta

from sqlalchemy import delete, insert, select, union, func

from .database import conflict
from .reviewed_models import contents, records, tombstones
from .reviewed_store import (
    operation,
    pair,
    prepare,
    project_record,
    quota,
    record_operation,
    scope,
    target,
)


async def publish(db, workspace, project, record, body):
    binding_hash, now = await prepare(db, workspace, project, body)
    fingerprint, replayed = await operation(
        db, workspace, project, record, body, "publish"
    )
    header, dead = await pair(db, workspace, project, record, body, binding_hash)
    if not replayed:
        if header:
            conflict("reviewed_publish_receipt_missing")
        await quota(db, workspace, project, record)
        if not dead and (
            body.expires_at <= now or body.expires_at > now + timedelta(days=90)
        ):
            conflict("reviewed_expiry_out_of_range")
        await db.execute(
            insert(records).values(
                workspace_id=workspace,
                project_id=str(project),
                record_id=str(record),
                company_id=str(body.company_id),
                employee_id=body.employee_id,
                binding_hash=binding_hash,
                content_hash=body.content_hash,
                source_hash=body.source_hash,
                approval_id=str(body.approval_id),
                approved_by=body.approved_by,
                expires_at=body.expires_at,
                publish_key=body.idempotency_key,
                request_hash=fingerprint,
            )
        )
        # A withdrawn-before-publish record retains its hash/receipt, never text.
        if not dead:
            await db.execute(
                insert(contents).values(
                    workspace_id=workspace,
                    project_id=str(project),
                    record_id=str(record),
                    content=body.content,
                )
            )
        await record_operation(
            db, workspace, project, record, body, "publish", fingerprint
        )
    elif not header or header["request_hash"] != fingerprint:
        conflict("reviewed_publish_receipt_conflict")
    result = await project_record(
        db, workspace, project, record, body, binding_hash, now, include_text=False
    )
    await db.commit()
    return {**result, "request_hash": fingerprint}, not replayed


async def erase(db, workspace, project, record, body, *, expired=False):
    binding_hash, now = await prepare(db, workspace, project, body)
    action = "expire" if expired else "withdraw"
    fingerprint, replayed = await operation(
        db, workspace, project, record, body, action
    )
    header, dead = await pair(db, workspace, project, record, body, binding_hash)
    if expired and (not header or header["expires_at"] > now):
        conflict("reviewed_record_not_expired")
    if not replayed:
        await quota(db, workspace, project, record)
        if not dead:
            await db.execute(
                insert(tombstones).values(
                    workspace_id=workspace,
                    project_id=str(project),
                    record_id=str(record),
                    company_id=str(body.company_id),
                    employee_id=body.employee_id,
                    binding_hash=binding_hash,
                    reason="expired" if expired else "withdrawn",
                )
            )
        await db.execute(
            delete(contents).where(*target(contents, workspace, project, record))
        )
        await record_operation(
            db, workspace, project, record, body, action, fingerprint
        )
    elif not dead:
        conflict("reviewed_erasure_receipt_conflict")
    result = await project_record(
        db, workspace, project, record, body, binding_hash, now, include_text=False
    )
    if not result["erased_from_reviewed_store"]:
        conflict("reviewed_erasure_not_proven")
    await db.commit()
    return {**result, "request_hash": fingerprint}, not replayed


async def inspect(db, workspace, project, body):
    binding_hash, now = await prepare(db, workspace, project, body)
    ids = union(
        select(records.c.record_id).where(*scope(records, workspace, project)),
        select(tombstones.c.record_id).where(*scope(tombstones, workspace, project)),
    ).subquery()
    query = select(ids.c.record_id).order_by(ids.c.record_id).limit(body.limit + 1)
    if body.after:
        query = query.where(ids.c.record_id > str(body.after))
    found = (await db.scalars(query)).all()
    result = [
        await project_record(
            db, workspace, project, record, body, binding_hash, now, include_text=True
        )
        for record in found[: body.limit]
    ]
    await db.commit()
    return {
        "records": result,
        "next_after": found[body.limit - 1] if len(found) > body.limit else None,
    }


async def recall(db, workspace, project, body):
    return await _recall(db, workspace, project, body, None)


async def recall_selected(db, workspace, project, body):
    return await _recall(db, workspace, project, body, body.record_ids)


async def _recall(db, workspace, project, body, selected):
    binding_hash, now = await prepare(db, workspace, project, body)
    join_content = records.join(
        contents,
        (records.c.workspace_id == contents.c.workspace_id)
        & (records.c.project_id == contents.c.project_id)
        & (records.c.record_id == contents.c.record_id),
    )
    joined = join_content.outerjoin(
        tombstones,
        (records.c.workspace_id == tombstones.c.workspace_id)
        & (records.c.project_id == tombstones.c.project_id)
        & (records.c.record_id == tombstones.c.record_id),
    )
    found = (
        await db.scalars(
            select(records.c.record_id)
            .select_from(joined)
            .where(
                *scope(records, workspace, project),
                records.c.company_id == str(body.company_id),
                records.c.employee_id == body.employee_id,
                records.c.binding_hash == binding_hash,
                records.c.expires_at > now,
                tombstones.c.record_id.is_(None),
                # Apply current caller selection before ranking/limit. Withheld
                # matches must not crowd permitted records out of the window.
                records.c.record_id.in_([str(record) for record in selected])
                if selected is not None
                else True,
                func.to_tsvector("simple", contents.c.content).op("@@")(
                    func.websearch_to_tsquery("simple", body.query)
                    if selected is not None
                    else func.plainto_tsquery("simple", body.query)
                ),
            )
            .order_by(records.c.record_id)
            .limit(9)
        )
    ).all()
    result, total = [], 0
    truncated = len(found) > 8
    for record in found[:8]:
        value = await project_record(
            db, workspace, project, record, body, binding_hash, now, include_text=True
        )
        size = len(value["content"].encode())
        if total + size > 8192:
            truncated = True
            break
        result.append(value)
        total += size
    await db.commit()
    return {"records": result, "truncated": truncated}
