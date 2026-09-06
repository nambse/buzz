"""Isolated pinned Hermes OAuth protocol helpers, with no ambient auth resolution."""
import argparse
from contextlib import redirect_stderr, redirect_stdout
import json
import logging
import os
from pathlib import Path
import signal
import sys
import tempfile

from .journal import BridgeError
from .oauth_credentials import MAX_STATE, token_text
from .verify_source import verify_source
from .worker import prepare_home


def load_oauth_helpers():
    """Import only reviewed image source after the caller prepared an empty home."""
    verify_source(Path('/opt/hermes'))
    sys.path.insert(0, '/opt/hermes')
    from hermes_cli.auth import _codex_device_code_login, refresh_codex_oauth_pure
    return _codex_device_code_login, refresh_codex_oauth_pure


def flow(action, payload):
    """Return closed errors; never expose raw provider exception messages."""
    try:
        _, refresh = load_oauth_helpers()
        if action == 'refresh':
            if set(payload) != {'access_token', 'refresh_token'}:
                raise BridgeError('oauth_request_uncertain', 503)
            return refresh(token_text(payload['access_token']), token_text(payload['refresh_token']),
                           timeout_seconds=20)
    except Exception as error:
        if getattr(error, 'code', None) == 'codex_rate_limited':
            return {'error': 'oauth_retry_later'}
        if getattr(error, 'relogin_required', False):
            return {'error': 'oauth_relogin_required'}
    return {'error': 'oauth_request_uncertain'}


def main():
    """Private child IPC only; the interactive login command never uses this output."""
    parser = argparse.ArgumentParser()
    parser.add_argument('action', choices=('refresh',))
    args = parser.parse_args()
    os.umask(0o077)
    signal.signal(signal.SIGALRM, signal.SIG_DFL)
    signal.alarm(32)
    raw = sys.stdin.buffer.read(MAX_STATE + 1)
    if len(raw) > MAX_STATE:
        raise BridgeError('oauth_request_uncertain', 503)
    payload = json.loads(raw)
    logging.disable(logging.CRITICAL)
    with tempfile.TemporaryDirectory(prefix='ortak-oauth-flow-') as directory:
        prepare_home(Path(directory) / 'home')
        with open(os.devnull, 'w') as quiet, redirect_stdout(quiet), redirect_stderr(quiet):
            result = flow(args.action, payload)
    # Access/refresh values travel only over the parent's private bounded pipe;
    # stdout must never be connected to a terminal or a log for this subcommand.
    if sys.stdout.isatty():
        raise BridgeError('oauth_private_pipe_required', 503)
    sys.stdout.write(json.dumps(result, separators=(',', ':')))


if __name__ == '__main__':
    try:
        main()
    except BaseException:
        sys.exit(1)
