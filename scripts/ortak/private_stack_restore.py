#!/usr/bin/env python3
"""Verify a recovery80 backup in fresh offline destinations; never replace the source."""

import argparse
import json
import os
from pathlib import Path
import re
import time
from uuid import uuid4

from backup_private_database import digest, private_binary
from private_native_services import private_file
from private_stack_backup import BackupCommands, LIMIT, VOLUMES
from private_stack_database import DATABASES, metadata
from private_stack_state import Installation, docker, inspect_container, require, save_json
from recovery_image_export import verify_gzip
import recovery_archive_io

LABEL = "org.ortak.restore80"


def selected_backup(root, backup):
    """Accept one complete owner-private backup with closed component paths and exact hashes."""
    require(backup.resolve() == backup and backup.parent == root / "backups" and re.fullmatch(r"[0-9a-f]{32}", backup.name))
    receipt = json.loads(private_file(backup / "receipt.json", 512 * 1024))
    require(receipt["format"] == "ortak-private-backup80/1" and receipt["schema"] == 80
            and receipt["phase"] == "captured_not_restored"
            and set(receipt["components"]) == set(VOLUMES) | {"private_state", "native_artifacts", "images"})
    images = receipt["components"]["images"]["images"]
    require(all(row["image"] in images for row in receipt["installation"]["containers"].values()))
    for kind, component in receipt["components"].items():
        filename = {"private_state": "private-state.tar", "native_artifacts": "native-artifacts.tar",
                    "images": "images.tar.gz"}.get(kind, kind + ".tar")
        path = backup / filename
        require(component["path"] == filename and path.resolve() == path and path.is_file()
                and path.stat().st_uid == os.getuid() and path.stat().st_mode & 0o777 == 0o600
                and path.stat().st_size == component["bytes"] and digest(path) == component["sha256"])
    return receipt


def restore_volume(backup, receipt, output, kind, commands):
    """Only a newly created labeled empty volume receives archive extraction."""
    name = f"ortak_restore80_{output.name}_{kind}"
    require(not docker("volume", "ls", "--filter", f"name=^{name}$", "--format", "{{.Name}}")[1].strip())
    save_json(output / f"{kind}-volume-intent.json", {"name": name, "kind": kind, "operation": output.name})
    require(docker("volume", "create", "--label", f"{LABEL}={output.name}", "--label", f"org.ortak.store={kind}", name)[1].strip() == name)
    volume = json.loads(docker("volume", "inspect", name)[1])[0]
    require(volume["Labels"] == {LABEL: output.name, "org.ortak.store": kind} and volume["Driver"] == "local")
    helper = output / "helpers"
    helper.mkdir(mode=0o700, exist_ok=True)
    code = helper / "recovery_archive_io.py"
    if not code.exists():
        with private_binary(code) as stream:
            stream.write(Path(recovery_archive_io.__file__).read_bytes())
    image = receipt["installation"]["containers"]["hermes"]["image"]
    helper_name = f"ortak-restore80-{output.name}-{kind}"
    identifier = docker("create", "-i", "--pull", "never", "--name", helper_name,
                        "--label", f"{LABEL}={output.name}", "--network", "none", "--read-only",
                        "--user", "0:0", "--cap-drop", "ALL", "--cap-add", "DAC_OVERRIDE", "--cap-add", "CHOWN", "--cap-add", "FOWNER",
                        "--security-opt", "no-new-privileges", "--pids-limit", "16", "--memory", "256m",
                        "--mount", f"type=volume,source={name},target=/restore-target,volume-nocopy",
                        "--mount", f"type=bind,source={helper},target=/restore-code,readonly",
                        "--entrypoint", "/usr/local/bin/python", image, "/restore-code/recovery_archive_io.py", str(LIMIT))[1].strip()
    row = inspect_container(identifier)
    require(row["Name"] == "/" + helper_name and row["Image"] == image
            and row["Config"]["Labels"].get(LABEL) == output.name
            and row["HostConfig"]["NetworkMode"] == "none" and not row["HostConfig"]["PortBindings"]
            and [(m["Name"], m["Destination"], m["RW"]) for m in row["Mounts"] if m["Type"] == "volume"]
            == [(name, "/restore-target", True)])
    save_json(output / f"{kind}-extractor.json", {"id": identifier, "volume": name, "name": helper_name})
    try:
        raw = commands.run(kind + "-restore", commands.docker("start", "-a", "-i", identifier), archive=backup / f"{kind}.tar")
        result = json.loads(raw)
        require(result.pop("status") == "verified" and result == receipt["components"][kind]["tree"])
    finally:
        current = inspect_container(identifier)
        require(current["Id"] == identifier and current["Config"]["Labels"].get(LABEL) == output.name)
        if current["State"]["Running"]:
            docker("stop", "--time", "1", identifier, timeout=5)
    return {"volume": name, "extractor": identifier, "tree": result}


def verify_database(receipt, output, kind, restored, commands):
    """Boot one restored PostgreSQL without any source mount, host port or network."""
    image = receipt["installation"]["containers"][VOLUMES[kind]]["image"]
    name = f"ortak-restore80-db-{output.name}-{kind}"
    identifier = docker("run", "-d", "--pull", "never", "--name", name,
                        "--label", f"{LABEL}={output.name}", "--network", "none", "--read-only", "--cap-drop", "ALL",
                        "--cap-add", "CHOWN", "--cap-add", "DAC_OVERRIDE", "--cap-add", "FOWNER", "--cap-add", "SETUID", "--cap-add", "SETGID",
                        "--security-opt", "no-new-privileges", "--memory", "512m", "--cpus", "0.5", "--pids-limit", "64",
                        "--tmpfs", "/tmp:rw,noexec,nosuid,nodev,size=64m", "--tmpfs", "/var/run/postgresql:rw,noexec,nosuid,nodev,size=16m",
                        "--mount", f"type=volume,source={restored['volume']},target=/var/lib/postgresql/data,volume-nocopy",
                        image)[1].strip()
    row = inspect_container(identifier)
    require(row["Name"] == "/" + name and row["Image"] == image
            and row["Config"]["Labels"].get(LABEL) == output.name
            and row["HostConfig"]["NetworkMode"] == "none" and not row["HostConfig"]["PortBindings"]
            and len(row["Mounts"]) == 1 and row["Mounts"][0]["Name"] == restored["volume"])
    save_json(output / f"{kind}-database.json", {"id": identifier, "name": name, "volume": restored["volume"], "network": "none"})
    try:
        role, database = DATABASES[kind]
        deadline = time.monotonic() + 40
        while True:
            code, _ = docker("exec", identifier, "pg_isready", "-q", "-U", role, "-d", database, allow_failure=True)
            if code == 0:
                break
            require(time.monotonic() < deadline)
            time.sleep(0.5)
        observed = metadata(commands, identifier, kind, "restored-" + kind)
        require(observed == receipt["databases"][kind])
    finally:
        current = inspect_container(identifier)
        require(current["Id"] == identifier and current["Config"]["Labels"].get(LABEL) == output.name)
        if current["State"]["Running"]:
            docker("stop", "--time", "20", identifier, timeout=30)
    return {"container": identifier, "metadata_equal": True, "tables": len(observed["tables"]), "retained_stopped": True}


def verify_journal(receipt, output, restored, commands):
    """Check the restored journal through an inert read-only helper, never start a controller."""
    volume = json.loads(docker("volume", "inspect", restored["volume"])[1])[0]
    require(volume["Labels"] == {LABEL: output.name, "org.ortak.store": "journal"})
    source = Path(__file__).resolve().with_name("private_stack_journal.py")
    helper = output / ("helpers/private_stack_journal-" + digest(source)[:16] + ".py")
    with private_binary(helper) as stream:
        stream.write(source.read_bytes())
    image = receipt["installation"]["containers"]["hermes"]["image"]
    name = f"ortak-journal80-{output.name}-{digest(source)[:8]}"
    save_json(output / ("journal-reader-" + digest(source)[:8] + ".json"), {"name": name, "volume": restored["volume"], "image": image})
    identifier = docker("create", "--name", name, "--pull", "never",
                       "--label", f"{LABEL}={output.name}", "--network", "none", "--read-only", "--user", "0:0",
                       "--cap-drop", "ALL", "--cap-add", "DAC_OVERRIDE", "--security-opt", "no-new-privileges",
                       "--pids-limit", "16", "--memory", "384m",
                       "--tmpfs", "/tmp:rw,noexec,nosuid,nodev,size=256m",
                       "--mount", f"type=volume,source={restored['volume']},target=/journal,readonly,volume-nocopy",
                       "--mount", f"type=bind,source={helper},target=/inspect.py,readonly",
                       "--entrypoint", "/usr/local/bin/python", image, "/inspect.py")[1].strip()
    row = inspect_container(identifier)
    require(row["Name"] == "/" + name and row["Image"] == image and row["Config"]["Labels"].get(LABEL) == output.name
            and row["HostConfig"]["NetworkMode"] == "none" and all(not mount["RW"] for mount in row["Mounts"]))
    try:
        raw = commands.run("journal-integrity-" + digest(source)[:16], commands.docker("start", "-a", identifier), ceiling=8192)
    finally:
        row = inspect_container(identifier)
        require(row["Id"] == identifier and row["Config"]["Labels"].get(LABEL) == output.name)
        if row["State"]["Running"]:
            docker("stop", "--time", "1", identifier, timeout=5)
    observed = json.loads(raw)
    require(observed["integrity"] == "ok" and observed["foreign_keys"] == "ok")
    return observed


def verify(installation, backup):
    """Restore to fresh retained targets; the active source keeps serving independently."""
    receipt = selected_backup(installation.root, backup)
    parent = installation.root / "recovery-verifications"
    parent.mkdir(mode=0o700, exist_ok=True)
    require(parent.resolve() == parent)
    output = parent / uuid4().hex
    output.mkdir(mode=0o700)
    result = {"format": "ortak-private-restore80/1", "backup": str(backup), "phase": "started", "components": {}}
    save_json(output / "receipt.json", result)
    commands = BackupCommands(output)
    image = receipt["components"]["images"]
    selected = {**image, "format": "ortak-private-image-export/1", "compression": "gzip", "output_limit": 4 * 1024**3}
    result["images"] = verify_gzip(backup / "images.tar.gz", selected, 8 * 1024**3)
    commands.run("image-load", commands.docker("image", "load"), archive=backup / "images.tar.gz", ceiling=65536)
    for image_id in image["images"]:
        require(docker("image", "inspect", "--format", "{{.Id}}", image_id)[1].strip() == image_id)
    for kind in VOLUMES:
        restored = restore_volume(backup, receipt, output, kind, commands)
        result["components"][kind] = restored
        if kind in DATABASES:
            restored["database"] = verify_database(receipt, output, kind, restored, commands)
        if kind == "journal":
            restored["integrity"] = verify_journal(receipt, output, restored, commands)
        save_json(output / "receipt.json", result)
    for kind in ("private_state", "native_artifacts"):
        destination = output / kind
        destination.mkdir(mode=0o700)
        component = receipt["components"][kind]
        maximum = 128 * 1024**2 if kind == "private_state" else 512 * 1024**2
        with (backup / component["path"]).open("rb") as stream:
            restored = recovery_archive_io.archive(stream, maximum, destination)
        require(restored == component["tree"])
        result["components"][kind] = {"destination": str(destination), "tree": restored, "activated": False}
    result.update(phase="verified_offline_stores", source_replaced=False, runtime_activated=False)
    save_json(output / "receipt.json", result)
    save_json(backup / "restore-verification.json", {"receipt": str(output / "receipt.json"), "sha256": digest(output / "receipt.json")})
    return {"verification": str(output), "phase": result["phase"], "source_replaced": False}


def main():
    """Keep the same operation lock as source lifecycle actions during verification."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--backup", type=Path, required=True)
    args = parser.parse_args()
    os.umask(0o077)
    installation = Installation()
    try:
        installation.load()
        print(json.dumps(verify(installation, args.backup), indent=2))
    finally:
        installation.close()


if __name__ == "__main__":
    try:
        main()
    except Exception:
        raise SystemExit("Geri yükleme doğrulanmadı. Kaynak değiştirilmedi; ayrı doğrulama hedefleri ve receipt.json korundu.") from None
