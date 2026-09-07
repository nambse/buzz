"""Canonical employee v1 claims, never current source/ACL authorization."""

import hashlib
import json
import re
from datetime import datetime, timezone

from .database import canonical
from .reviewed_employee_schemas import nonnil


def digest(value):
    return hashlib.sha256(value.encode()).hexdigest()


def exact(value, fields):
    if not isinstance(value, dict) or set(value) != set(fields.split()):
        raise ValueError("invalid employee provenance object")


def hash_value(value):
    if not isinstance(value, str) or not re.fullmatch(r"[0-9a-f]{64}", value):
        raise ValueError("invalid employee provenance hash")


def timestamp(value):
    if not isinstance(value, str) or not re.fullmatch(
        r"[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}\.[0-9]{6}Z", value
    ):
        raise ValueError("invalid employee provenance timestamp")
    parsed = datetime.fromisoformat(value[:-1] + "+00:00")
    if parsed.year < 1970 or utc(parsed) != value:
        raise ValueError("invalid employee provenance timestamp")
    return parsed


def utc(value):
    return value.astimezone(timezone.utc).isoformat(timespec="microseconds").replace("+00:00", "Z")


def pairs(items):
    value = {}
    for key, item in items:
        if key in value:
            raise ValueError("duplicate employee provenance key")
        value[key] = item
    return value


def validate(body):
    encoded = body.provenance
    if len(encoded.encode()) > 4096:
        raise ValueError("employee provenance bound")
    try:
        value = json.loads(encoded, object_pairs_hook=pairs)
    except RecursionError:
        raise ValueError("employee provenance depth") from None
    exact(value, "approval audience audience_hash format source source_hash")
    if canonical(value) != encoded or value["format"] != "ortak-reviewed-employee-provenance/1":
        raise ValueError("noncanonical employee provenance")
    a, s, p = value["audience"], value["source"], value["approval"]
    exact(a, "company_id destination_channel_id destination_community_id employee_id format human_public_key kind")
    exact(s, "author_public_key channel_id community_id event_created_at event_id evidence_hash")
    exact(p, "approval_id approved_by content_hash expires_at format")
    for candidate in (a["company_id"], a["destination_channel_id"], a["destination_community_id"],
                      s["community_id"], s["channel_id"], p["approval_id"]):
        if not isinstance(candidate, str):
            raise ValueError("invalid provenance UUID")
        nonnil(candidate)
    for candidate in (s["author_public_key"], s["event_id"], s["evidence_hash"],
                      p["approved_by"], p["content_hash"], value["audience_hash"], value["source_hash"]):
        hash_value(candidate)
    if a["kind"] == "relationship":
        hash_value(a["human_public_key"])
        if a["human_public_key"] != p["approved_by"]:
            raise ValueError("relationship approver mismatch")
    elif a["kind"] != "experience" or a["human_public_key"] is not None:
        raise ValueError("invalid employee audience kind")
    if (a["format"] != "ortak-reviewed-employee-audience/1"
        or p["format"] != "ortak-reviewed-employee-sharing/1"
        or a["company_id"] != body.company_id or a["employee_id"] != body.employee_id
        or a["destination_channel_id"] != body.destination_channel_id
        or s["community_id"] != a["destination_community_id"]
        or s["author_public_key"] != p["approved_by"] or p["content_hash"] != body.content_hash
        or len(canonical(a).encode()) > 2048
        or digest(canonical(a)) != value["audience_hash"]
        or digest(canonical({"audience_hash": value["audience_hash"],
            "format": "ortak-reviewed-employee-source/1", "source": s})) != body.source_hash
        or value["source_hash"] != body.source_hash or digest(encoded) != body.sharing_hash):
        raise ValueError("employee provenance commitment mismatch")
    timestamp(s["event_created_at"])
    timestamp(p["expires_at"])
    return value
