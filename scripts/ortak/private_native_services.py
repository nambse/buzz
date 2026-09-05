#!/usr/bin/env python3
"""Run the isolated local Ortak services with fresh, explicitly selected state.

The environment is reconstructed rather than inherited. Keys never appear in
command arguments or console output. Central routing remains disabled here.
"""

import argparse
import json
import os
from pathlib import Path
import plistlib
import re
import selectors
import stat
import subprocess
import time
from uuid import uuid4

from init_private_stack import PROJECT, STATE_DIRECTORY, create_file


def private_file(path: Path, limit: int = 65536) -> str:
    """Read only a bounded, owner-private regular file, without following links."""
    flags = os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK
    descriptor = os.open(path, flags)
    with os.fdopen(descriptor) as stream:
        metadata = os.fstat(stream.fileno())
        if not stat.S_ISREG(metadata.st_mode) or metadata.st_uid != os.getuid() or metadata.st_mode & 0o077:
            raise ValueError("private file ownership or permissions changed")
        value = stream.read(limit + 1)
        if len(value) > limit:
            raise ValueError("private file exceeds size limit")
        return value


def selected_root(path: Path) -> Path:
    """Require the exact completed store initializer marker and private root."""
    root = path.absolute()
    if root != STATE_DIRECTORY or root.is_symlink() or root != root.resolve():
        raise ValueError("use the canonical state directory")
    metadata = root.stat()
    if metadata.st_uid != os.getuid() or metadata.st_mode & 0o077:
        raise ValueError("state directory must remain owner-private")
    marker = json.loads(private_file(root / ".ortak-private-stack.json", 4096))
    if marker != {"project": PROJECT, "state_directory": str(root)}:
        raise ValueError("state marker does not match this private stack")
    return root


def base_environment(root: Path) -> dict[str, str]:
    """Use no ambient provider, profile, database, proxy or dotenv settings."""
    return {
        "PATH": "/usr/bin:/bin:/usr/sbin:/sbin",
        "HOME": str(root / "home"),
        "TMPDIR": str(root / "tmp"),
        "LANG": "en_US.UTF-8",
        "RUST_LOG": "warn",
    }


def identity(binary: Path, root: Path) -> dict[str, str]:
    """Generate an identity through the built Nostr library without logging it."""
    process = subprocess.Popen(
        [str(binary), "generate-key"], cwd=root, env=base_environment(root),
        stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL,
    )
    output = bytearray()
    deadline = time.monotonic() + 5
    try:
        with selectors.DefaultSelector() as ready:
            ready.register(process.stdout, selectors.EVENT_READ)
            while True:
                remaining = deadline - time.monotonic()
                if remaining <= 0 or not ready.select(remaining):
                    raise ValueError("key generator timed out")
                chunk = os.read(process.stdout.fileno(), 1025 - len(output))
                if not chunk:
                    break
                output.extend(chunk)
                if len(output) > 1024:
                    raise ValueError("unexpected key-generator response")
        if process.wait(timeout=max(0.001, deadline - time.monotonic())) != 0:
            raise ValueError("key generator failed")
    finally:
        if process.poll() is None:
            process.kill()
            process.wait(timeout=2)
        process.stdout.close()
    matches = dict(re.findall(r"(Public|Secret) key:\s+([0-9a-f]{64})", output.decode()))
    if set(matches) != {"Public", "Secret"}:
        raise ValueError("invalid key-generator response")
    return {"public_key": matches["Public"], "secret_key": matches["Secret"]}


def prepare(root: Path, binaries: Path) -> None:
    """Atomically save one new identity bundle, or preserve the completed bundle."""
    for name in ("home", "tmp", "repos", "pack-cache", "logs"):
        directory = root / name
        if not directory.exists():
            directory.mkdir(mode=0o700)
        if (directory.is_symlink() or not directory.is_dir()
                or directory.stat().st_uid != os.getuid() or directory.stat().st_mode & 0o077):
            raise ValueError("private child directory ownership changed")
    destination = root / "identities.json"
    if destination.exists():
        bundle = json.loads(private_file(destination))
        if bundle.get("project") != PROJECT:
            raise ValueError("identity bundle belongs to another stack")
        print("Existing private identity bundle preserved.")
        return
    bundle = {
        "project": PROJECT,
        "company_id": str(uuid4()),
        "employee_id": "ada-private",
        **{name: identity(binaries / "buzz-admin", root) for name in ("relay", "owner", "employee")},
    }
    create_file(destination, json.dumps(bundle, indent=2) + "\n")
    print("Fresh relay, human fixture and employee identities saved privately; no activation occurred.")


def environment(root: Path) -> dict[str, str]:
    """Bind the native services to this stack's exact local stores and identity."""
    lines = private_file(root / "runtime.env").splitlines()
    values = dict(line.split("=", 1) for line in lines)
    if len(lines) != 2 or set(values) != {"ORTAK_DATABASE_URL", "ORTAK_REDIS_URL"}:
        raise ValueError("runtime file must contain exactly the two selected stores")
    database = values.get("ORTAK_DATABASE_URL", "")
    redis = values.get("ORTAK_REDIS_URL", "")
    if not re.fullmatch(r"postgres://ortak:[0-9a-f]{64}@127\.0\.0\.1:55433/ortak", database):
        raise ValueError("database is not the selected private store")
    if not re.fullmatch(r"redis://:[0-9a-f]{64}@127\.0\.0\.1:56382/0", redis):
        raise ValueError("Redis is not the selected private store")
    bundle = json.loads(private_file(root / "identities.json"))
    if bundle.get("project") != PROJECT:
        raise ValueError("identity bundle belongs to another stack")
    for name in ("relay", "owner", "employee"):
        for field in ("public_key", "secret_key"):
            if not re.fullmatch(r"[0-9a-f]{64}", bundle[name][field]):
                raise ValueError("invalid private identity bundle")
    return {
        **base_environment(root), **values,
        "DATABASE_URL": database, "REDIS_URL": redis,
        "RELAY_URL": "ws://localhost:3038",
        "BUZZ_RELAY_URL": "ws://localhost:3038",
        "BUZZ_BIND_ADDR": "127.0.0.1:3038",
        "BUZZ_HEALTH_PORT": "8089", "BUZZ_METRICS_PORT": "9198",
        "BUZZ_RELAY_PRIVATE_KEY": bundle["relay"]["secret_key"],
        "BUZZ_PRIVATE_KEY": bundle["owner"]["secret_key"],
        "RELAY_OWNER_PUBKEY": bundle["owner"]["public_key"],
        "BUZZ_REQUIRE_AUTH_TOKEN": "true",
        "BUZZ_REQUIRE_RELAY_MEMBERSHIP": "true",
        "BUZZ_ADMIN_AUTH": "nip98",
        "BUZZ_CORS_ORIGINS": "http://localhost:1427,tauri://localhost",
        "BUZZ_DB_POOL_SIZE": "8", "BUZZ_REDIS_POOL_SIZE": "4",
        "BUZZ_AUTO_MIGRATE": "false",
        "ORTAK_CENTRAL_ROUTING_ENABLED": "false",
        "BUZZ_HUDDLE_AUDIO_AVAILABLE": "false", "BUZZ_MESH": "off",
        "BUZZ_PUSH_ENABLED": "false",
        "BUZZ_GIT_REPO_PATH": str(root / "repos"),
        "BUZZ_GIT_PACK_CACHE_PATH": str(root / "pack-cache"),
        "ORTAK_API_BIND": "127.0.0.1:8787",
    }


def main() -> None:
    """Prepare or replace this process with one explicitly selected service."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--state-dir", type=Path, required=True)
    parser.add_argument("--binary-dir", type=Path, required=True)
    parser.add_argument("action", choices=("prepare", "migrate", "relay", "api", "buzz", "admin", "desktop"))
    parser.add_argument("arguments", nargs=argparse.REMAINDER)
    args = parser.parse_args()
    root = selected_root(args.state_dir)
    binaries = args.binary_dir.resolve(strict=True)
    os.umask(0o077)
    if args.action == "prepare":
        prepare(root, binaries)
        return
    env = environment(root)
    commands = {"migrate": ["buzz-admin", "migrate"], "relay": ["buzz-relay"],
                "api": ["ortak-server"], "buzz": ["buzz"], "admin": ["buzz-admin"],
                "desktop": ["buzz-desktop"]}
    command = commands[args.action]
    if args.arguments:
        if args.action not in {"buzz", "admin"}:
            raise ValueError("service action does not accept arbitrary arguments")
        if "generate-key" in args.arguments:
            raise ValueError("use prepare to keep generated keys out of console output")
        command.extend(args.arguments)
    if args.action == "api":
        env["ORTAK_API_CONFIG_JSON"] = private_file(root / "api-config.json")
    if args.action == "desktop":
        # This explicit private launcher selects the newly generated test owner;
        # it neither reads nor replaces a desktop/keyring identity. The compiled
        # private bundle keeps all app data and model caches in its own namespace.
        expected = (Path(__file__).resolve().parents[2] / "desktop/src-tauri/target"
                    / "ortak-private-native/debug/bundle/macos/Ortak Private.app/Contents/MacOS")
        if binaries != expected.resolve(strict=True):
            raise ValueError("desktop action requires the verified private app bundle")
        info = plistlib.loads((binaries.parent / "Info.plist").read_bytes())
        if info.get("CFBundleIdentifier") != "dev.ortak.private20260905":
            raise ValueError("desktop bundle identity differs")
        env["HOME"] = str(Path.home())
    if args.action == "relay":
        store = json.loads(private_file(root / "object-store/credentials.json", 4096))
        if (set(store) != {"access_key", "secret_key"}
                or not re.fullmatch(r"[0-9a-f]{32}", store["access_key"])
                or not re.fullmatch(r"[0-9a-f]{64}", store["secret_key"])):
            raise ValueError("invalid selected object-store credentials")
        env.update({"BUZZ_S3_ENDPOINT": "http://127.0.0.1:9008",
                    "BUZZ_S3_ACCESS_KEY": store["access_key"],
                    "BUZZ_S3_SECRET_KEY": store["secret_key"],
                    "BUZZ_S3_BUCKET": "ortak-private-media",
                    "BUZZ_S3_REGION": "us-east-1"})
    command[0] = str(binaries / command[0])
    os.chdir(root)
    os.execve(command[0], command, env)


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, KeyError, subprocess.SubprocessError):
        raise SystemExit("Private service setup failed; selected state was preserved. No secrets were logged.") from None
