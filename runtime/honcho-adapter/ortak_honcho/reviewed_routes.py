"""Workspace-authorized project record family, independent from legacy sessions."""

from uuid import UUID
from fastapi import APIRouter, Depends, Response
from src.dependencies import db
from src.security import require_auth

from .reviewed import erase, inspect, publish, recall, recall_selected
from .reviewed_schemas import (
    ReviewedInspect,
    ReviewedMutation,
    ReviewedPublish,
    ReviewedRecall,
    ReviewedSelectedRecall,
)
from .schemas import Name


def reviewed_router(bounded):
    router = APIRouter(
        prefix="/workspaces/{workspace_id}/reviewed-projects/{project_id}",
        dependencies=[Depends(require_auth(workspace_name="workspace_id"))],
    )

    @router.post("/records/{record_id}/publish")
    async def publish_route(
        workspace_id: Name,
        project_id: UUID,
        record_id: UUID,
        body: ReviewedPublish,
        response: Response,
        connection=db,
    ):
        result, created = await bounded(
            publish(connection, workspace_id, project_id, record_id, body)
        )
        response.status_code = 201 if created else 200
        return result

    @router.post("/records/{record_id}/withdraw")
    async def withdraw_route(
        workspace_id: Name,
        project_id: UUID,
        record_id: UUID,
        body: ReviewedMutation,
        connection=db,
    ):
        result, _ = await bounded(
            erase(connection, workspace_id, project_id, record_id, body)
        )
        return result

    @router.post("/records/{record_id}/expire")
    async def expire_route(
        workspace_id: Name,
        project_id: UUID,
        record_id: UUID,
        body: ReviewedMutation,
        connection=db,
    ):
        result, _ = await bounded(
            erase(connection, workspace_id, project_id, record_id, body, expired=True)
        )
        return result

    @router.post("/inspect")
    async def inspect_route(
        workspace_id: Name, project_id: UUID, body: ReviewedInspect, connection=db
    ):
        return await bounded(inspect(connection, workspace_id, project_id, body))

    @router.post("/recall")
    async def recall_route(
        workspace_id: Name, project_id: UUID, body: ReviewedRecall, connection=db
    ):
        return await bounded(recall(connection, workspace_id, project_id, body))

    @router.post("/recall-selected")
    async def recall_selected_route(
        workspace_id: Name,
        project_id: UUID,
        body: ReviewedSelectedRecall,
        connection=db,
    ):
        return await bounded(
            recall_selected(connection, workspace_id, project_id, body)
        )

    return router
