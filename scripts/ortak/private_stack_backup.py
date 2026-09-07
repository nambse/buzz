#!/usr/bin/env python3
"""Capture the registered schema80 installation into one retained cold backup."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import plistlib
import shutil
import sys
import time
from uuid import uuid4

from backup_private_database import Commands, digest, private_binary
from private_native_services import private_file
from private_stack_archive import capture
from private_stack_database import metadata
from private_stack_services import pending, start, stop
from private_stack_state import DOCKER, Installation, command, docker, fingerprint, inspect_container, require, save_json
import recovery_archive_io

VOLUMES = {"main": "postgres-1", "redis": "redis-1", "minio": "minio-1",
           "honcho": "honcho-db-1", "journal": "hermes"}
LIMIT = 2 * 1024**3
PRIVATE = (".ortak-private-stack.json", "compose.env", "runtime.env", "identities.json",
           "api-config.json", "worker-config.json", "prepared-catalog.json", "employees",
           "hermes", "honcho", "memory", "object-store", "secrets", "workspaces", "lifecycle", "bin")


class BackupCommands(Commands):
    """Reuse the proven streaming limits with this user's selected Docker socket."""

    def __init__(self, root):
        super().__init__(root)
        self.deadline = time.monotonic() + 600

    def docker(self, *args):
        return [str(DOCKER), "--host", f"unix://{Path.home()}/.docker/run/docker.sock", *args]


def describe(path, maximum):
    """Verify an inert archive and persist only counts and digests."""
    with path.open("rb") as incoming:
        summary = recovery_archive_io.archive(incoming, maximum)
    return {"path": path.name, "bytes": path.stat().st_size, "sha256": digest(path), "tree": summary}


def cold(installation):
    """No source container, native service or volume writer may remain active."""
    from private_stack_services import job
    require(not any(row["Running"] for row in installation.verify().values()))
    require(not any(job(installation, action)["loaded"] for action in installation.manifest["services"]))
    for suffix in VOLUMES.values():
        row = installation.manifest["containers"][suffix]
        volume = next(m["Name"] for m in row["mounts"] if m["Type"] == "volume")
        require(not docker("ps", "-q", "--filter", f"volume={volume}", maximum=8192)[1].strip())


def native_selection(bundle):
    """Select only the private bundle; refuse capture while any process maps its binary."""
    require(bundle.is_absolute() and bundle.resolve() == bundle)
    info = plistlib.loads((bundle / "Contents/Info.plist").read_bytes())
    require(info.get("CFBundleIdentifier") == "dev.ortak.private20260905")
    binary = bundle / "Contents/MacOS/buzz-desktop"
    require(binary.is_file())
    code, output = command(["/usr/sbin/lsof", "-t", str(binary)], maximum=8192, allow_failure=True)
    require(code == 1 and not output.strip())
    return {"bundle": str(bundle), "binary_sha256": fingerprint(binary),
            "app_data": str(Path.home() / "Library/Application Support/dev.ortak.private20260905")}


def freeze_helpers(output):
    """Freeze just the inert standard-library archive reader/writer programs."""
    root = output / "helpers"
    root.mkdir(mode=0o700)
    for name in ("private_stack_archive.py", "recovery_archive_io.py"):
        source = Path(__file__).resolve().with_name(name)
        destination = root / name
        with private_binary(destination) as stream:
            stream.write(source.read_bytes())
    return root


def capture_volume(installation, output, helpers, kind, commands):
    """Create one inert reader, verify its ownership, then stream the cold volume."""
    cold(installation)
    source = installation.manifest["containers"][VOLUMES[kind]]
    volume = next(m["Name"] for m in source["mounts"] if m["Type"] == "volume")
    image = installation.manifest["containers"]["hermes"]["image"]
    name = f"ortak-backup80-{output.name}-{kind}"
    save_json(output / f"{kind}-reader-intent.json", {"name": name, "source_volume": volume,
              "image": image, "network": "none", "source_read_only": True})
    identifier = docker("create", "--pull", "never", "--name", name,
                        "--label", f"org.ortak.backup80={output.name}", "--network", "none", "--read-only",
                        "--user", "0:0", "--cap-drop", "ALL", "--cap-add", "DAC_OVERRIDE",
                        "--security-opt", "no-new-privileges", "--pids-limit", "16", "--memory", "256m",
                        "--mount", f"type=volume,source={volume},target=/capture-source,readonly,volume-nocopy",
                        "--mount", f"type=bind,source={helpers},target=/restore-code,readonly",
                        "--entrypoint", "/usr/local/bin/python", image,
                        "/restore-code/private_stack_archive.py", str(LIMIT))[1].strip()
    row = inspect_container(identifier)
    require(row["Name"] == "/" + name and row["Image"] == image
            and row["Config"]["Labels"].get("org.ortak.backup80") == output.name
            and row["HostConfig"]["NetworkMode"] == "none" and not row["HostConfig"]["PortBindings"]
            and all(not m["RW"] for m in row["Mounts"]))
    save_json(output / f"{kind}-reader.json", {"id": identifier, "name": name, "source_volume": volume})
    archive = output / f"{kind}.tar"
    try:
        commands.run(kind + "-capture", commands.docker("start", "-a", identifier), output=archive,
                     ceiling=LIMIT + 128 * 1024**2)
    finally:
        current = inspect_container(identifier)
        require(current["Id"] == identifier and current["Config"]["Labels"].get("org.ortak.backup80") == output.name)
        if current["State"]["Running"]:
            docker("stop", "--time", "1", identifier, timeout=5)
    cold(installation)
    return {**describe(archive, LIMIT), "source_volume": volume, "retained_reader": identifier}


def host_archives(installation, output, native):
    """Copy explicit configuration, app data and artifacts, excluding caches and prior backups."""
    root = installation.root
    entries = [(root / relative, relative) for relative in PRIVATE]
    entries.append((Path(native["app_data"]), "native-app-data"))
    config = output / "private-state.tar"
    with private_binary(config) as stream:
        capture(stream, entries, 128 * 1024**2, linux_metadata=False)
        stream.flush()
        os.fsync(stream.fileno())
    artifacts = json.loads(private_file(root / "artifacts/active-services.json"))
    selected = [(Path(value["path"]), str(Path(value["path"]).relative_to(root))) for value in artifacts.values()]
    selected += [(root / "artifacts/active-services.json", "artifacts/active-services.json"),
                 (Path(native["bundle"]), "desktop/Ortak Private.app")]
    package = output / "native-artifacts.tar"
    with private_binary(package) as stream:
        capture(stream, selected, 512 * 1024**2, linux_metadata=False)
        stream.flush()
        os.fsync(stream.fileno())
    return {"private_state": describe(config, 128 * 1024**2),
            "native_artifacts": describe(package, 512 * 1024**2)}


def backup(installation, bundle):
    """Capture one new operation, preserving failed outputs and an explicit source restart path."""
    installation.verify()
    native = native_selection(bundle)
    require(not any(pending(installation).values()))
    require(shutil.disk_usage(installation.root).free >= 12 * 1024**3)
    parent = installation.root / "backups"
    require(parent.resolve() == parent and parent.stat().st_uid == os.getuid() and not parent.stat().st_mode & 0o077)
    output = parent / uuid4().hex
    output.mkdir(mode=0o700)
    helpers = freeze_helpers(output)
    receipt = {"format": "ortak-private-backup80/1", "phase": "prepared", "schema": 80,
               "installation": installation.manifest, "native": native, "components": {}}
    save_json(output / "receipt.json", receipt)
    commands = BackupCommands(output)

    def before_stores():
        receipt["databases"] = {kind: metadata(commands, installation.manifest["containers"][VOLUMES[kind]]["id"], kind, kind)
                                for kind in ("main", "honcho")}
        save_json(output / "receipt.json", receipt)

    try:
        stop(installation, before_stores=before_stores)
        cold(installation)
        receipt["phase"] = "capturing"
        save_json(output / "receipt.json", receipt)
        for kind in VOLUMES:
            receipt["components"][kind] = capture_volume(installation, output, helpers, kind, commands)
            save_json(output / "receipt.json", receipt)
        receipt["components"].update(host_archives(installation, output, native))
        images = sorted({row["image"] for row in installation.manifest["containers"].values()}
                        | {installation.manifest["containers"]["hermes"]["labels"]["org.ortak.worker.image"]})
        exported = output / "images.tar.gz"
        compressed = commands.run("images-export", commands.docker("image", "save", *images),
                                  output=exported, ceiling=8 * 1024**3, gzip_output=True, output_ceiling=4 * 1024**3)
        receipt["components"]["images"] = {"path": exported.name, "sha256": digest(exported), "images": images, **compressed}
        cold(installation)
        receipt.update(phase="captured_not_restored", restore_verified=False)
        save_json(output / "receipt.json", receipt)
    finally:
        start(installation)
    return {"backup": str(output), "phase": receipt["phase"], "restore_verified": False}


def main():
    """The private application must already be closed by its normal Quit action."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--native-bundle", type=Path, required=True)
    args = parser.parse_args()
    os.umask(0o077)
    installation = Installation()
    try:
        installation.load()
        print(json.dumps(backup(installation, args.native_bundle), indent=2))
    finally:
        installation.close()


if __name__ == "__main__":
    try:
        main()
    except Exception:
        raise SystemExit("Yedek tamamlanmadı. Uygulamayı kapatın; kaynak için ortak status/start ve yedek receipt.json kaydını inceleyin.") from None
