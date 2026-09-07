"""Pinned ownership and bounded commands for the persistent private installation."""

import fcntl
import hashlib
import json
import os
from pathlib import Path
import selectors
import signal
import stat
import subprocess
import time

from init_private_stack import PROJECT, STATE_DIRECTORY
from private_native_services import private_file, selected_root

SERVICES = {"relay": "buzz-relay", "api": "ortak-server",
            "worker": "ortak-worker", "management": "ortak-management"}
CONTAINERS = ("postgres-1", "redis-1", "minio-1", "honcho-db-1", "honcho-1", "hermes")
OPTIONAL_CONTAINERS = ("semantic",)
DOCKER = Path("/Applications/Docker.app/Contents/Resources/bin/docker")


def require(condition):
    """Refuse without including rejected configuration or command output."""
    if not condition:
        raise ValueError("private_stack_selection_refused")


def command(argv, *, timeout=20, maximum=3 * 1024**2, allow_failure=False, input_bytes=None):
    """Bound output/time and contain the CLI process group, including on interruption."""
    env = {"PATH": "/usr/bin:/bin:/usr/sbin:/sbin", "HOME": str(Path.home()), "LANG": "en_US.UTF-8"}
    process = subprocess.Popen([str(x) for x in argv], env=env, start_new_session=True,
                               stdin=subprocess.PIPE if input_bytes else subprocess.DEVNULL,
                               stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
    data = bytearray()
    try:
        if input_bytes:
            require(len(input_bytes) <= 16384)
            process.stdin.write(input_bytes)
            process.stdin.close()
        deadline = time.monotonic() + timeout
        with selectors.DefaultSelector() as ready:
            ready.register(process.stdout, selectors.EVENT_READ)
            while True:
                remaining = deadline - time.monotonic()
                require(remaining > 0 and ready.select(remaining))
                part = os.read(process.stdout.fileno(), min(65536, maximum + 1 - len(data)))
                if not part:
                    break
                data.extend(part)
                require(len(data) <= maximum)
        code = process.wait(timeout=max(0.001, deadline - time.monotonic()))
        require(allow_failure or code == 0)
        return code, data.decode("utf-8")
    finally:
        # Kill surviving descendants even when the direct child already exited.
        try:
            os.killpg(process.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
        process.wait(timeout=3)
        process.stdout.close()


def docker(*arguments, **options):
    """Use only this user's Docker Desktop socket, never ambient remote contexts."""
    return command([DOCKER, "--host", f"unix://{Path.home()}/.docker/run/docker.sock", *arguments], **options)


def fingerprint(path):
    """Hash a bounded owned regular file without following aliases."""
    require(path == path.resolve() and path.is_absolute())
    with os.fdopen(os.open(path, os.O_RDONLY | os.O_NOFOLLOW), "rb") as stream:
        row = os.fstat(stream.fileno())
        require(stat.S_ISREG(row.st_mode) and row.st_uid == os.getuid() and row.st_size <= 512 * 1024**2)
        digest = hashlib.file_digest(stream, "sha256").hexdigest()
    return digest


def save_json(path, value):
    """Atomically persist a private checkpoint and sync its directory."""
    require(not path.is_symlink())
    temporary = path.with_name(path.name + ".next")
    fd = os.open(temporary, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600)
    with os.fdopen(fd, "w") as stream:
        json.dump(value, stream, indent=2)
        stream.write("\n")
        stream.flush()
        os.fsync(stream.fileno())
    os.replace(temporary, path)
    fd = os.open(path.parent, os.O_RDONLY)
    try:
        os.fsync(fd)
    finally:
        os.close(fd)


def container_contract(row):
    """Persist only identity and security/topology metadata, never environment."""
    result = {"id": row["Id"], "name": row["Name"], "image": row["Image"],
            "labels": row["Config"].get("Labels", {}),
            "mounts": sorted([{k: m.get(k) for k in ("Type", "Name", "Source", "Destination", "RW")}
                              for m in row["Mounts"]], key=lambda mount: mount["Destination"]),
            "ports": row["HostConfig"]["PortBindings"],
            "restart": row["HostConfig"]["RestartPolicy"],
            "network_names": sorted(row["NetworkSettings"]["Networks"])}
    if row["Config"].get("Labels", {}).get("org.ortak.role") == "semantic-scorer":
        result["scorer_contract"] = {
            "user": row["Config"]["User"], "entrypoint": row["Config"]["Entrypoint"],
            "cmd": row["Config"]["Cmd"], "stop_timeout": row["Config"].get("StopTimeout"),
            **{key: row["HostConfig"].get(key) for key in
               ("ReadonlyRootfs", "Init", "CapDrop", "SecurityOpt", "PidsLimit", "Memory", "NanoCpus", "Tmpfs", "NetworkMode")}}
    return result


def inspect_container(name):
    """Read exactly one container; callers validate before any mutation."""
    rows = json.loads(docker("inspect", name)[1])
    require(len(rows) == 1)
    return rows[0]


class Installation:
    """Hold a nonblocking, per-installation operation lock across all mutations."""

    def __init__(self):
        self.root = selected_root(STATE_DIRECTORY)
        self.directory = self.root / "lifecycle"
        self.directory.mkdir(mode=0o700, exist_ok=True)
        require(self.directory.resolve() == self.directory and not self.directory.stat().st_mode & 0o077)
        self.lock = os.open(self.directory / "operation.lock", os.O_RDWR | os.O_CREAT | os.O_NOFOLLOW, 0o600)
        try:
            fcntl.flock(self.lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BaseException:
            os.close(self.lock)
            raise

    def close(self):
        """Release the operation lock after a durable outcome or propagated failure."""
        os.close(self.lock)

    def load(self):
        """Load the selected immutable installation contract."""
        self.manifest = json.loads(private_file(self.directory / "installation.json", 131072))
        require(self.manifest["format"] == "ortak-private-installation/1"
                and self.manifest["state_directory"] == str(self.root)
                and set(CONTAINERS) <= set(self.manifest["containers"]) <= set(CONTAINERS + OPTIONAL_CONTAINERS)
                and set(self.manifest["services"]) == set(SERVICES))
        return self.manifest

    def verify(self):
        """Compare all owned resources before starting or stopping any of them."""
        states = {}
        for name, expected in self.manifest["containers"].items():
            row = inspect_container(expected["id"])
            normalized = {**expected, "mounts": sorted(expected["mounts"], key=lambda mount: mount["Destination"])}
            require(container_contract(row) == normalized)
            states[name] = row["State"]
        for selected in self.manifest["services"].values():
            require(fingerprint(Path(selected["path"])) == selected["sha256"])
        for relative, digest in self.manifest["launcher_files"].items():
            path = self.directory / relative
            require(path.is_relative_to(self.directory) and ".." not in Path(relative).parts)
            require(fingerprint(path) == digest)
        return states

    def record(self, action, phase):
        """A failed/interrupted action leaves a durable explicit recovery checkpoint."""
        save_json(self.directory / "operation.json", {"action": action, "phase": phase, "unix_time": time.time()})
