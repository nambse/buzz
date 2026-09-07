"""Native scoped search, followed by canonical receipt and live identity checks."""

from sqlalchemy import select
from src import models as native
from src.utils.search import search

from . import PROTOCOL
from .database import bounds, conflict
from .models import write_receipts
from .resources import owned_bundle, owned_session


async def recall(db, workspace_id, session_id, body):
    context = {
        "protocol": PROTOCOL,
        "company_id": str(body.company_id),
        "employee_id": body.employee_id,
        "scope": body.scope.model_dump(mode="json", exclude_none=True),
    }
    await bounds(db)
    bundle, _ = await owned_bundle(
        db, workspace_id, str(body.company_id), body.employee_id
    )
    session = await owned_session(
        db, workspace_id, session_id, context, bundle, create=False
    )
    await db.commit()
    if session is None:
        return {"records": [], "truncated": False}
    # Embedding provider work happens without a held DB transaction/row lock.
    found = await search(
        body.query,
        filters={"workspace_id": workspace_id, "session_id": session_id},
        limit=body.max_records,
    )
    # Native sessions use expire_on_commit=False. Discard pre-provider ORM
    # identities so fresh SELECTs cannot silently reuse their old attributes.
    db.expunge_all()
    await bounds(db)
    bundle, _ = await owned_bundle(
        db, workspace_id, str(body.company_id), body.employee_id
    )
    if (
        await owned_session(db, workspace_id, session_id, context, bundle, create=False)
        is None
    ):
        conflict("session_removed_during_recall")
    ids = [message.public_id for message in found]
    current = (
        await db.scalars(
            select(native.Message)
            .where(
                native.Message.workspace_name == workspace_id,
                native.Message.session_name == session_id,
                native.Message.public_id.in_(ids),
            )
            .with_for_update(read=True)
        )
    ).all()
    by_id = {message.public_id: message for message in current}
    records, total = [], 0
    truncated = len(found) == body.max_records
    for record_id in ids:
        message = by_id.get(record_id)
        if message is None:
            conflict("message_removed_during_recall")
        envelope = message.h_metadata.get("ortak", {})
        if not isinstance(envelope, dict):
            conflict("message_provenance_conflict")
        receipt = (
            (
                await db.execute(
                    select(write_receipts).where(
                        write_receipts.c.workspace_id == workspace_id,
                        write_receipts.c.session_id == session_id,
                        write_receipts.c.idempotency_key == envelope.get("write_key"),
                    )
                )
            )
            .mappings()
            .first()
        )
        record = (
            next(
                (
                    item
                    for item in receipt["response"]["records"]
                    if item["record_ref"] == record_id
                ),
                None,
            )
            if receipt
            else None
        )
        if (
            not record
            or record["content"] != message.content
            or record["metadata"] != message.h_metadata
            or record["scope"] != context["scope"]
            or envelope.get("company_id") != context["company_id"]
            or envelope.get("employee_id") != context["employee_id"]
            or message.peer_name != bundle["response"]["employee_peer"]
        ):
            conflict("message_provenance_conflict")
        size = len(record["content"].encode())
        if total + size > body.max_bytes:
            truncated = True
            break
        records.append(
            {
                key: record[key]
                for key in ("record_ref", "content", "scope", "provenance")
            }
        )
        total += size
    await db.commit()
    return {"records": records, "truncated": truncated}
