"""Explicit authenticated operator admission of one real selected-profile probe."""
import argparse
import hashlib
import http.client
import json
import os
import stat
import sys
from uuid import UUID

from .journal import BridgeError
from .service import profile_registry


def selected_profile(config, employee, binding_sha256=None):
    """Require an exact public binding fingerprint when an employee has variants."""
    matches = [p for p in profile_registry(config['profiles'], config.get('company_id'))
               if p['employee_id'] == employee]
    if binding_sha256 is not None:
        if (len(binding_sha256) != 64
                or any(c not in '0123456789abcdef' for c in binding_sha256)):
            raise BridgeError('invalid_binding_fingerprint')
        matches = [p for p in matches if hashlib.sha256(json.dumps(p['binding'],
            sort_keys=True, separators=(',', ':')).encode()).hexdigest() == binding_sha256]
    if len(matches) != 1 or 'oauth_directory' not in matches[0]:
        raise BridgeError('oauth_profile_required')
    return matches[0]


def private_bytes(path, maximum):
    """Read an explicitly supplied private controller file, without symlink following."""
    fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
    with os.fdopen(fd, 'rb') as file:
        meta = os.fstat(file.fileno())
        if (not stat.S_ISREG(meta.st_mode) or meta.st_uid != os.geteuid()
                or stat.S_IMODE(meta.st_mode) != 0o600 or meta.st_nlink != 1):
            raise BridgeError('controller_file_permissions')
        value = file.read(maximum + 1)
    if len(value) > maximum:
        raise BridgeError('controller_file_too_large')
    return value


def main():
    """Return admission identity only; terminal results remain in the normal journal."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--config', required=True)
    parser.add_argument('--token-file', required=True)
    parser.add_argument('--employee', required=True)
    parser.add_argument('--binding-sha256', help='Exact canonical JSON binding hash; required for multiple variants.')
    parser.add_argument('--probe-id', required=True, help='Persist a fresh UUID before admission; retries reuse it.')
    parser.add_argument('--port', type=int, default=8650)
    args = parser.parse_args()
    if str(UUID(args.probe_id)) != args.probe_id or not 1 <= args.port <= 65535:
        raise BridgeError('invalid_probe')
    config = json.loads(private_bytes(args.config, 256 * 1024))
    profile = selected_profile(config, args.employee, args.binding_sha256)
    token = private_bytes(args.token_file, 4096).decode().strip()
    if not 32 <= len(token) <= 4096 or any(c.isspace() for c in token):
        raise BridgeError('invalid_service_credential')
    body = {'company_id': config['company_id'], 'binding': profile['binding'], 'probe_id': args.probe_id}
    connection = http.client.HTTPConnection('127.0.0.1', args.port, timeout=45)
    try:
        connection.request('POST', '/v1/profiles/probe', json.dumps(body),
            {'Authorization': 'Bearer ' + token, 'Content-Type': 'application/json'})
        response = connection.getresponse()
        data = response.read(8193)
        if response.status != 200 or len(data) > 8192:
            raise BridgeError('probe_admission_failed')
        result = json.loads(data)
        expected = 'ortak:' + config['company_id'] + ':' + args.probe_id
        if result.get('runtime_run_ref') != expected or result.get('status') not in {
                'accepted', 'running', 'completed', 'failed', 'cancelling', 'cancelled'}:
            raise BridgeError('invalid_probe_receipt')
        print(json.dumps({'runtime_run_ref': expected, 'status': result['status']}))
    finally:
        connection.close()


if __name__ == '__main__':
    try:
        main()
    except BaseException:
        print('Probe admission did not complete. Inspect the same probe ID before retrying; no credentials were displayed.', file=sys.stderr)
        sys.exit(1)
