"""Native Honcho app plus authenticated, bounded Ortak extension routes."""

import asyncio
from contextlib import asynccontextmanager

from fastapi import APIRouter, Depends, HTTPException, Response
from sqlalchemy import select
from sqlalchemy.exc import DBAPIError, IntegrityError
from src.config import settings
from src.db import engine
from src.dependencies import db
from src.main import app
from src.security import require_auth

from . import PROTOCOL
from .models import TABLES
from .recall import recall
from .resources import create_resources, inspect_resources
from .schemas import CreateResources, InspectResources, Name, Recall, Remember
from .service import remember


def auth_enabled():
    if not settings.AUTH.USE_AUTH:
        raise HTTPException(503, detail="honcho_authentication_required")


router = APIRouter(prefix="/v3/ortak", dependencies=[Depends(auth_enabled)])


async def bounded(operation):
    try:
        async with asyncio.timeout(10):
            return await operation
    except TimeoutError:
        raise HTTPException(503, detail="honcho_operation_timeout") from None
    except IntegrityError:
        raise HTTPException(409, detail="honcho_concurrent_conflict") from None
    except DBAPIError:
        raise HTTPException(503, detail="honcho_database_retry") from None


@router.get("/protocol", dependencies=[Depends(require_auth())])
async def protocol():
    # This is a wire contract, not an Ortak activation/capability health claim.
    return {"protocol": PROTOCOL, "honcho_version": "3.1.1"}


@router.post("/resources/create", dependencies=[Depends(require_auth(admin=True))])
async def create_route(body: CreateResources, response: Response, connection=db):
    result, created = await bounded(create_resources(connection, body))
    response.status_code = 201 if created else 200
    return result


@router.post(
    "/workspaces/{workspace_id}/resources/inspect",
    dependencies=[Depends(require_auth(workspace_name="workspace_id"))],
)
async def inspect_route(workspace_id: Name, body: InspectResources, connection=db):
    return await bounded(inspect_resources(connection, workspace_id, body))


SESSION_AUTH = Depends(
    require_auth(workspace_name="workspace_id", session_name="session_id")
)


@router.post(
    "/workspaces/{workspace_id}/sessions/{session_id}/remember",
    dependencies=[SESSION_AUTH],
)
async def remember_route(
    workspace_id: Name,
    session_id: Name,
    body: Remember,
    response: Response,
    connection=db,
):
    result, created = await bounded(
        remember(connection, workspace_id, session_id, body)
    )
    response.status_code = 201 if created else 200
    return result


@router.post(
    "/workspaces/{workspace_id}/sessions/{session_id}/recall",
    dependencies=[SESSION_AUTH],
)
async def recall_route(
    workspace_id: Name, session_id: Name, body: Recall, connection=db
):
    return await bounded(recall(connection, workspace_id, session_id, body))


class BodyLimit:
    """Bound extension request buffering, including missing/chunked lengths."""

    def __init__(self, app):
        self.app = app

    async def __call__(self, scope, receive, send):
        if scope["type"] != "http" or not scope["path"].startswith("/v3/ortak/"):
            return await self.app(scope, receive, send)
        from starlette.responses import JSONResponse

        try:
            async with asyncio.timeout(10):
                chunks, size = [], 0
                while True:
                    message = await receive()
                    if message["type"] == "http.disconnect":
                        return
                    data = message.get("body", b"")
                    size += len(data)
                    if size > 1152 * 1024:
                        return await JSONResponse({"detail": "request_too_large"}, 413)(
                            scope, receive, send
                        )
                    chunks.append(data)
                    if not message.get("more_body", False):
                        break
        except TimeoutError:
            return await JSONResponse({"detail": "request_body_timeout"}, 408)(
                scope, receive, send
            )
        body = b"".join(chunks)
        delivered = False

        async def replay_body():
            nonlocal delivered
            if delivered:
                return await receive()
            delivered = True
            return {"type": "http.request", "body": body, "more_body": False}

        await self.app(scope, replay_body, send)


native_lifespan = app.router.lifespan_context


@asynccontextmanager
async def extension_lifespan(application):
    auth_enabled()
    async with engine.connect() as connection:
        for table in TABLES:
            await connection.execute(select(table).limit(0))
    async with native_lifespan(application):
        yield


app.router.lifespan_context = extension_lifespan
app.include_router(router)
app.add_middleware(BodyLimit)
