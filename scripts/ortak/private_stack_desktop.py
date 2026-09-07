"""Freeze and open the selected private application with its existing operator identity."""

import hashlib
import json
import os
from pathlib import Path
import plistlib
import shutil
import stat
import subprocess
import time

from private_native_services import environment, private_file
from private_stack_state import command, fingerprint, require, save_json

IDENTIFIER = "dev.ortak.private20260905"
BINARY = "Contents/MacOS/buzz-desktop"


def tree(bundle):
    """Bound and hash every owned regular bundle file; refuse aliases and special files."""
    require(bundle.is_absolute() and bundle.resolve() == bundle)
    files, total = {}, 0
    pending = [bundle]
    while pending:
        path = pending.pop()
        metadata = path.lstat()
        require(metadata.st_uid == os.getuid() and not stat.S_ISLNK(metadata.st_mode))
        if stat.S_ISDIR(metadata.st_mode):
            children = list(path.iterdir())
            require(len(children) + len(pending) + len(files) <= 1024)
            pending.extend(children)
            continue
        require(stat.S_ISREG(metadata.st_mode) and metadata.st_nlink == 1)
        total += metadata.st_size
        require(total <= 512 * 1024**2)
        files[str(path.relative_to(bundle))] = fingerprint(path)
    info = plistlib.loads((bundle / "Contents/Info.plist").read_bytes())
    require(info.get("CFBundleIdentifier") == IDENTIFIER and BINARY in files)
    return dict(sorted(files.items()))


def running():
    """Discover only private bundle executables; never inspect process environments."""
    code, output = command(["/usr/bin/pgrep", "-x", "buzz-desktop"], maximum=8192, allow_failure=True)
    require(code in (0, 1))
    pids = output.split()
    require(len(pids) <= 16 and all(pid.isdecimal() for pid in pids))
    found = []
    for pid in pids:
        code, mappings = command(["/usr/sbin/lsof", "-a", "-p", pid, "-d", "txt", "-Fn"],
                                 maximum=65536, allow_failure=True)
        require(code in (0, 1))
        for line in mappings.splitlines():
            if not line.startswith("n/") or not line.endswith("/" + BINARY):
                continue
            bundle = Path(line[1:]).parents[2]
            info = plistlib.loads((bundle / "Contents/Info.plist").read_bytes())
            if info.get("CFBundleIdentifier") == IDENTIFIER:
                found.append({"pid": int(pid), "bundle": str(bundle)})
    return found


def install_app(installation, bundle):
    """Retain a content-addressed copy without replacing app data or previous bundles."""
    installation.verify()
    require(not running())
    files = tree(bundle)
    digest = hashlib.sha256(json.dumps(files, sort_keys=True).encode()).hexdigest()
    parent = installation.root / "desktop"
    parent.mkdir(mode=0o700, exist_ok=True)
    require(parent.resolve() == parent and not parent.stat().st_mode & 0o077)
    target = parent / digest / "Ortak Private.app"
    installation.record("install-app", "copying_private_bundle")
    if not target.exists():
        target.parent.mkdir(mode=0o700, exist_ok=True)
        require(target.parent.resolve() == target.parent)
        # An interrupted copy is retained as staging; a retry never selects it.
        staging = target.with_name("Ortak Private.app.staging")
        require(not staging.exists())
        shutil.copytree(bundle, staging)
        require(tree(staging) == files and tree(bundle) == files)
        os.replace(staging, target)
    require(tree(target) == files)
    selected = {"format": "ortak-private-desktop/1", "bundle": str(target),
                "files": files, "source_bundle": str(bundle)}
    path = installation.directory / "desktop.json"
    if path.exists():
        save_json(installation.directory / "desktop.previous.json", json.loads(private_file(path)))
    save_json(path, selected)
    installation.record("install-app", "selected")
    return {"application": "installed", "bundle": str(target), "identity": "preserved"}


def open_app(installation):
    """Open one verified artifact using private env, with no key in argv or logs."""
    from private_stack_services import start
    selected = json.loads(private_file(installation.directory / "desktop.json"))
    require(selected["format"] == "ortak-private-desktop/1")
    bundle = Path(selected["bundle"])
    require(bundle.is_relative_to(installation.root / "desktop") and tree(bundle) == selected["files"])
    current = running()
    require(len(current) <= 1 and all(row["bundle"] == str(bundle) for row in current))
    receipt = installation.directory / "desktop-launch.json"
    if current:
        saved = json.loads(private_file(receipt))
        birth = command(["/bin/ps", "-p", str(current[0]["pid"]), "-o", "lstart="], maximum=1024)[1].strip()
        require(saved == {**current[0], "started_at": birth} and bool(birth))
    start(installation)
    if current:
        return {"application": "already_open", "pid": current[0]["pid"]}
    env = environment(installation.root)
    env["HOME"] = str(Path.home())
    api = json.loads(private_file(installation.root / "api-config.json"))
    env["ORTAK_ENCRYPTED_DM_API_BINDINGS"] = json.dumps({"http://localhost:3038": api["origin"]})
    installation.record("open", "launching_private_application")
    process = subprocess.Popen([str(bundle / BINARY)], cwd=installation.root, env=env,
                               stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
                               stderr=subprocess.DEVNULL, start_new_session=True)
    birth = command(["/bin/ps", "-p", str(process.pid), "-o", "lstart="], maximum=1024)[1].strip()
    require(bool(birth))
    save_json(receipt, {"pid": process.pid, "bundle": str(bundle), "started_at": birth})
    deadline = time.monotonic() + 5
    while True:
        require(process.poll() is None)
        if {"pid": process.pid, "bundle": str(bundle)} in running():
            break
        require(time.monotonic() < deadline)
        time.sleep(0.1)
    installation.record("open", "application_started")
    return {"application": "started", "pid": process.pid, "identity": "selected_private_operator"}
