#!/usr/bin/env python3
"""Prepare only the selected fresh private stack's MinIO credential files."""

import argparse
import json
import os
from pathlib import Path
import re
import secrets

from init_private_stack import create_file
from private_native_services import private_file, selected_root


def prepare(root: Path) -> None:
    """Persist one credential bundle, then converge its two derived secret files."""
    directory = root / "object-store"
    bundle_path = directory / "credentials.json"
    if directory.exists():
        if directory.is_symlink() or not directory.is_dir() or directory.stat().st_mode & 0o077:
            raise ValueError("object-store directory is not private")
        # A directory without a complete bundle is never adopted or overwritten.
        bundle = json.loads(private_file(bundle_path, 4096))
    else:
        directory.mkdir(mode=0o700)
        bundle = {"access_key": secrets.token_hex(16), "secret_key": secrets.token_hex(32)}
        create_file(bundle_path, json.dumps(bundle) + "\n")
    if (set(bundle) != {"access_key", "secret_key"}
            or not re.fullmatch(r"[0-9a-f]{32}", bundle["access_key"])
            or not re.fullmatch(r"[0-9a-f]{64}", bundle["secret_key"])):
        raise ValueError("invalid private object-store bundle")
    for name, field in (("root-user", "access_key"), ("root-password", "secret_key")):
        destination = directory / name
        expected = bundle[field] + "\n"
        if destination.exists():
            if (destination.is_symlink() or not destination.is_file()
                    or destination.stat().st_uid != os.getuid()
                    or destination.stat().st_size > 128
                    or destination.read_text() != expected):
                raise ValueError("object-store secret disagrees with durable bundle")
        else:
            create_file(destination, expected, 0o444)
    print("Private object-store credentials prepared; existing values preserved.")


def main() -> None:
    """Select the same fixed state marker used by the native service launcher."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--state-dir", type=Path, required=True)
    args = parser.parse_args()
    os.umask(0o077)
    prepare(selected_root(args.state_dir))


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, KeyError):
        raise SystemExit("Private object-store preparation failed; existing state was preserved.") from None
