#!/usr/bin/env python3
"""Bind the fresh private Office to one company and a draft employee atomically.

This fixed local development bootstrap never provisions or activates a runtime,
memory resource, signer or employee. Its API audience contains only public IDs.
"""

import argparse
import json
import os
import re
import subprocess
from pathlib import Path
from uuid import UUID

from init_private_stack import PROJECT, create_file
from private_native_services import private_file, selected_root


def read_snapshot(path):
    """Read a bounded protected snapshot, distinguishing absence from JSON null."""
    try:
        text = private_file(path)
    except FileNotFoundError:
        return None, None
    def unique(pairs):
        result = {}
        for key, value in pairs:
            if key in result:
                raise ValueError("duplicate configuration field")
            result[key] = value
        return result
    return text, json.loads(text, object_pairs_hook=unique)


def creation_permission(value, expected):
    """Accept only the fixed audience and an optional, strictly boolean flag."""
    normalized = json.loads(json.dumps(value))
    if (not isinstance(normalized, dict) or not isinstance(normalized.get("humans"), list)
            or len(normalized["humans"]) != 1 or not isinstance(normalized["humans"][0], dict)):
        raise ValueError("existing API audience differs; preserved")
    creation = normalized["humans"][0].pop("can_create_projects", False)
    if not isinstance(creation, bool) or normalized != expected:
        raise ValueError("existing API audience differs; preserved")
    return creation


def sync_directory(root):
    """Persist the atomic configuration rename and its retained backup entry."""
    descriptor = os.open(root, os.O_RDONLY | os.O_DIRECTORY)
    try:
        os.fsync(descriptor)
    finally:
        os.close(descriptor)


def main() -> None:
    """Verify the selected Office owner, commit the binding, then publish config."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--state-dir", type=Path, required=True)
    parser.add_argument("--community", type=UUID, required=True)
    parser.add_argument("--channel", type=UUID, required=True)
    parser.add_argument("--enable-project-creation", action="store_true",
                        help="allow this verified private owner to create manual channel-bound projects")
    args = parser.parse_args()
    root = selected_root(args.state_dir)
    identity = json.loads(private_file(root / "identities.json"))
    company = UUID(identity["company_id"])
    owner = identity["owner"]["public_key"]
    if (identity.get("project") != PROJECT or identity.get("employee_id") != "ada-private"
            or not re.fullmatch(r"[0-9a-f]{64}", owner)
            or not all((company.int, args.community.int, args.channel.int))):
        raise ValueError("unexpected private identity")
    config = {
        "origin": "http://127.0.0.1:8787",
        "community_id": str(args.community),
        "humans": [{"public_key": owner, "role": "operator",
                    "channel_ids": [str(args.channel)], "employee_ids": ["ada-private"]}],
        "allowed_web_origins": ["http://localhost:1427", "tauri://localhost"],
    }
    destination = root / "api-config.json"
    existing_text, existing = read_snapshot(destination)
    expected = json.loads(json.dumps(config))
    if existing_text is not None:
        previous_creation = creation_permission(existing, expected)
        # A routine bootstrap retry preserves an explicitly enabled capability.
        if previous_creation or "can_create_projects" in existing["humans"][0]:
            config["humans"][0]["can_create_projects"] = previous_creation
    if args.enable_project_creation:
        config["humans"][0]["can_create_projects"] = True
    before = root / "api-config.before-work.json"
    before_text, backup = read_snapshot(before)
    if before_text is not None:
        if creation_permission(backup, expected):
            raise ValueError("retained API configuration differs; preserved")
        if existing_text is not None and existing != config and backup != existing:
            raise ValueError("retained API configuration differs; preserved")
    temporary = root / "api-config.work-next.json"
    pending_text, pending = read_snapshot(temporary)
    if pending_text is not None and pending != config:
        raise ValueError("pending API configuration differs; preserved")
    # All interpolated values are UUIDs, fixed constants, or a validated hex key.
    # One transaction also makes an interrupted config publication retryable.
    sql = f"""
BEGIN;
SET LOCAL lock_timeout = '2s';
SET LOCAL statement_timeout = '5s';
SELECT pg_advisory_xact_lock(hashtextextended('ortak-private-bootstrap-20260905', 0));
DO $bootstrap$
BEGIN
    PERFORM 1 FROM communities c
        JOIN channels ch ON ch.community_id = c.id
        JOIN channel_members cm ON cm.community_id = ch.community_id AND cm.channel_id = ch.id
        WHERE c.id = '{args.community}' AND c.host = 'localhost:3038'
          AND c.deletion_state = 'active' AND c.deleted_at IS NULL AND c.archived_at IS NULL
          AND ch.id = '{args.channel}' AND ch.visibility = 'private'
          AND ch.deleted_at IS NULL AND ch.archived_at IS NULL
          AND cm.pubkey = decode('{owner}', 'hex') AND cm.role = 'owner'
          AND cm.removed_at IS NULL
        FOR SHARE OF c, ch, cm;
    IF NOT FOUND THEN RAISE EXCEPTION 'selected fresh private Office ownership was not verified'; END IF;

    INSERT INTO companies (id, slug, display_name) VALUES
        ('{company}', '{PROJECT}', 'Ortak Private') ON CONFLICT (id) DO NOTHING;
    IF NOT EXISTS (SELECT 1 FROM companies WHERE id = '{company}'
        AND slug = '{PROJECT}' AND status = 'active')
    THEN RAISE EXCEPTION 'company identity or status differs'; END IF;

    INSERT INTO office_company_bindings (community_id, company_id)
        VALUES ('{args.community}', '{company}') ON CONFLICT (community_id) DO NOTHING;
    IF NOT EXISTS (SELECT 1 FROM office_company_bindings
        WHERE community_id = '{args.community}' AND company_id = '{company}')
    THEN RAISE EXCEPTION 'Office is bound to a different company'; END IF;

    INSERT INTO employees (company_id, id) VALUES ('{company}', 'ada-private')
        ON CONFLICT (company_id, id) DO NOTHING;
    IF NOT EXISTS (SELECT 1 FROM employees WHERE company_id = '{company}'
        AND id = 'ada-private' AND status = 'draft' AND active_revision_id IS NULL)
    THEN RAISE EXCEPTION 'employee has progressed beyond this draft bootstrap'; END IF;
END;
$bootstrap$;
COMMIT;
"""
    subprocess.run(
        ["/usr/local/bin/docker", "--host", "unix:///Users/nambse/.docker/run/docker.sock",
         "exec", "-i", f"{PROJECT}-postgres-1",
         "psql", "--no-psqlrc", "--no-password", "--quiet", "--set", "ON_ERROR_STOP=1",
         "-h", "/var/run/postgresql", "-U", "ortak", "-d", "ortak"],
        input=sql.encode(), stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
        env={"PATH": "/usr/bin:/bin:/usr/sbin:/sbin", "LANG": "C", "LC_ALL": "C"},
        timeout=20, check=True,
    )
    # Neither an interrupted write nor a DB roundtrip may hide a concurrent
    # audience edit. Recheck the protected snapshots before publication.
    if read_snapshot(destination) != (existing_text, existing):
        raise ValueError("API configuration changed during bootstrap; preserved")
    if read_snapshot(before) != (before_text, backup) or read_snapshot(temporary) != (pending_text, pending):
        raise ValueError("API publication snapshots changed during bootstrap; preserved")
    if existing_text is None and pending_text is None:
        create_file(destination, json.dumps(config, indent=2) + "\n")
        sync_directory(root)
    elif existing != config or pending_text is not None:
        # Only the explicit capability can differ; retain the previous public
        # configuration and atomically publish the reviewed replacement.
        if existing_text is not None and existing != config and before_text is None:
            create_file(before, existing_text)
        if pending_text is None:
            create_file(temporary, json.dumps(config, indent=2) + "\n")
        os.replace(temporary, destination)
        sync_directory(root)
    print("Verified private Office binding and draft employee; API audience saved. No activation occurred.")


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, KeyError, subprocess.SubprocessError):
        raise SystemExit("Private control bootstrap failed; existing state was preserved. No secrets were logged.") from None
