"""Authenticated bounded bridge protocol; executor availability is explicit."""
import hmac
import json
import threading
from http.server import BaseHTTPRequestHandler, HTTPServer
from urllib.parse import parse_qs, unquote, urlsplit
from uuid import UUID

from . import API_VERSION
from .journal import BridgeError, Journal, identity, reference, start_key

MAX_BODY = 256 * 1024
EMPTY_POLICY = {'allowed_tools': [], 'allowed_workspaces': [], 'allowed_networks': [], 'approval_required': []}


def profile_registry(profiles, company_id=None):
    """Freeze explicit model variants without splitting profile or credential ownership."""
    if not isinstance(profiles, list) or len(profiles) > 64:
        raise BridgeError('invalid_profile_registry')
    try:
        frozen = json.loads(json.dumps(profiles, allow_nan=False))
        bindings, identities = set(), {}
        for profile in frozen:
            binding = profile['binding']
            employee, ref = profile['employee_id'], binding['profile_ref']
            if (not isinstance(binding, dict)
                    or any(not isinstance(x, str) or not 1 <= len(x) <= 256 or '\0' in x
                           for x in (employee, ref, binding['model']))
                    or not isinstance(binding['options'], dict)):
                raise ValueError()
            encoded = json.dumps(binding, sort_keys=True, separators=(',', ':'))
            # A profile's model/options may vary. Its owner, workspace and
            # credential identity (including the one OAuth store) may not.
            base = {k: v for k, v in binding.items() if k not in {'model', 'options'}}
            owner = (employee, base, 'oauth_directory' in profile, profile.get('oauth_directory'),
                     'oauth_owner' in profile, profile.get('oauth_owner'))
            if encoded in bindings or (ref in identities and identities[ref] != owner):
                raise ValueError()
            bindings.add(encoded)
            identities[ref] = owner
        # Public-only authorization precedes all credential reads. Existing
        # undelegated registries keep their original validation behavior.
        from .oauth_connection import connection_identity
        for profile in frozen:
            if 'oauth_owner' in profile:
                connection_identity(company_id, profile, frozen)
        return frozen
    except (KeyError, TypeError, ValueError):
        raise BridgeError('invalid_profile_registry') from None


class UnavailableExecutor:
    """Safe shipping default until a contained Hermes executor is validated."""
    available = False

    def inspect(self, binding):
        """Never claim a profile healthy without execution capability evidence."""
        return False

    def start(self, spec, journal):
        """No fallback to fake execution or a locally installed Hermes."""
        raise BridgeError('executor_unavailable', 503)

    def stop(self, key):
        """A disabled executor owns no live work; existing recovery still needs evidence."""
        return False

class Bridge:
    """One authenticated company, immutable server-owned profile registry."""
    def __init__(self, journal, company_id, profiles, executor=None):
        if str(UUID(company_id)) != company_id:
            raise BridgeError('invalid_company')
        self.journal = journal
        self.company_id = company_id
        self.profiles = profile_registry(profiles, company_id)
        self.executor = executor if executor is not None else UnavailableExecutor()
        self.lock = threading.RLock()

    def scoped_key(self, body):
        """Require body identity to match both canonical key and fixed company."""
        key = body.get('idempotency_key')
        company, run = identity(key)
        if body.get('company_id') != self.company_id or company != self.company_id or body.get('run_id') != run:
            raise BridgeError('run_not_found', 404)
        return key

    def profile(self, company, binding, employee=None):
        """Client-provided paths/options never select arbitrary local resources."""
        if company != self.company_id or not isinstance(binding, dict):
            raise BridgeError('profile_not_found', 404)
        for profile in self.profiles:
            if binding == profile['binding'] and (employee is None or employee == profile['employee_id']):
                return profile
        raise BridgeError('profile_not_found', 404)

    def validate(self, body):
        """Validate B2 RunSpec bounds and reject every unsupported policy upfront."""
        if set(body) not in ({'company_id', 'spec'}, {'company_id', 'spec', 'workspace'}) or not isinstance(body['spec'], dict):
            raise BridgeError('invalid_spec')
        spec = body['spec']
        required = {'run_id', 'employee_id', 'revision_id', 'binding', 'permissions', 'input', 'context', 'idempotency_key'}
        if set(spec) != required:
            raise BridgeError('invalid_spec')
        key = self.scoped_key({'company_id': body['company_id'], 'run_id': spec['run_id'], 'idempotency_key': spec['idempotency_key']})
        if not isinstance(spec['employee_id'], str) or not 1 <= len(spec['employee_id']) <= 128:
            raise BridgeError('invalid_spec')
        try:
            if str(UUID(spec['revision_id'])) != spec['revision_id']:
                raise ValueError()
        except (ValueError, TypeError, AttributeError):
            raise BridgeError('invalid_spec') from None
        self.profile(body['company_id'], spec['binding'], spec['employee_id'])
        if not isinstance(spec['input'], str) or not spec['input'].strip() or len(spec['input'].encode()) > 65536 or '\0' in spec['input']:
            raise BridgeError('invalid_spec')
        context = spec['context']
        if not isinstance(context, dict) or set(context) - {'conversation_ref', 'reply_to_message_id', 'work_item_id', 'memory_context', 'conversation_context'}:
            raise BridgeError('invalid_context')
        memory = context.get('memory_context', [])
        if not isinstance(memory, list) or len(memory) > 64 or any(not isinstance(x, str) or len(x.encode()) > 8192 or '\0' in x for x in memory):
            raise BridgeError('invalid_context')
        for name in ('conversation_ref', 'reply_to_message_id', 'work_item_id'):
            value = context.get(name)
            if value is not None and (not isinstance(value, str) or len(value.encode()) > 1024 or '\0' in value):
                raise BridgeError('invalid_context')
        if context.get('work_item_id') is not None:
            try:
                work = UUID(context['work_item_id'])
                if not work.int or str(work) != context['work_item_id'] or context.get('conversation_ref') is not None or context.get('reply_to_message_id') is not None:
                    raise ValueError()
            except (ValueError, TypeError, AttributeError):
                raise BridgeError('invalid_context') from None
        from .conversation_context import validate as validate_conversation
        validate_conversation(spec)
        from .workspace_contract import validate_workspace
        validate_workspace(body.get('workspace'), spec, self.company_id)
        return key, spec

    def dispatch(self, method, url, body=None):
        """Implement the Rust HermesAdapter's wire contract."""
        parsed = urlsplit(url)
        path = unquote(parsed.path)
        if path.startswith('/v1/confidential/'):
            from .confidential.service import dispatch
            return dispatch(self, method, path, parsed.query, body)
        if method == 'GET' and path == '/v1/capabilities':
            caps = ['health_probe', 'profile_inspect', 'run_events', 'run_lookup', 'run_cancel_start', 'run_cancel']
            if self.executor.available:
                caps.append('run_start')
                if getattr(self.executor, 'workspace_text_read', False):
                    caps.append('workspace_text_read')
                if getattr(self.executor, 'confidential_dm', False):
                    caps.append('confidential_dm_v1')
            return {'adapter': 'hermes', 'api_version': API_VERSION, 'capabilities': caps}
        if method == 'POST' and path == '/v1/profiles/inspect':
            self.profile(body.get('company_id'), body.get('binding'))
            resolver = getattr(self.executor, 'credential_references', lambda binding: [])
            return {'profile_ref': body['binding']['profile_ref'],
                    'credential_references': resolver(body['binding']),
                    'healthy': bool(self.executor.available and self.executor.inspect(body['binding']))}
        if method == 'POST' and path == '/v1/profiles/probe':
            if set(body) != {'company_id', 'binding', 'probe_id'}:
                raise BridgeError('invalid_probe')
            self.profile(body['company_id'], body['binding'])
            identity(f"ortak-run:{self.company_id}:{body['probe_id']}")
            with self.lock:
                if not self.executor.available:
                    raise BridgeError('executor_unavailable', 503)
                return self.executor.start_profile_probe(body['binding'], body['probe_id'])
        if method == 'POST' and path == '/v1/runs':
            key, spec = self.validate(body)
            workspace = body.get('workspace')
            with self.lock:
                # Existing tombstones remain authoritative even if the executor is down.
                existing = self.journal.lookup(key)
                if existing is None and not self.executor.available:
                    raise BridgeError('executor_unavailable', 503)
                if existing is None and workspace is not None and not getattr(self.executor, 'workspace_text_read', False):
                    raise BridgeError('unsupported_permission_policy', 422)
                receipt, fresh = self.journal.reserve(spec, workspace=workspace)
                if fresh:
                    try:
                        if workspace is None:
                            self.executor.start(spec, self.journal)
                        else:
                            self.executor.start(spec, self.journal, workspace=workspace)
                    except Exception:
                        self.journal.fail(key, 'executor_unavailable')
                        raise BridgeError('executor_unavailable', 503) from None
                return receipt
        if method == 'POST' and path in {'/v1/runs/tools/pending', '/v1/runs/tools/resolve'}:
            from . import journal_tools
            expected = {'company_id', 'run_id', 'idempotency_key'}
            if path.endswith('/resolve'):
                expected |= {'request', 'result'}
            if set(body) != expected:
                raise BridgeError('invalid_tool_request', 422)
            key = self.scoped_key(body)
            if path.endswith('/pending'):
                return journal_tools.pending(self.journal, key)
            return journal_tools.resolve(self.journal, key, body['request'], body['result'])
        if method == 'POST' and path == '/v1/runs/lookup':
            result = self.journal.lookup(self.scoped_key(body))
            if result is None:
                raise BridgeError('run_not_found', 404)
            return result
        if method == 'POST' and path == '/v1/runs/cancel':
            key = self.scoped_key(body)
            reason = body.get('reason')
            if not isinstance(reason, str) or len(reason.encode()) > 2048:
                raise BridgeError('invalid_reason')
            with self.lock:
                known_start = self.journal.has_start(key)
                outcome = self.journal.request_cancel(key)
                # A terminal journal record never proves container containment.
                # Only a cancellation-only tombstone proves no launch was issued.
                if known_start and not self.executor.stop(key):
                    raise BridgeError('execution_not_stopped', 503)
                if outcome != 'already_terminal':
                    self.journal.finish_cancel(key)
                return {'runtime_run_ref': reference(key), 'outcome': outcome}
        if method == 'GET' and path.startswith('/v1/runs/') and path.endswith('/events'):
            key = start_key(path[len('/v1/runs/'):-len('/events')])
            if identity(key)[0] != self.company_id:
                raise BridgeError('run_not_found', 404)
            query = parse_qs(parsed.query, keep_blank_values=True)
            if set(query) - {'after', 'limit'} or any(len(v) != 1 for v in query.values()):
                raise BridgeError('invalid_cursor')
            try:
                after = int(query.get('after', ['0'])[0])
                limit = int(query.get('limit', ['100'])[0])
            except ValueError:
                raise BridgeError('invalid_cursor') from None
            return self.journal.events(key, after, limit)
        raise BridgeError('not_found', 404)


def handler(bridge, token):
    """Build a no-access-log handler; credentials and request content are not logged."""
    if not isinstance(token, str) or not 32 <= len(token) <= 4096:
        raise BridgeError('invalid_service_credential')

    class Handler(BaseHTTPRequestHandler):
        def setup(self):
            super().setup()
            self.connection.settimeout(5)

        def log_message(self, *args):
            pass

        def do_GET(self):
            self.request_bridge()

        def do_POST(self):
            self.request_bridge()

        def request_bridge(self):
            try:
                authorization = self.headers.get_all('Authorization', [])
                if len(authorization) != 1 or not hmac.compare_digest(authorization[0].encode(), ('Bearer ' + token).encode()):
                    raise BridgeError('unauthorized', 401)
                if self.headers.get('Transfer-Encoding') is not None:
                    raise BridgeError('unsupported_transfer_encoding')
                lengths = self.headers.get_all('Content-Length', [])
                if len(lengths) > 1:
                    raise BridgeError('invalid_body')
                size = int(lengths[0]) if lengths else 0
                confidential = unquote(urlsplit(self.path).path).startswith('/v1/confidential/')
                maximum = 112 * 1024 if confidential else MAX_BODY
                if not 0 <= size <= maximum:
                    raise BridgeError('body_too_large', 413)
                raw = self.rfile.read(size)
                if len(raw) != size:
                    raise BridgeError('invalid_body')
                if confidential and size:
                    from .confidential.wire import _load, ConfidentialError
                    try:
                        body = _load(raw, maximum)
                    except ConfidentialError:
                        raise BridgeError('invalid_confidential_request', 422) from None
                else:
                    body = json.loads(raw) if size else {}
                if not isinstance(body, dict):
                    raise BridgeError('invalid_body')
                result = bridge.dispatch(self.command, self.path, body)
                status = 200
            except BridgeError as error:
                result, status = {'error': error.code}, error.status
            except (ValueError, KeyError, TypeError):
                result, status = {'error': 'invalid_request'}, 400
            except Exception:
                result, status = {'error': 'bridge_unavailable'}, 503
            data = json.dumps(result, separators=(',', ':')).encode()
            self.send_response(status)
            self.send_header('Content-Type', 'application/json')
            self.send_header('Content-Length', str(len(data)))
            self.send_header('Connection', 'close')
            self.end_headers()
            self.wfile.write(data)
            self.close_connection = True
    return Handler


def serve(bridge, token, port, listen_address='127.0.0.1'):
    """Bounded authenticated lane; wildcard bind requires explicit container opt-in."""
    if listen_address not in {'127.0.0.1', '0.0.0.0'}:
        raise BridgeError('invalid_listen_address')
    server = HTTPServer((listen_address, port), handler(bridge, token))
    server.request_queue_size = 8
    server.serve_forever(poll_interval=0.5)
