"""Start/status/stop the registered installation without recreating any resource."""

import http.client
import json
import os
from pathlib import Path
import plistlib
import re
import time

from private_native_services import private_file
from private_stack_install import plist_path
from private_stack_state import CONTAINERS, SERVICES, command, docker, require

DOMAIN = f"gui/{os.getuid()}"
ENDPOINTS = (("relay", 8089, "/_readiness", (200,)),
             ("api", 8787, "/api/v1/employees", (401, 403)),
             ("hermes", 8650, "/v1/capabilities", (401, 403)),
             ("honcho", 8009, "/v3/ortak/protocol", (401, 403)),
             ("object_store", 9008, "/minio/health/ready", (200,)))


def job_target(action):
    """The launchd target is fixed independently of mutable manifest strings."""
    require(action in SERVICES)
    return f"{DOMAIN}/dev.ortak.private-v0.{action}"


def job(installation, action):
    """Verify both the installed plist and the actual loaded job arguments."""
    manifest = installation.manifest
    known = (manifest["plists"][action], manifest["previous_plists"][action])
    path = plist_path(action)
    require(plistlib.loads(private_file(path).encode()) in known)
    code, output = command(["/bin/launchctl", "print", job_target(action)], allow_failure=True)
    if code:
        require("Could not find service" in output)
        return {"loaded": False, "running": False}
    match = re.search(r"\n\s*arguments = \{([^}]+)\}", output)
    require(match is not None)
    arguments = [line.strip() for line in match[1].splitlines() if line.strip()]
    require(arguments in [value["ProgramArguments"] for value in known])
    pid = re.search(r"\n\s*pid = (\d+)", output)
    return {"loaded": True, "running": bool(pid), "pid": int(pid[1]) if pid else None,
            "launcher": "installed" if arguments == known[0]["ProgramArguments"] else "bootstrap"}


def unboot(installation, action):
    """Disable next-login launch and unload exactly the verified owned job."""
    state = job(installation, action)
    command(["/bin/launchctl", "disable", job_target(action)])
    if state["loaded"]:
        command(["/bin/launchctl", "bootout", job_target(action)], timeout=55)
    deadline = time.monotonic() + 55
    while True:
        loaded = job(installation, action)["loaded"]
        alive = False
        if state.get("pid"):
            try:
                os.kill(state["pid"], 0)
                alive = True
            except ProcessLookupError:
                pass
        if not loaded and not alive:
            break
        require(time.monotonic() < deadline)
        time.sleep(0.2)


def boot(installation, action):
    """Install the frozen plist only after its previous job is unloaded."""
    state = job(installation, action)
    if state["loaded"]:
        return
    path = plist_path(action)
    temporary = path.with_suffix(".plist.next")
    with os.fdopen(os.open(temporary, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600), "wb") as stream:
        stream.write(plistlib.dumps(installation.manifest["plists"][action]))
        stream.flush()
        os.fsync(stream.fileno())
    os.replace(temporary, path)
    command(["/bin/launchctl", "enable", job_target(action)])
    command(["/bin/launchctl", "bootstrap", DOMAIN, path])


def probes():
    """Read only local response statuses; a 401 proves an auth boundary, not authorization."""
    result = {}
    for name, port, path, expected in ENDPOINTS:
        connection = http.client.HTTPConnection("127.0.0.1", port, timeout=1)
        try:
            connection.request("GET", path, headers={"Connection": "close"})
            status = connection.getresponse().status
            result[name] = {"responding": status in expected, "http_status": status}
        except (OSError, http.client.HTTPException):
            result[name] = {"responding": False}
        finally:
            connection.close()
    return result


def pending(installation):
    """Read durable obligations using the exact owned local PostgreSQL container."""
    sql = """BEGIN READ ONLY;
    SELECT json_build_object(
      'runs',(SELECT count(*) FROM runs WHERE status IN('queued','running','waiting')),
      'inbox',(SELECT count(*) FROM office_inbox WHERE state IN('pending','claimed')),
      'outbox',(SELECT count(*) FROM outbox WHERE state='pending'),
      'outputs',(SELECT count(*) FROM runtime_work_outputs WHERE state='pending'),
      'memory',(SELECT count(*) FROM runtime_memory_writes WHERE state='pending'),
      'provisioning',(SELECT count(*) FROM provisioning_operations WHERE NOT dry_run AND status IN('pending','running','compensating')),
      'schema',(SELECT max(version) FROM _sqlx_migrations WHERE success));
    COMMIT;"""
    identifier = installation.manifest["containers"]["postgres-1"]["id"]
    output = docker("exec", "-i", identifier, "psql", "-U", "ortak", "-d", "ortak", "-XAtq",
                    "-v", "ON_ERROR_STOP=1", input_bytes=sql.encode(), maximum=8192)[1]
    result = json.loads(output)
    require(result.pop("schema") == installation.manifest["schema_version"])
    require(all(type(count) is int and count >= 0 for count in result.values()))
    return result


def status(installation):
    """Report resource identity separately from auth/provider/model health."""
    containers = installation.verify()
    jobs = {name: job(installation, name) for name in SERVICES}
    counts = pending(installation) if containers["postgres-1"]["Running"] else None
    return {"containers": {name: {"running": row["Running"], "status": row["Status"]}
                           for name, row in containers.items()}, "services": jobs,
            "endpoints": probes(), "pending": counts, "identity": "verified",
            "provider_health": "not_probed", "credentials": "not_loaded"}


def start(installation):
    """Restart the same containers, then load the same bounded service launchers."""
    states = installation.verify()
    for name in SERVICES:
        job(installation, name)
    installation.record("start", "starting_stores")
    for name in CONTAINERS:
        if not states[name]["Running"]:
            docker("start", installation.manifest["containers"][name]["id"], timeout=30)
    deadline = time.monotonic() + 45
    while True:
        current = installation.verify()
        healthy = all(row["Running"] and row.get("Health", {}).get("Status", "healthy") == "healthy"
                      for row in current.values())
        if healthy:
            break
        require(time.monotonic() < deadline)
        time.sleep(1)
    pending(installation)  # Refuse incompatible schemas before any native server starts.
    installation.record("start", "starting_services")
    for name in ("relay", "api", "management", "worker"):
        boot(installation, name)
    deadline = time.monotonic() + 45
    while True:
        current = probes()
        if all(value["responding"] for value in current.values()) and all(job(installation, name)["running"] for name in SERVICES):
            break
        require(time.monotonic() < deadline)
        time.sleep(1)
    installation.record("start", "running")
    return {"state": "running", "new_resources": False, "provider_health": "not_probed"}


def stop(installation):
    """Close ingress, drain bounded obligations, then stop owned compute and stores."""
    states = installation.verify()
    for name in SERVICES:
        job(installation, name)
    installation.record("stop", "closing_ingress")
    for name in ("api", "relay", "management"):
        unboot(installation, name)
    if states["postgres-1"]["Running"]:
        installation.record("stop", "draining")
        deadline = time.monotonic() + 45
        while any(pending(installation).values()):
            if time.monotonic() >= deadline:
                # Leave the worker/stores alive; restart reopens ingress for UI recovery.
                installation.record("stop", "pending_work_reopen_with_start")
                raise ValueError("private_stack_pending_work")
            time.sleep(1)
    unboot(installation, "worker")
    installation.verify()
    if states["postgres-1"]["Running"]:
        require(not any(pending(installation).values()))
    company = installation.manifest["company_id"]
    running = docker("ps", "-q", "--filter", f"label=org.ortak.company={company}",
                     "--filter", "label=org.ortak.start_key", maximum=8192)[1]
    require(not running.strip())
    installation.record("stop", "stopping_stores")
    for name in reversed(CONTAINERS):
        if states[name]["Running"]:
            docker("stop", "--time", "30", installation.manifest["containers"][name]["id"], timeout=40)
    require(not any(row["Running"] for row in installation.verify().values()))
    installation.record("stop", "stopped")
    return {"state": "stopped", "data_retained": True}
