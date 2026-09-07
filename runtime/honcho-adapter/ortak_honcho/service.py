"""One transaction owns the receipt, native messages, embeddings, and queue."""

from sqlalchemy import insert, select
from src import crud, schemas
from src import models as native
from src.deriver.enqueue import handle_session

from . import PROTOCOL
from .database import bounds, conflict, lock, replay, request_hash
from .models import write_receipts
from .resources import owned_bundle, owned_session


async def remember(db, workspace_id, session_id, body):
    data = body.model_dump(mode="json", exclude_none=True)
    fingerprint = request_hash(
        {"workspace_id": workspace_id, "session_id": session_id, **data}
    )
    await bounds(db)
    await lock(db, f"ortak-remember:{workspace_id}:{session_id}:{body.idempotency_key}")
    receipt = (
        (
            await db.execute(
                select(write_receipts).where(
                    write_receipts.c.workspace_id == workspace_id,
                    write_receipts.c.session_id == session_id,
                    write_receipts.c.idempotency_key == body.idempotency_key,
                )
            )
        )
        .mappings()
        .first()
    )
    bundle, workspace = await owned_bundle(
        db, workspace_id, str(body.company_id), body.employee_id
    )
    context = {
        "protocol": PROTOCOL,
        "company_id": str(body.company_id),
        "employee_id": body.employee_id,
        "scope": body.scope.model_dump(mode="json", exclude_none=True),
    }
    session = await owned_session(db, workspace_id, session_id, context, bundle)
    if receipt:
        response = replay(receipt, fingerprint)
        current = (
            await db.scalars(
                select(native.Message)
                .where(
                    native.Message.workspace_name == workspace_id,
                    native.Message.session_name == session_id,
                    native.Message.public_id.in_(response["record_refs"]),
                )
                .with_for_update(read=True)
            )
        ).all()
        frozen = {item["record_ref"]: item for item in response["records"]}
        if len(current) != len(frozen) or any(
            message.content != frozen[message.public_id]["content"]
            or message.h_metadata != frozen[message.public_id]["metadata"]
            or message.peer_name != bundle["response"]["employee_peer"]
            for message in current
        ):
            conflict("remembered_message_changed")
        await db.commit()
        return response, False
    originals = [
        schemas.MessageCreate(
            content=fact.content,
            peer_id=bundle["response"]["employee_peer"],
            created_at=fact.provenance.recorded_at,
            metadata={
                "ortak": {
                    **context,
                    "write_key": body.idempotency_key,
                    "request_hash": fingerprint,
                    "fact_index": index,
                    "provenance": fact.provenance.model_dump(
                        mode="json", exclude_none=True
                    ),
                }
            },
        )
        for index, fact in enumerate(body.facts)
    ]
    messages = await crud.create_messages(
        db,
        messages=originals,
        workspace_name=workspace_id,
        session_name=session_id,
        prepared_session=session,
        commit=False,
    )
    payloads = [
        {
            "workspace_name": workspace_id,
            "session_name": session_id,
            "message_id": message.id,
            "content": message.content,
            "peer_name": message.peer_name,
            "created_at": message.created_at,
            "message_public_id": message.public_id,
            "seq_in_session": message.seq_in_session,
            "configuration": original.configuration,
        }
        for message, original in zip(messages, originals, strict=True)
    ]
    queue = await handle_session(
        db,
        payloads,
        workspace_id,
        session_id,
        prepared_session=session,
        prepared_workspace=workspace,
    )
    if queue:
        await db.execute(insert(native.QueueItem), queue)
    response = {
        "protocol": PROTOCOL,
        "workspace_id": workspace_id,
        "session_id": session_id,
        "request_hash": fingerprint,
        "record_refs": [m.public_id for m in messages],
        "records": [
            {
                "record_ref": message.public_id,
                "content": message.content,
                "scope": context["scope"],
                "provenance": fact.provenance.model_dump(
                    mode="json", exclude_none=True
                ),
                "metadata": message.h_metadata,
            }
            for message, fact in zip(messages, body.facts, strict=True)
        ],
    }
    await db.execute(
        insert(write_receipts).values(
            workspace_id=workspace_id,
            session_id=session_id,
            idempotency_key=body.idempotency_key,
            request_hash=fingerprint,
            response=response,
        )
    )
    await db.commit()
    return response, True
