"""Explicit owned scoring service, separate from the employee controller HTTP lane."""
import argparse
import asyncio
import json
import logging
import os
from pathlib import Path
import signal
import sys

from ..journal import BridgeError
from ..profile_probe import private_bytes
from ..worker import prepare_home
from .contract import Selection
from .credentials import Maintenance
from .listener import Listener
from .transport import CodexTransport


async def serve(selection, token, host, port):
    transport = CodexTransport()
    maintenance = Maintenance(selection.store)
    listener = Listener(selection, token, transport, maintenance)
    stopping = asyncio.Event()
    loop = asyncio.get_running_loop()
    for name in (signal.SIGINT, signal.SIGTERM):
        loop.add_signal_handler(name, stopping.set)
    maintenance.start()
    try:
        await listener.start(host, port)
        await stopping.wait()
    finally:
        try:
            await listener.close()
        finally:
            # Its subprocess has a 35-second deadline and exact reap. Always
            # retire this owner, including when HTTP transport cleanup fails.
            # A cleanup failure propagates; it never becomes shutdown success.
            maintenance.close()


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--config', required=True)
    parser.add_argument('--token-file', required=True)
    parser.add_argument('--port', type=int, default=8651)
    parser.add_argument('--listen-address', choices=('127.0.0.1', '0.0.0.0'), default='127.0.0.1')
    parser.add_argument('--enable-selected-semantic-oauth', action='store_true')
    args = parser.parse_args()
    if not args.enable_selected_semantic_oauth or not 1 <= args.port <= 65535:
        raise BridgeError('explicit_semantic_selection_required', 422)
    os.umask(0o077)
    logging.disable(logging.CRITICAL)
    # Read only the explicitly named public configuration and private listener key.
    path = Path(args.config)
    if path.stat().st_size > 256 * 1024:
        raise BridgeError('configuration_too_large', 422)
    config = json.loads(path.read_text())
    selection = Selection(config)
    token = private_bytes(Path(args.token_file), 4096).decode().strip()
    # No user home, environment token/proxy, gateway or runtime context is imported.
    prepare_home('/tmp/ortak-semantic-home')
    asyncio.run(serve(selection, token, args.listen_address, args.port))


if __name__ == '__main__':
    try:
        main()
    except BaseException:
        sys.exit(1)
