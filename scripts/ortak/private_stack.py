#!/usr/bin/env python3
"""Ortak özel kurulumu: install, status, start, stop veya restart."""

import argparse
import json
import os
from pathlib import Path

from private_stack_install import register
from private_stack_services import start, status, stop
from private_stack_state import Installation


def main():
    """One locked operator action; failure keeps its durable checkpoint for recovery."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("action", choices=("install", "upgrade-launcher", "install-app", "open", "status", "start", "stop", "restart", "backup", "verify-backup"))
    parser.add_argument("--native-bundle", type=Path)
    parser.add_argument("--backup", type=Path)
    args = parser.parse_args()
    if args.action in ("backup", "install-app") and args.native_bundle is None:
        parser.error("this action requires --native-bundle; close the private application first")
    if args.action == "verify-backup" and args.backup is None:
        parser.error("verify-backup requires --backup")
    os.umask(0o077)
    installation = Installation()
    try:
        if args.action in ("install", "upgrade-launcher"):
            result = register(installation, upgrade=args.action == "upgrade-launcher")
        else:
            installation.load()
            if args.action in ("install-app", "open"):
                from private_stack_desktop import install_app, open_app
                result = install_app(installation, args.native_bundle) if args.action == "install-app" else open_app(installation)
            elif args.action == "restart":
                stop(installation)
                result = start(installation)
            elif args.action == "backup":
                from private_stack_backup import backup
                result = backup(installation, args.native_bundle)
            elif args.action == "verify-backup":
                from private_stack_restore import verify
                result = verify(installation, args.backup)
            else:
                result = {"status": status, "start": start, "stop": stop}[args.action](installation)
        print(json.dumps(result, ensure_ascii=False, indent=2))
    finally:
        installation.close()


if __name__ == "__main__":
    try:
        main()
    except Exception:
        raise SystemExit("Ortak işlemi durdu. Veriler korundu; lifecycle/operation.json kaydını ve status sonucunu inceleyin.") from None
