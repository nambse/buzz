"""Enroll one fresh employee via Hermes' device login; token values stay in files."""
import argparse
import logging
import os
from pathlib import Path
import signal
import sys
import tempfile

from .journal import BridgeError
from .oauth_credentials import OAuthStore, oauth_identity
from .oauth_flow import load_oauth_helpers
from .worker import prepare_home


def main():
    """Print only device-login instructions and a fixed completion/failure message."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--directory', required=True)
    parser.add_argument('--company', required=True)
    parser.add_argument('--employee', required=True)
    parser.add_argument('--profile-ref', required=True)
    parser.add_argument('--credential-ref', required=True)
    args = parser.parse_args()
    if not sys.stdin.isatty() or not sys.stdout.isatty():
        raise BridgeError('oauth_interactive_terminal_required')
    os.umask(0o077)
    identity = oauth_identity(args.company, args.employee,
                              {'profile_ref': args.profile_ref, 'credential_refs': [args.credential_ref]})
    store = OAuthStore.create(args.directory, identity)
    logging.disable(logging.CRITICAL)
    signal.signal(signal.SIGALRM, signal.SIG_DFL)
    signal.alarm(1100)
    with tempfile.TemporaryDirectory(prefix='ortak-oauth-login-') as directory:
        prepare_home(Path(directory) / 'home')
        login, _ = load_oauth_helpers()
        store.enroll(login)
    print('Fresh Ortak OAuth session saved privately. Provider/model health is not yet verified.')


if __name__ == '__main__':
    try:
        main()
    except BaseException:
        print('Ortak OAuth login did not complete. No credentials were displayed; owned state was retained.', file=sys.stderr)
        sys.exit(1)
