#!/usr/bin/env python3
"""Prepare fresh, private development-store credentials without printing them.

Existing unmarked directories are never adopted or overwritten. This creates no
employee, activates no router and never reads the owner's existing credentials.
"""

import argparse
import json
import os
from pathlib import Path
import secrets


PROJECT = "ortak-private-v0"
# This includes credentials and recovery metadata, not disposable build output.
# macOS clears /private/tmp on reboot; the active stack must live in user storage.
STATE_DIRECTORY = Path.home() / ".local/share/ortak/private-v0"


def create_file(path: Path, content: str, mode: int = 0o600) -> None:
    """Create exactly once, retaining private parent-directory protection."""
    descriptor = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, mode)
    with os.fdopen(descriptor, "w") as stream:
        stream.write(content)
        stream.flush()
        os.fsync(stream.fileno())
    path.chmod(mode)


def main() -> None:
    """Initialize a fresh marked state directory or verify a completed one."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--state-dir", type=Path, required=True)
    args = parser.parse_args()
    requested = args.state_dir.absolute()
    if requested.is_symlink():
        raise ValueError("state directory must not be a symlink")
    root = requested.resolve()
    if root != STATE_DIRECTORY:
        raise ValueError("this fixed Compose project requires its canonical state directory")
    if any(character in str(root) for character in "\n\r='\" $"):
        raise ValueError("state path contains unsupported characters")
    marker = root / ".ortak-private-stack.json"
    expected = {"project": PROJECT, "state_directory": str(root)}
    required = [
        "compose.env", "runtime.env", "secrets/postgres-password",
        "secrets/redis-password", "secrets/redis.conf",
    ]
    if root.exists():
        if (
            root.stat().st_uid != os.getuid()
            or root.stat().st_mode & 0o077
            or not marker.is_file()
            or marker.is_symlink()
            or marker.stat().st_size > 4096
            or json.loads(marker.read_text()) != expected
            or any(not (root / name).is_file() or (root / name).is_symlink() for name in required)
        ):
            raise ValueError("existing directory is not a completed private stack state")
        print(f"Existing marked state preserved: {root}")
        return
    os.umask(0o077)
    root.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
    if (root.parent.is_symlink() or root.parent.stat().st_uid != os.getuid()
            or root.parent.stat().st_mode & 0o077):
        raise ValueError("private stack parent must remain owner-private")
    root.mkdir(mode=0o700, parents=False)
    secret_root = root / "secrets"
    secret_root.mkdir(mode=0o700)
    postgres_password = secrets.token_hex(32)
    redis_password = secrets.token_hex(32)
    # Service UIDs need to read mounted secret files. Their host parents remain
    # mode0700; only these exact files are mounted into the selected containers.
    create_file(secret_root / "postgres-password", postgres_password + "\n", 0o444)
    create_file(secret_root / "redis-password", redis_password + "\n", 0o444)
    create_file(
        secret_root / "redis.conf",
        "bind 0.0.0.0\nprotected-mode yes\nport 6379\ndir /data\n"
        "appendonly yes\nappendfsync everysec\n"
        f"requirepass {redis_password}\n",
        0o444,
    )
    create_file(root / "compose.env", f"ORTAK_PRIVATE_STATE={root}\n")
    create_file(
        root / "runtime.env",
        f"ORTAK_DATABASE_URL=postgres://ortak:{postgres_password}@127.0.0.1:55433/ortak\n"
        f"ORTAK_REDIS_URL=redis://:{redis_password}@127.0.0.1:56382/0\n",
    )
    # The final marker records completion. An interrupted partial initialization
    # remains distinguishable and is never silently overwritten by a retry.
    create_file(marker, json.dumps(expected, indent=2) + "\n")
    print(f"Fresh private state prepared: {root}")
    print("Credentials saved to private files; no employee or routing activation occurred.")


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError):
        raise SystemExit("Private stack initialization failed; existing state was preserved.") from None
