"""Register the already-authorized fresh installation without creating resources."""

import hashlib
import json
import os
from pathlib import Path
import plistlib
import shlex
import sys

from init_private_stack import create_file
from private_native_services import private_file
from private_stack_state import CONTAINERS, PROJECT, SERVICES, container_contract, fingerprint, inspect_container, require, save_json

MODULES = ("private_stack.py", "private_stack_state.py", "private_stack_services.py",
           "private_stack_install.py", "private_stack_launch.py", "private_native_services.py", "init_private_stack.py",
           "private_stack_backup.py", "private_stack_restore.py", "private_stack_archive.py", "private_stack_database.py",
           "private_stack_journal.py", "backup_private_database.py", "recovery_archive_io.py", "recovery_image_export.py",
           "private_stack_desktop.py")


def plist_path(action):
    """Use only this user's four explicitly selected service labels."""
    require(action in SERVICES)
    return Path.home() / "Library/LaunchAgents" / f"dev.ortak.private-v0.{action}.plist"


def register(installation, *, upgrade=False):
    """Freeze current resources and launcher code; service replacement happens on restart."""
    root, directory = installation.root, installation.directory
    target = directory / "installation.json"
    if target.exists():
        installation.load()
        installation.verify()
        if upgrade:
            return freeze_launcher(installation, installation.manifest)
        return {"installation": "already_registered", "resource_creation": False}
    require(not upgrade)
    artifacts = json.loads(private_file(root / "artifacts/active-services.json"))
    hermes = json.loads(private_file(root / "hermes/controller/selection.json"))
    contracts = {}
    for suffix in CONTAINERS:
        row = inspect_container(f"{PROJECT}-{suffix}")
        labels = row["Config"].get("Labels", {})
        require(row["Name"] == f"/{PROJECT}-{suffix}")
        if suffix == "hermes":
            require(labels.get("org.ortak.company") == hermes["company_id"]
                    and labels.get("org.ortak.journal_owner") == hermes["journal_owner"]
                    and row["Image"] == hermes["controller_image"])
        else:
            require(labels.get("com.docker.compose.project") == PROJECT
                    and labels.get("com.docker.compose.service") == suffix.removesuffix("-1"))
        for bindings in row["HostConfig"]["PortBindings"].values():
            require(all(binding["HostIp"] == "127.0.0.1" for binding in bindings))
        require(row["HostConfig"]["RestartPolicy"]["Name"] == "unless-stopped")
        contracts[suffix] = container_contract(row)
    services = {}
    previous_plists = {}
    for action, name in SERVICES.items():
        selected = artifacts[name]
        require(Path(selected["path"]).is_relative_to(root / "artifacts")
                and fingerprint(Path(selected["path"])) == selected["sha256"])
        services[action] = {"path": selected["path"], "sha256": selected["sha256"]}
        previous = plistlib.loads(private_file(plist_path(action)).encode())
        require(previous["Label"] == f"dev.ortak.private-v0.{action}"
                and previous["ProgramArguments"] == [sys.executable, str(root / "run-service.py"), action]
                and previous["WorkingDirectory"] == str(root))
        previous_plists[action] = previous
    manifest = {"format": "ortak-private-installation/1", "state_directory": str(root),
                "schema_version": 80, "company_id": hermes["company_id"], "containers": contracts,
                "services": services, "previous_plists": previous_plists}
    return freeze_launcher(installation, manifest)


def freeze_launcher(installation, manifest):
    """Upgrade launcher code only; retain resource selections and prior loaded job authority."""
    root, directory = installation.root, installation.directory
    if "plists" in manifest:
        from private_stack_services import job
        previous = {}
        for action in SERVICES:
            job(installation, action)
            previous[action] = plistlib.loads(private_file(plist_path(action)).encode())
        manifest = {**manifest, "previous_plists": previous}
    source = Path(__file__).resolve().parent
    contents = {name: (source / name).read_bytes() for name in MODULES}
    revision = hashlib.sha256(b"".join(contents.values())).hexdigest()
    code = directory / f"code-{revision[:16]}"
    code.mkdir(mode=0o700, exist_ok=True)
    require(code.resolve() == code)
    for name, content in contents.items():
        path = code / name
        if not path.exists():
            create_file(path, content.decode(), 0o500)
        require(path.read_bytes() == content and not path.is_symlink())
    launchers = {str(path.relative_to(directory)): fingerprint(path) for path in sorted(code.glob("*.py"))}
    plists = {}
    for action in SERVICES:
        plists[action] = {"Label": f"dev.ortak.private-v0.{action}", "KeepAlive": True,
                          "RunAtLoad": True, "ProcessType": "Background", "ThrottleInterval": 30,
                          "Umask": 63, "ExitTimeOut": 50, "WorkingDirectory": str(root),
                          "ProgramArguments": [sys.executable, str(code / "private_stack_launch.py"), action],
                          "StandardOutPath": "/dev/null", "StandardErrorPath": "/dev/null"}
    updated = {**manifest, "launcher_files": launchers, "launcher_revision": revision, "plists": plists}
    wrapper_dir = root / "bin"
    wrapper_dir.mkdir(mode=0o700, exist_ok=True)
    require(wrapper_dir.resolve() == wrapper_dir)
    wrapper = "#!/bin/sh\nexec " + shlex.quote(sys.executable) + " " + shlex.quote(str(code / "private_stack.py")) + ' "$@"\n'
    wrapper_path = wrapper_dir / "ortak"
    require(not wrapper_path.is_symlink())
    if wrapper_path.exists():
        expected = manifest.get("wrapper_sha256")
        if expected is not None:
            require(fingerprint(wrapper_path) == expected)
        else:
            # Compatibility for the first registered, pre-activation bootstrap.
            previous_code = directory / f"code-{manifest['launcher_revision'][:16]}"
            old = "#!/bin/sh\nexec " + shlex.quote(sys.executable) + " " + shlex.quote(str(previous_code / "private_stack.py")) + ' "$@"\n'
            require(wrapper_path.read_text() == old)
    updated["wrapper_sha256"] = hashlib.sha256(wrapper.encode()).hexdigest()
    save_json(directory / "installation.previous.json", manifest)
    # Write the wrapper first: a previous installed entry point still loads and
    # verifies the selected manifest, and the new code does the same on retry.
    staged = wrapper_path.with_name("ortak.next")
    create_file(staged, wrapper, 0o700)
    os.replace(staged, wrapper_path)
    save_json(directory / "installation.json", updated)
    installation.manifest = updated
    installation.record("install", "registered_restart_required")
    return {"installation": "registered", "command": str(wrapper_dir / "ortak"), "resource_creation": False}
