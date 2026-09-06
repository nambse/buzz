"""One explicitly selected Honcho bundle with durable same-key preparation."""
import http.client
import json
import os
import stat
import time
from urllib.parse import urlsplit

from bootstrap_private_memory import (PROTOCOL, create_body, creation_receipt, diagnostic,
                                     expected_resource, validate_identity, validate_write)
from disposable_employee import canonical, pending, present, read, require, save, selected_root, validate


class Http:
    """Fixed validated loopback origin, no proxy/redirect/retry, twenty-second budget."""
    def __init__(self, origin, token):
        self.origin = urlsplit(origin)
        require(isinstance(token, str) and 1 <= len(token) <= 16384 and token.isascii()
                and all(33 <= ord(c) <= 126 for c in token), 'selected_memory_token_unavailable')
        self.token, self.deadline = token, time.monotonic() + 20

    def request(self, method, path, body=None):
        remaining = self.deadline - time.monotonic()
        require(remaining > 0, 'memory_deadline')
        connection = http.client.HTTPConnection(self.origin.hostname, self.origin.port, timeout=min(5, remaining))
        headers = {'Authorization': 'Bearer ' + self.token}
        data = None if body is None else canonical(body).encode()
        require(data is None or len(data) <= 16384, 'memory_request_bound')
        if data is not None: headers['Content-Type'] = 'application/json'
        try:
            connection.request(method, path, body=data, headers=headers)
            response = connection.getresponse()
            require(response.status in (200, 201), 'memory_service_refused')
            size = response.getheader('Content-Length')
            require(size is None or 0 <= int(size) <= 65536, 'memory_response_bound')
            chunks, total = [], 0
            while True:
                remaining = self.deadline - time.monotonic()
                require(remaining > 0, 'memory_deadline')
                if connection.sock is not None: connection.sock.settimeout(min(5, remaining))
                chunk = response.read1(min(4096, 65537 - total))
                if not chunk: break
                chunks.append(chunk); total += len(chunk)
                require(total <= 65536, 'memory_response_bound')
            return json.loads(b''.join(chunks))
        finally: connection.close()


def exports(selection, state):
    """Both real consumers receive the identical original creation receipt."""
    memory = selection['memory']
    receipt = creation_receipt(state)
    common = {key: memory[key] for key in ('origin', 'token_ref', 'token_env')}
    diagnostic_pin = {key: memory[key] for key in ('validation_run_id', 'validation_recorded_at')}
    return {'prepared-memory.json': {**common, **diagnostic_pin, 'creation_receipt': receipt, 'validate_memory_io': True},
            'worker-memory-prepared.json': {**common, 'deployment_id': memory['deployment_id'],
                'endpoint_ref': memory['binding']['endpoint_ref'], 'validate_memory_io': True,
                'require_creation_receipts': True, 'employees': [{**diagnostic_pin,
                    'employee_id': selection['employee_id'], 'binding': memory['binding'],
                'creation_key': memory['creation_key'], 'creation_receipt': receipt}]}}


def validate_state(state, intent):
    """Validate both published and staged journal state before credential lookup."""
    require(isinstance(state, dict) and set(state) == {'intent', 'resource_receipt', 'resource_identity', 'roundtrip_receipt', 'completed'}
            and state['intent'] == intent and type(state['completed']) is bool, 'memory_intent_changed')
    require(not state['resource_identity'] or state['resource_receipt'], 'memory_state_inconsistent')
    require(not state['roundtrip_receipt'] or state['resource_identity'], 'memory_state_inconsistent')
    require(not state['completed'] or state['roundtrip_receipt'], 'memory_state_inconsistent')
    if state['resource_receipt'] is not None:
        require(state['resource_receipt'] == expected_resource(intent), 'memory_receipt_changed')
    if state['resource_identity'] is not None: validate_identity(intent, state['resource_identity'])
    if state['roundtrip_receipt'] is not None: validate_write(intent, state['roundtrip_receipt'])


def prepare_memory(selection, http_factory, *, export_only=False):
    """Read immutable intent before token resolution; no automatic external retries."""
    validate(selection)
    with selected_root(selection) as root:
        directory = root / 'memory'
        if not directory.exists():
            require(not export_only, 'completed_memory_required'); directory.mkdir(mode=0o700)
        meta = directory.lstat()
        require(stat.S_ISDIR(meta.st_mode) and meta.st_uid == os.getuid()
                and stat.S_IMODE(meta.st_mode) == 0o700, 'private_memory_directory_changed')
        target = directory / 'bootstrap.json'
        intent = {**selection['memory'], 'company_id': selection['company_id'], 'employee_id': selection['employee_id']}
        if present(target):
            state = read(target)
        else:
            require(not export_only and {p.name for p in directory.iterdir()} <= {pending(target).name}, 'unmarked_memory_directory')
            state = {'intent': intent, 'resource_receipt': None, 'resource_identity': None,
                     'roundtrip_receipt': None, 'completed': False}
        validate_state(state, intent)
        if present(pending(target)):
            staged = read(pending(target), staged=True)
            validate_state(staged, intent)
            require(all(state[field] is None or state[field] == staged[field] for field in
                        ('resource_receipt', 'resource_identity', 'roundtrip_receipt'))
                    and (not state['completed'] or staged['completed'])
                    and (present(target) or staged == state), 'memory_pending_transition_changed')
            state = staged
        save(target, state)
        require(not export_only or state['completed'], 'completed_memory_required')
        if state['completed']:
            for name, fragment in exports(selection, state).items():
                leaf = directory / name
                if present(leaf): require(read(leaf) == fragment, 'memory_export_changed')
                if present(pending(leaf)):
                    require(read(pending(leaf), staged=True) == fragment, 'memory_export_changed')
        # Credential lookup and transport construction occur only after all
        # local scope, original receipt and recovery checks have succeeded.
        http = http_factory()
        require(http.request('GET', '/v3/ortak/protocol') == {'protocol': PROTOCOL, 'honcho_version': '3.1.1'}, 'wrong_memory_protocol')
        if state['resource_receipt'] is None:
            received = http.request('POST', '/v3/ortak/resources/create', create_body(intent))
            require(received == expected_resource(intent), 'memory_receipt_mismatch')
            state['resource_receipt'] = received; save(target, state)
        base = '/v3/ortak/workspaces/' + intent['binding']['workspace']
        body = {key: create_body(intent)[key] for key in ('company_id', 'employee_id', 'user_peer', 'employee_peer')}
        received = http.request('POST', base + '/resources/inspect', body)
        validate_identity(intent, received)
        require(state['resource_identity'] is None or state['resource_identity'] == received, 'native_memory_identity_changed')
        if state['resource_identity'] is None:
            state['resource_identity'] = received; save(target, state)
        verified_now = not state['completed']
        if verified_now:
            _, session, write, recall = diagnostic(intent)
            received = http.request('POST', base + '/sessions/' + session + '/remember', write)
            record = validate_write(intent, received)
            require(state['roundtrip_receipt'] is None or state['roundtrip_receipt'] == received, 'memory_diagnostic_changed')
            state['roundtrip_receipt'] = received; save(target, state)
            recalled = http.request('POST', base + '/sessions/' + session + '/recall', recall)
            require(isinstance(recalled, dict) and set(recalled) == {'records', 'truncated'}
                    and type(recalled['truncated']) is bool and recalled['records'] == [record], 'memory_recall_mismatch')
            state['completed'] = True; save(target, state)
        for name, fragment in exports(selection, state).items():
            save(directory / name, fragment, immutable=True)
        return {'result': 'memory_prepared', 'employee_id': selection['employee_id'],
                'roundtrip': 'verified_now' if verified_now else 'previously_verified',
                'employee_activated': False, 'worker_started': False}
