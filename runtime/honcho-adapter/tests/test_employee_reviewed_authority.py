"""Actual workspace JWT/native generation, canonical claims and wire bounds."""

from copy import deepcopy
from uuid import uuid4

from sqlalchemy import delete, select
from src import models as native
from src.config import settings
from src.db import SessionLocal
from src.security import JWTParams, create_jwt

from .employee_reviewed_fixture import base, canonical, namespace, publish, recall, selected, sha, stored, url


async def test_employee_namespace_binds_original_native_generation_and_workspace_jwt(client, monkeypatch):
    owner, common = await selected(client)
    path = base(owner) + "/namespace"
    initial = await stored(owner)
    result = await client.post(path, json=common)
    assert result.status_code == 200 and result.json() == namespace(common)
    assert (await client.get("/v3/ortak/protocol")).json() == {"protocol": "ortak-honcho/1", "honcho_version": "3.1.1"}
    assert await stored(owner) == initial
    foreign = create_jwt(JWTParams(w="foreign_workspace"))
    denied = await client.post(path, json=common, headers={"Authorization": "Bearer " + foreign})
    assert denied.status_code in {401, 403}
    for field in ("company_id", "employee_id", "binding", "ownership"):
        changed = deepcopy(common)
        if field == "company_id":
            changed[field] = str(uuid4())
        elif field == "employee_id":
            changed[field] = "other-employee"
        elif field == "binding":
            changed[field]["user_peer"], changed[field]["employee_peer"] = (
                changed[field]["employee_peer"], changed[field]["user_peer"]
            )
        else:
            changed[field]["native_ids"]["workspace"] = "replacement"
        response = await client.post(path, json=changed)
        assert response.status_code == 409, response.text
    monkeypatch.setattr(settings.AUTH, "USE_AUTH", False)
    assert (await client.post(path, json=common)).status_code == 503
    monkeypatch.setattr(settings.AUTH, "USE_AUTH", True)
    async with SessionLocal() as db:
        peer = await db.scalar(select(native.Peer).where(
            native.Peer.workspace_name == owner["workspace_id"], native.Peer.name == owner["employee_peer"]))
        metadata, old_id = deepcopy(peer.h_metadata), peer.id
        await db.execute(delete(native.Peer).where(native.Peer.id == old_id))
        replacement = native.Peer(name=owner["employee_peer"], workspace_name=owner["workspace_id"], h_metadata=metadata)
        db.add(replacement)
        await db.flush()
        assert replacement.id != old_id
        await db.commit()
    rejected = await client.post(path, json=common)
    assert rejected.status_code == 409 and rejected.json()["detail"] == "resource_identity_changed"
    assert await stored(owner) == initial


async def test_employee_canonical_provenance_hashes_and_conservative_own_source_are_enforced(client):
    owner, common = await selected(client)
    record = str(uuid4())
    original = publish(common, record, kind="relationship")
    # Rehash tampered canonical provenance, rather than relying on a stale outer
    # hash to reject these independent source/audience/approval predicates.
    import json
    changes = [
        ("source", "author_public_key", "c" * 64),
        ("source", "community_id", str(uuid4())),
        ("audience", "human_public_key", "c" * 64),
        ("audience", "destination_channel_id", str(uuid4())),
        ("approval", "content_hash", "f" * 64),
        ("approval", "expires_at", "2026-09-07T00:00:00Z"),
    ]
    for section, key, value in changes:
        claims = json.loads(original["provenance"])
        claims[section][key] = value
        claims["audience_hash"] = sha(canonical(claims["audience"]))
        claims["source_hash"] = sha(canonical({"format": "ortak-reviewed-employee-source/1",
            "audience_hash": claims["audience_hash"], "source": claims["source"]}))
        encoded = canonical(claims)
        request = {**original, "provenance": encoded, "source_hash": claims["source_hash"], "sharing_hash": sha(encoded)}
        response = await client.post(url(owner, record), json=request)
        assert response.status_code == 422, response.text
        assert response.json() == {"detail": "employee_request_invalid"}
    for encoded in (" " + original["provenance"], original["provenance"][:-1] + ',"format":"duplicate"}'):
        result = await client.post(url(owner, record), json={**original, "provenance": encoded, "sharing_hash": sha(encoded)})
        assert result.status_code == 422
    assert await stored(owner) == (0, 0, 0, 0, 0, 0, 0)
    assert (await client.post(url(owner, record), json=original)).status_code == 201


async def test_employee_new_family_body_limit_closed_shapes_and_validation_omit_text(client):
    owner, common = await selected(client)
    record = str(uuid4())
    request = publish(common, record)
    invalid_requests = [
        {**request, "project_id": str(uuid4())},
        {**request, "content": "private-validation-secret\x00", "content_hash": sha("private-validation-secret\x00")},
        {**request, "content": "ç" * 2049, "content_hash": sha("ç" * 2049)},
        {**request, "company_id": "00000000-0000-0000-0000-000000000000"},
        {**request, "provenance": "[" * 1500 + "0" + "]" * 1500},
    ]
    for invalid in invalid_requests:
        response = await client.post(url(owner, record), json=invalid)
        assert response.status_code == 422
        assert response.json() == {"detail": "employee_request_invalid"}
    for ids in ([], [record, record], [str(uuid4()) for _ in range(9)], ["00000000-0000-0000-0000-000000000000"]):
        assert (await client.post(base(owner) + "/recall-selected",
            json=recall(common, request["destination_channel_id"], ids))).status_code == 422
    query = recall(common, request["destination_channel_id"], [record])
    query.pop("human_public_key")
    assert (await client.post(base(owner) + "/recall-selected", json=query)).status_code == 422
    oversized = canonical({**common, "padding": "x" * 33000}).encode()

    async def chunks():
        for offset in range(0, len(oversized), 4096):
            yield oversized[offset:offset + 4096]

    too_large = await client.post(base(owner) + "/namespace", content=chunks(), headers={"Content-Type": "application/json"})
    assert too_large.status_code == 413 and too_large.json() == {"detail": "request_too_large"}
    # The old family retains its old body budget, reaching its schema validator.
    old = await client.post(f"/v3/ortak/workspaces/{owner['workspace_id']}/resources/inspect",
        content=oversized, headers={"Content-Type": "application/json"})
    assert old.status_code == 422
    assert await stored(owner) == (0, 0, 0, 0, 0, 0, 0)
