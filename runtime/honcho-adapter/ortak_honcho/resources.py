"""Create-only bundles and owned sessions, without native get-or-create commits."""

from sqlalchemy import insert, select
from sqlalchemy.exc import IntegrityError
from src import models as native
from src.utils.scopes import is_scope_peer

from . import PROTOCOL
from .database import bounds, conflict, lock, replay, request_hash
from .models import resource_receipts, session_ownership


async def create_resources(db, body):
    data = body.model_dump(mode="json")
    fingerprint = request_hash(data)
    await bounds(db)
    await lock(
        db, f"ortak-create:{body.company_id}:{body.employee_id}:{body.idempotency_key}"
    )
    receipt = (
        (
            await db.execute(
                select(resource_receipts).where(
                    resource_receipts.c.company_id == str(body.company_id),
                    resource_receipts.c.employee_id == body.employee_id,
                    resource_receipts.c.idempotency_key == body.idempotency_key,
                )
            )
        )
        .mappings()
        .first()
    )
    if receipt:
        response = replay(receipt, fingerprint)
        await owned_bundle(
            db, body.workspace_id, str(body.company_id), body.employee_id
        )
        await db.commit()
        return response, False
    await lock(db, "ortak-workspace:" + body.workspace_id)
    if await db.scalar(
        select(native.Workspace.id).where(native.Workspace.name == body.workspace_id)
    ):
        conflict("workspace_already_exists")
    if await db.scalar(
        select(resource_receipts.c.workspace_id).where(
            resource_receipts.c.workspace_id == body.workspace_id
        )
    ):
        conflict("workspace_receipt_already_exists")
    owner = {
        "protocol": PROTOCOL,
        "company_id": str(body.company_id),
        "employee_id": body.employee_id,
    }
    workspace = native.Workspace(name=body.workspace_id, h_metadata={"ortak": owner})
    db.add(workspace)
    try:
        await db.flush()
        peers = [
            native.Peer(
                name=name, workspace_name=body.workspace_id, h_metadata={"ortak": owner}
            )
            for name in (body.user_peer, body.employee_peer)
        ]
        db.add_all(peers)
        await db.flush()
        response = {
            "protocol": PROTOCOL,
            "workspace_id": body.workspace_id,
            "user_peer": body.user_peer,
            "employee_peer": body.employee_peer,
            "ownership": "created",
        }
        await db.execute(
            insert(resource_receipts).values(
                company_id=str(body.company_id),
                employee_id=body.employee_id,
                idempotency_key=body.idempotency_key,
                workspace_id=body.workspace_id,
                request_hash=fingerprint,
                response=response,
                native_ids={
                    "workspace": workspace.id,
                    "peers": {p.name: p.id for p in peers},
                },
            )
        )
        await db.commit()
    except IntegrityError:
        await db.rollback()
        conflict("resource_creation_conflict")
    return response, True


async def owned_bundle(db, workspace_id, company_id, employee_id):
    """Lock current native identities; a receipt never authorizes a replacement."""
    receipt = (
        (
            await db.execute(
                select(resource_receipts).where(
                    resource_receipts.c.workspace_id == workspace_id,
                    resource_receipts.c.company_id == company_id,
                    resource_receipts.c.employee_id == employee_id,
                )
            )
        )
        .mappings()
        .first()
    )
    if not receipt:
        conflict("owned_bundle_required")
    workspace = await db.scalar(
        select(native.Workspace)
        .where(native.Workspace.name == workspace_id)
        .with_for_update(read=True)
    )
    peer_names = (
        receipt["response"]["user_peer"],
        receipt["response"]["employee_peer"],
    )
    peers = (
        await db.scalars(
            select(native.Peer)
            .where(
                native.Peer.workspace_name == workspace_id,
                native.Peer.name.in_(peer_names),
            )
            .order_by(native.Peer.name)
            .with_for_update(read=True)
        )
    ).all()
    owner = {"protocol": PROTOCOL, "company_id": company_id, "employee_id": employee_id}
    if (
        workspace is None
        or workspace.id != receipt["native_ids"]["workspace"]
        or workspace.h_metadata.get("ortak") != owner
        or len(peers) != 2
        or any(
            p.id != receipt["native_ids"]["peers"].get(p.name)
            or p.h_metadata.get("ortak") != owner
            or is_scope_peer(p.name, p.internal_metadata)
            for p in peers
        )
    ):
        conflict("resource_identity_changed")
    return receipt, workspace


async def inspect_resources(db, workspace_id, body):
    """Read the frozen receipt only after locking and checking current native IDs."""
    await bounds(db)
    receipt, _ = await owned_bundle(
        db, workspace_id, str(body.company_id), body.employee_id
    )
    response = receipt["response"]
    if (response["user_peer"] != body.user_peer
            or response["employee_peer"] != body.employee_peer):
        conflict("resource_binding_changed")
    result = {
        **response,
        "company_id": str(body.company_id),
        "employee_id": body.employee_id,
        "request_hash": receipt["request_hash"],
        "native_ids": receipt["native_ids"],
    }
    # No insert/update, get-or-create, session, provider or queue operation.
    await db.commit()
    return result


async def owned_session(db, workspace_id, session_id, context, receipt, *, create=True):
    await lock(db, f"ortak-session:{workspace_id}:{session_id}")
    session = await db.scalar(
        select(native.Session)
        .where(
            native.Session.workspace_name == workspace_id,
            native.Session.name == session_id,
        )
        .with_for_update()
    )
    ownership = (
        (
            await db.execute(
                select(session_ownership).where(
                    session_ownership.c.workspace_id == workspace_id,
                    session_ownership.c.session_id == session_id,
                )
            )
        )
        .mappings()
        .first()
    )
    peer_names = {
        receipt["response"]["user_peer"],
        receipt["response"]["employee_peer"],
    }
    if ownership or session:
        if (
            not ownership
            or not session
            or not session.is_active
            or ownership["native_id"] != session.id
            or ownership["context"] != context
            or session.h_metadata.get("ortak") != context
        ):
            conflict("session_ownership_conflict")
        memberships = (
            (
                await db.execute(
                    select(native.session_peers_table)
                    .where(
                        native.session_peers_table.c.workspace_name == workspace_id,
                        native.session_peers_table.c.session_name == session_id,
                    )
                    .with_for_update(read=True)
                )
            )
            .mappings()
            .all()
        )
        if {m["peer_name"] for m in memberships} != peer_names or any(
            m["left_at"] for m in memberships
        ):
            conflict("session_membership_changed")
        return session
    if not create:
        return None
    session = native.Session(
        name=session_id, workspace_name=workspace_id, h_metadata={"ortak": context}
    )
    db.add(session)
    await db.flush()
    await db.execute(
        insert(native.session_peers_table),
        [
            {
                "workspace_name": workspace_id,
                "session_name": session_id,
                "peer_name": peer,
                "configuration": {},
            }
            for peer in sorted(peer_names)
        ],
    )
    await db.execute(
        insert(session_ownership).values(
            workspace_id=workspace_id,
            session_id=session_id,
            native_id=session.id,
            context=context,
        )
    )
    return session
