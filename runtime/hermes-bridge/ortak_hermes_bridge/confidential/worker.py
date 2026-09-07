"""One protected run in the same bounded toolless Hermes worker containment."""
import argparse
import os
import sys
from pathlib import Path
from ..journal import BridgeError, Journal
from ..worker import (arm_deadline, prepare_home, bounded_json, selected_provider_token,
                      load_hermes)
from ..service import Bridge
from ..hermes_candidate import execute_candidate
from .journal import ExecutionJournal, reserve
from .request import ConfidentialRequest
from .wire import _load


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--journal', required=True)
    args = parser.parse_args()
    os.umask(0o077)
    arm_deadline()
    prepare_home('/tmp/hermes-home')
    path = Path(args.journal)
    if path.parent != Path('/ortak-state') or not path.is_file():
        raise BridgeError('invalid_journal_path')
    body = _load(sys.stdin.buffer.read(128 * 1024 + 1), 128 * 1024)
    oauth_token = body.pop('oauth_access_token', None)
    marker = bounded_json('/profile/ORTAK_DISPOSABLE_PROFILE.json')
    profile = bounded_json('/profile/ORTAK_RUNTIME_BINDING.json')
    journal = Journal(path)
    bridge = Bridge(journal, marker['company_id'], [{'employee_id': marker['employee_id'], 'binding': profile}])
    request = ConfidentialRequest(body, bridge)
    try:
        if marker != {'company_id': request.claims['company_id'], 'employee_id': request.claims['employee_id'],
                      'profile_ref': request.spec['binding']['profile_ref']}:
            raise BridgeError('profile_ownership_mismatch')
        receipt = journal.lookup(request.key)
        if receipt is None or receipt['status'] != 'accepted': return
        # Exact replay of the encrypted bytes cannot start/restart a second run.
        _, fresh = reserve(journal, request)
        if fresh: raise BridgeError('confidential_reservation_missing')
        provider = bounded_json('/profile/ORTAK_PROVIDER.json')
        token = selected_provider_token(request.spec, provider, oauth_token)
        execute_candidate(request.spec, ExecutionJournal(journal, request), None, provider['provider'], token,
                          load_base=load_hermes)
    finally:
        request.close()


if __name__ == '__main__':
    try:
        main()
    except BaseException:
        sys.exit(1)
