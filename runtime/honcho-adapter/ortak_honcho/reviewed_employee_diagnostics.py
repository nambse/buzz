"""Finite synthetic I/O operations; no periodic probe or runtime authority."""

from sqlalchemy import delete, insert, select

from .database import conflict, request_hash
from .reviewed_employee_models import diagnostics, diagnostic_content, diagnostic_tombstones
from .reviewed_employee_provenance import digest, utc
from .reviewed_employee_store import body_hash, finish, matching, pins, prepare, quota, row, target


def diagnostic_pins(identity, body, challenge_hash):
    return {**pins(identity), "employee_revision_id": body.employee_revision_id,
            "employee_lifecycle_epoch": body.employee_lifecycle_epoch,
            "challenge_hash": challenge_hash}


def commitment(identity, body, operation, action):
    value = {"format": "ortak-reviewed-employee-diagnostic/1" if action == "write"
             else "ortak-reviewed-employee-diagnostic-withdraw/1",
             "operation_id": operation, "namespace_hash": identity["namespace_hash"],
             "binding_hash": identity["binding_hash"],
             "employee_revision_id": body.employee_revision_id,
             "employee_lifecycle_epoch": body.employee_lifecycle_epoch}
    value["challenge" if action == "write" else "challenge_hash"] = (
        body.challenge if action == "write" else body.challenge_hash
    )
    return request_hash(value)


async def pair(db, workspace, employee, operation, expected):
    header = await row(db, diagnostics, workspace, employee, operation)
    dead = await row(db, diagnostic_tombstones, workspace, employee, operation)
    matching(header, expected)
    matching(dead, expected)
    return header, dead


async def project(db, workspace, employee, operation, identity, expected, read=False):
    header, dead = await pair(db, workspace, employee, operation, expected)
    if header is None and dead is None:
        conflict("employee_diagnostic_missing")
    challenge = await db.scalar(select(diagnostic_content.c.challenge).where(
        *target(diagnostic_content, workspace, employee, operation)
    ))
    if dead is not None and challenge is not None:
        conflict("employee_diagnostic_erasure_not_proven")
    if dead is None and (challenge is None or digest(challenge) != expected["challenge_hash"]):
        conflict("employee_diagnostic_content_missing")
    return {**identity, "operation_id": operation,
        **{key: expected[key] for key in (
            "employee_revision_id", "employee_lifecycle_epoch", "challenge_hash"
        )},
        "write_request_hash": header["write_request_hash"] if header is not None else None,
        "withdraw_request_hash": dead["withdraw_request_hash"] if dead is not None else None,
        "challenge": challenge if read and dead is None else None,
        "erased": dead is not None and challenge is None,
        "tombstone_at": utc(dead["created_at"]) if dead is not None else None}


async def mutate(db, workspace, employee, operation, body, action):
    identity = await prepare(db, workspace, employee, body)
    challenge_hash = digest(body.challenge) if action == "write" else body.challenge_hash
    expected = diagnostic_pins(identity, body, challenge_hash)
    request = commitment(identity, body, operation, action)
    fingerprint = body_hash(body, operation, action)
    header, dead = await pair(db, workspace, employee, operation, expected)
    selected = header if action == "write" else dead
    if selected is not None:
        matching(selected, {"body_hash": fingerprint,
                            f"{action}_request_hash": request})
    else:
        await quota(db, diagnostics, diagnostic_tombstones, workspace, employee, operation, 128)
        key = {"workspace_id": workspace, "employee_id": employee, "operation_id": operation}
        if action == "write":
            await db.execute(insert(diagnostics).values(**key, **expected,
                body_hash=fingerprint, write_request_hash=request))
            if dead is None:
                await db.execute(insert(diagnostic_content).values(**key, challenge=body.challenge))
        else:
            await db.execute(insert(diagnostic_tombstones).values(**key, **expected,
                body_hash=fingerprint, withdraw_request_hash=request))
            await db.execute(delete(diagnostic_content).where(
                *target(diagnostic_content, workspace, employee, operation)
            ))
    result = await project(db, workspace, employee, operation, identity, expected)
    return await finish(db, result), selected is None


async def read(db, workspace, employee, operation, body):
    identity = await prepare(db, workspace, employee, body)
    expected = diagnostic_pins(identity, body, body.challenge_hash)
    result = await project(db, workspace, employee, operation, identity, expected, read=True)
    return await finish(db, result)
