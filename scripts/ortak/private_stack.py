#!/usr/bin/env python3
"""Ortak özel kurulumu: install, status, start, stop veya restart."""

import argparse
import json
import os

from private_stack_install import register
from private_stack_services import start, status, stop
from private_stack_state import Installation


def main():
    """One locked operator action; failure keeps its durable checkpoint for recovery."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("action", choices=("install", "upgrade-launcher", "status", "start", "stop", "restart"))
    args = parser.parse_args()
    os.umask(0o077)
    installation = Installation()
    try:
        if args.action in ("install", "upgrade-launcher"):
            result = register(installation, upgrade=args.action == "upgrade-launcher")
        else:
            installation.load()
            if args.action == "restart":
                stop(installation)
                result = start(installation)
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
