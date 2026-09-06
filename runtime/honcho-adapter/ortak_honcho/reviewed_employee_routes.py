"""Workspace-authenticated employee family, isolated from project/session APIs."""

from fastapi import APIRouter, Depends, HTTPException, Response
from fastapi.exceptions import RequestValidationError
from fastapi.routing import APIRoute
from src.dependencies import db
from src.security import require_auth

from . import reviewed_employee, reviewed_employee_diagnostics
from .reviewed_employee_schemas import Common, DiagnosticRead, DiagnosticWrite, Id, Mutation, Publish, Selected
from .reviewed_employee_store import namespace
from .schemas import Employee, Name


class EmployeeRoute(APIRoute):
    """Validation failures never echo submitted reviewed text or provenance."""

    def get_route_handler(self):
        handler = super().get_route_handler()

        async def checked(request):
            try:
                return await handler(request)
            except (RequestValidationError, RecursionError):
                raise HTTPException(422, detail="employee_request_invalid") from None

        return checked


def employee_router(bounded):
    router = APIRouter(
        prefix="/workspaces/{workspace_id}/reviewed-employees/{employee_id}",
        dependencies=[Depends(require_auth(workspace_name="workspace_id"))],
        route_class=EmployeeRoute,
    )

    @router.post("/namespace")
    async def namespace_route(workspace_id: Name, employee_id: Employee, body: Common, connection=db):
        return await bounded(namespace(connection, workspace_id, employee_id, body))

    @router.post("/records/{record_id}/publish")
    async def publish_route(workspace_id: Name, employee_id: Employee, record_id: Id,
                            body: Publish, response: Response, connection=db):
        result, created = await bounded(reviewed_employee.mutate(
            connection, workspace_id, employee_id, record_id, body, "publish"
        ))
        response.status_code = 201 if created else 200
        return result

    @router.post("/records/{record_id}/withdraw")
    async def withdraw_route(workspace_id: Name, employee_id: Employee, record_id: Id,
                             body: Mutation, connection=db):
        result, _ = await bounded(reviewed_employee.mutate(
            connection, workspace_id, employee_id, record_id, body, "withdraw"
        ))
        return result

    @router.post("/recall-selected")
    async def recall_route(workspace_id: Name, employee_id: Employee, body: Selected, connection=db):
        return await bounded(reviewed_employee.recall_selected(connection, workspace_id, employee_id, body))

    @router.post("/diagnostics/{operation_id}/write")
    async def diagnostic_write(workspace_id: Name, employee_id: Employee, operation_id: Id,
                               body: DiagnosticWrite, response: Response, connection=db):
        result, created = await bounded(reviewed_employee_diagnostics.mutate(
            connection, workspace_id, employee_id, operation_id, body, "write"
        ))
        response.status_code = 201 if created else 200
        return result

    @router.post("/diagnostics/{operation_id}/read")
    async def diagnostic_read(workspace_id: Name, employee_id: Employee, operation_id: Id,
                              body: DiagnosticRead, connection=db):
        return await bounded(reviewed_employee_diagnostics.read(
            connection, workspace_id, employee_id, operation_id, body
        ))

    @router.post("/diagnostics/{operation_id}/withdraw")
    async def diagnostic_withdraw(workspace_id: Name, employee_id: Employee, operation_id: Id,
                                  body: DiagnosticRead, connection=db):
        result, _ = await bounded(reviewed_employee_diagnostics.mutate(
            connection, workspace_id, employee_id, operation_id, body, "withdraw"
        ))
        return result

    return router
