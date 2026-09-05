"""Build only on the selected local daemon with bounded private diagnostics."""

import argparse
import json
import os
from pathlib import Path
import re
import selectors
import signal
import subprocess
import tempfile
import time

ROOT = Path(__file__).resolve().parent
DOCKER = "/usr/local/bin/docker"
HOST = "unix:///Users/nambse/.docker/run/docker.sock"
MAX_SECONDS = 1200
MAX_OUTPUT = 8 * 1024 * 1024


class BuildFailed(Exception):
    """A fixed failure code that never includes build output or environment."""


def command(target, root=ROOT):
    """Validate the explicit build target and use only its locked base digest."""
    if target not in {"runtime", "tests"}:
        raise BuildFailed("invalid_target")
    with (root / "honcho-source-lock.json").open("rb") as source:
        data = source.read(16385)
    if len(data) > 16384:
        raise BuildFailed("source_lock_too_large")
    base = json.loads(data)["candidate_build_base"]
    if not isinstance(base, str) or not re.fullmatch(r"[^\s]+@sha256:[0-9a-f]{64}", base):
        raise BuildFailed("immutable_build_base_required")
    for name in ("uv.lock", "pyproject.toml"):
        if not (root / "vendor" / name).is_file():
            raise BuildFailed("prepared_source_required")
    return [DOCKER, "--host", HOST, "buildx", "build", "--builder", "default",
            "--load", "--progress", "plain", "--target", target,
            "--build-arg", "BASE_IMAGE=" + base, "-t",
            "ortak-honcho-adapter-" + ("test" if target == "tests" else target) + ":3.1.1",
            str(root)]


def environment(directory):
    """Use fresh Docker config/home without ambient contexts or credentials."""
    home, config = directory / "home", directory / "docker"
    home.mkdir(mode=0o700)
    config.mkdir(mode=0o700)
    return {"PATH": "/usr/bin:/bin:/usr/sbin:/sbin", "LANG": "C", "LC_ALL": "C",
            "HOME": str(home), "DOCKER_CONFIG": str(config)}


def stop(process):
    """Reap the owned local process group even when its leader already exited."""
    try:
        os.killpg(process.pid, signal.SIGKILL)
    except ProcessLookupError:
        pass
    process.wait(timeout=3)
    process.stdout.close()


def run_build(args, selected_environment, output):
    """Cap combined Docker output before writing and bound the local CLI tree."""
    deadline = time.monotonic() + MAX_SECONDS
    count = 0
    process = subprocess.Popen(args, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT, env=selected_environment, start_new_session=True)
    try:
        with selectors.DefaultSelector() as ready:
            ready.register(process.stdout, selectors.EVENT_READ)
            while ready.get_map():
                remaining = deadline - time.monotonic()
                if remaining <= 0:
                    raise BuildFailed("build_deadline")
                events = ready.select(remaining)
                if not events:
                    raise BuildFailed("build_deadline")
                for key, _ in events:
                    block = os.read(key.fileobj.fileno(), 65536)
                    if not block:
                        ready.unregister(key.fileobj)
                        continue
                    count += len(block)
                    if count > MAX_OUTPUT:
                        raise BuildFailed("build_output_limit")
                    output.write(block)
                    output.flush()
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            raise BuildFailed("build_deadline")
        if process.wait(timeout=remaining) != 0:
            raise BuildFailed("build_failed")
        return count
    except subprocess.TimeoutExpired:
        raise BuildFailed("build_deadline") from None
    finally:
        stop(process)


def main(argv=None):
    """Build only when invoked; retain bounded owner-private logs for review."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("target", choices=("runtime", "tests"), nargs="?", default="runtime")
    args = command(parser.parse_args(argv).target, ROOT)
    with tempfile.TemporaryDirectory(prefix="ortak-honcho-build-config-", dir="/private/tmp") as temporary:
        selected_environment = environment(Path(temporary))
        descriptor, log = tempfile.mkstemp(prefix="ortak-honcho-build-", suffix=".log", dir="/private/tmp")
        print(json.dumps({"operation": "build_started", "daemon": HOST, "log": log}), flush=True)
        try:
            with os.fdopen(descriptor, "wb") as output:
                count = run_build(args, selected_environment, output)
            print(json.dumps({"operation": "build_completed", "output_bytes": count, "log": log}), flush=True)
        except (BuildFailed, OSError, subprocess.SubprocessError) as error:
            code = str(error) if isinstance(error, BuildFailed) else "build_process_failed"
            raise SystemExit(json.dumps({"operation": "build_failed", "code": code, "log": log})) from None


if __name__ == "__main__":
    main()
