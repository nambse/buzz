"""Apply two opt-in transaction seams, only to the exact reviewed source files."""

import hashlib
import json
import sys
from pathlib import Path


def replace_once(text, old, new):
    if text.count(old) != 1:
        raise ValueError("pinned patch anchor is not unique")
    return text.replace(old, new, 1)


def patch(source: Path):
    lock = json.loads(Path(__file__).with_name("honcho-source-lock.json").read_text())
    originals = {}
    for name, expected in lock["patched_inputs"].items():
        data = (source / name).read_bytes()
        if hashlib.sha256(data).hexdigest() != expected:
            raise ValueError(f"source hash mismatch: {name}")
        originals[name] = data.decode()
    name = "src/crud/message.py"
    data = originals[name]
    data = replace_once(
        data,
        '    session_name: str,\n) -> list[models.Message]:\n    """\n    Bulk create',
        '    session_name: str,\n    *,\n    prepared_session: models.Session | None = None,\n    commit: bool = True,\n) -> list[models.Message]:\n    """\n    Bulk create',
    )
    old = """    # Get or create session with peers in messages list
    peers = {message.peer_name: schemas.SessionPeerConfig() for message in messages}
    await get_or_create_session(
        db,
        session=schemas.SessionCreate(name=session_name, peers=peers),
        workspace_name=workspace_name,
    )

    await db.execute(text("SET LOCAL lock_timeout = '5s'"))"""
    new = """    # Ortak owns session validation/locks and the surrounding transaction.
    if prepared_session is None:
        if not commit:
            raise ValueError("flush-only writes require a prepared session")
        peers = {message.peer_name: schemas.SessionPeerConfig() for message in messages}
        await get_or_create_session(
            db, session=schemas.SessionCreate(name=session_name, peers=peers),
            workspace_name=workspace_name,
        )
    elif (prepared_session.name != session_name
          or prepared_session.workspace_name != workspace_name
          or not prepared_session.is_active):
        raise ValueError("prepared session does not match message scope")

    if commit:
        await db.execute(text("SET LOCAL lock_timeout = '5s'"))"""
    data = replace_once(data, old, new)
    data = replace_once(
        data,
        "    await db.commit()\n\n    return message_objects",
        "    if commit:\n        await db.commit()\n    else:\n        await db.flush()\n\n    return message_objects",
    )
    patched = {name: data}
    name = "src/deriver/enqueue.py"
    data = originals[name]
    data = replace_once(
        data,
        '    session_name: str,\n) -> list[dict[str, Any]]:\n    """\n    Handle enqueueing',
        '    session_name: str,\n    *,\n    prepared_session: models.Session | None = None,\n    prepared_workspace: models.Workspace | None = None,\n) -> list[dict[str, Any]]:\n    """\n    Handle enqueueing',
    )
    old = """    session = (
        await crud.get_or_create_session(
            db_session,
            session=schemas.SessionCreate(name=session_name),
            workspace_name=workspace_name,
        )
    ).resource

    # Fetch workspace for configuration resolution
    workspace = await crud.get_workspace(db_session, workspace_name=workspace_name)"""
    new = """    if prepared_session is not None or prepared_workspace is not None:
        if (prepared_session is None or prepared_workspace is None
                or prepared_session.name != session_name
                or prepared_session.workspace_name != workspace_name
                or prepared_workspace.name != workspace_name):
            raise ValueError("prepared queue scope does not match")
        session, workspace = prepared_session, prepared_workspace
    else:
        session = (
            await crud.get_or_create_session(
                db_session, session=schemas.SessionCreate(name=session_name),
                workspace_name=workspace_name,
            )
        ).resource
        workspace = await crud.get_workspace(db_session, workspace_name=workspace_name)"""
    patched[name] = replace_once(data, old, new)
    for name, data in patched.items():
        (source / name).write_text(data)


if __name__ == "__main__":
    patch(Path(sys.argv[1]))
