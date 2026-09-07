"""Exact owned namespace and finite employee-family storage primitives."""

from sqlalchemy import func, select, union

from .database import bounds, canonical, conflict, lock, request_hash
from .resources import owned_bundle
from .reviewed_employee_provenance import digest

PROTOCOL = "reviewed-employee/1"


def scope(table, workspace, employee):
    return table.c.workspace_id == workspace, table.c.employee_id == employee


def target(table, workspace, employee, identifier):
    key = "record_id" if "record_id" in table.c else "operation_id"
    return (*scope(table, workspace, employee), table.c[key] == identifier)


async def row(db, table, workspace, employee, identifier):
    return (await db.execute(select(table).where(*target(table, workspace, employee, identifier)))).mappings().first()


async def prepare(db, workspace, employee, body):
    if employee != body.employee_id or workspace != body.binding.workspace:
        conflict("employee_namespace_mismatch")
    await bounds(db)
    receipt, _ = await owned_bundle(db, workspace, body.company_id, employee)
    ownership = body.ownership.model_dump()
    if (receipt["request_hash"] != ownership["request_hash"]
        or receipt["native_ids"] != ownership["native_ids"]
        or receipt["response"]["user_peer"] != body.binding.user_peer
        or receipt["response"]["employee_peer"] != body.binding.employee_peer):
        conflict("employee_native_ownership_changed")
    await lock(db, f"ortak-reviewed-employee:{workspace}:{employee}")
    namespace = canonical({"company_id": body.company_id, "employee_id": employee,
                           "format": "ortak-reviewed-employee-namespace/1"})
    namespace_hash = digest(namespace)
    binding = body.binding.model_dump()
    binding_hash = request_hash({"binding": binding, "namespace_hash": namespace_hash, "protocol": PROTOCOL})
    return {"protocol": PROTOCOL, "company_id": body.company_id, "employee_id": employee,
            "deployment_id": body.deployment_id, "binding": binding, "ownership": ownership,
            "namespace": namespace, "namespace_hash": namespace_hash, "binding_hash": binding_hash}


def pins(identity):
    return {key: identity[key] for key in (
        "company_id", "deployment_id", "namespace_hash", "binding_hash", "ownership"
    )}


def matching(found, expected):
    if found is not None and any(found[key] != value for key, value in expected.items()):
        conflict("employee_record_identity_changed")


async def quota(db, header, tombstone, workspace, employee, identifier, maximum):
    key = "record_id" if "record_id" in header.c else "operation_id"
    ids = union(select(header.c[key]).where(*scope(header, workspace, employee)),
                select(tombstone.c[key]).where(*scope(tombstone, workspace, employee))).subquery()
    if await db.scalar(select(ids.c[key]).where(ids.c[key] == identifier)) is None:
        if await db.scalar(select(func.count()).select_from(ids)) >= maximum:
            conflict("employee_scope_limit")


def body_hash(body, identifier, action):
    return request_hash({"protocol": PROTOCOL, "identifier": identifier, "action": action,
                         "body": body.model_dump(mode="json")})


def commitment(identity, body, record, action):
    expected = f"employee-reviewed:{action}:{body.company_id}:{record}"
    if body.idempotency_key != expected:
        conflict("employee_operation_key_mismatch")
    return request_hash({"action": action, "binding_hash": identity["binding_hash"],
        "company_id": body.company_id, "content_hash": body.content_hash,
        "employee_id": body.employee_id, "fact_id": record,
        "format": "ortak-reviewed-employee-remote-request/1",
        "namespace_hash": identity["namespace_hash"], "sharing_hash": body.sharing_hash,
        "source_hash": body.source_hash, "target_id": body.target_id})


async def namespace(db, workspace, employee, body):
    result = await prepare(db, workspace, employee, body)
    return await finish(db, result)


async def finish(db, result):
    """Bound the complete wire value before committing any new evidence."""
    if len(canonical(result).encode()) > 65536:
        conflict("employee_response_limit")
    await db.commit()
    return result
