"""Closed private failure coordinates, never exception messages or frame values."""
from pathlib import Path
from enum import Enum


STAGES = frozenset({
    'load_runtime', 'construct_runtime', 'selection_check', 'prompt_build',
    'conversation_run', 'transport_select', 'request_build', 'provider_request',
    'provider_return', 'response_validate', 'response_normalize',
    'response_normalized', 'result_validate', 'result_commit',
})
KINDS = frozenset({
    'bridge', 'attribute', 'key', 'type', 'value', 'runtime', 'timeout',
    'permission', 'os', 'import', 'unicode', 'sqlite', 'provider_auth',
    'provider_rate_limit', 'provider_connection', 'provider_status', 'provider_api', 'other',
    'http_read_timeout', 'http_connect_timeout', 'http_write_timeout',
    'http_pool_timeout', 'http_protocol', 'http_connection', 'http_io',
})
BOUNDARIES = frozenset({
    'provider_incomplete', 'provider_response_invalid', 'invalid_output',
    'runtime_selection_changed', 'runtime_model_changed', 'runtime_reasoning_changed',
    'unsupported_hermes_tool_selection', 'unsupported_hermes_tool_boundary',
    'unsupported_hermes_credential_boundary', 'unsupported_hermes_response_boundary',
    'unsupported_hermes_reasoning_boundary', 'event_capacity',
    'image_source_mismatch', 'invalid_source_lock', 'source_environment_forbidden',
    'image_revision_mismatch', 'unexpected_hermes_source',
})
HTTP_STATUSES = frozenset({400, 401, 403, 404, 408, 409, 413, 422, 429, 500, 502, 503, 504})
# Closed reasons observed in the selected conversation/recovery source seams.
PROVIDER_REASONS = frozenset({
    'auth', 'billing', 'rate_limit', 'upstream_rate_limit', 'timeout', 'overloaded',
    'content_policy_blocked', 'context_overflow', 'image_corrupt', 'image_too_large',
    'invalid_encrypted_content', 'llama_cpp_grammar_pattern', 'long_context_tier',
    'multimodal_tool_content_unsupported', 'oauth_long_context_beta_forbidden',
    'payload_too_large', 'ssl_cert_verification', 'thinking_signature',
})
# Exact Python files from the reviewed source lock. No suffix matching against
# arbitrary paths, exception-provided filenames, source text or local variables.
HERMES_FILES = frozenset({
    'run_agent.py', 'model_tools.py', 'agent/agent_init.py', 'agent/tool_executor.py',
    'agent/conversation_loop.py', 'agent/agent_runtime_helpers.py',
    'agent/process_bootstrap.py', 'hermes_cli/env_loader.py', 'hermes_constants.py',
    'tools/registry.py', 'tools/lazy_deps.py', 'tools/env_probe.py',
    'agent/transports/codex.py', 'agent/transports/chat_completions.py',
    'agent/transports/__init__.py', 'agent/codex_headers.py', 'agent/codex_runtime.py',
    'agent/codex_responses_adapter.py', 'agent/reasoning_effort.py', 'hermes_cli/auth.py',
})
BRIDGE_FILES = frozenset({'hermes_candidate.py', 'journal.py', 'worker.py', 'verify_source.py'})
_BRIDGE_ROOT = str(Path(__file__).parent) + '/'


def exception_kind(error):
    """Map types to static words without stringifying any exception or its args."""
    cls = type(error)
    if cls.__module__ == 'ortak_hermes_bridge.journal' and cls.__name__ == 'BridgeError':
        return 'bridge'
    if cls.__module__ == 'sqlite3':
        return 'sqlite'
    if cls.__module__ in {'httpx', 'httpx._exceptions'}:
        return {'ReadTimeout': 'http_read_timeout', 'ConnectTimeout': 'http_connect_timeout',
                'WriteTimeout': 'http_write_timeout', 'PoolTimeout': 'http_pool_timeout',
                'RemoteProtocolError': 'http_protocol', 'LocalProtocolError': 'http_protocol',
                'ConnectError': 'http_connection', 'ReadError': 'http_io',
                'WriteError': 'http_io', 'CloseError': 'http_io'}.get(cls.__name__, 'other')
    if cls.__module__ in {'openai', 'openai._exceptions'}:
        return {'AuthenticationError': 'provider_auth', 'PermissionDeniedError': 'provider_auth',
                'RateLimitError': 'provider_rate_limit', 'APIConnectionError': 'provider_connection',
                'APITimeoutError': 'timeout', 'APIStatusError': 'provider_status',
                'BadRequestError': 'provider_status', 'InternalServerError': 'provider_status',
                'NotFoundError': 'provider_status', 'APIError': 'provider_api'}.get(cls.__name__, 'other')
    for kind, classes in (
        ('unicode', (UnicodeError,)), ('attribute', (AttributeError,)),
        ('key', (KeyError,)), ('type', (TypeError,)), ('value', (ValueError,)),
        ('timeout', (TimeoutError,)), ('permission', (PermissionError,)),
        ('import', (ImportError,)), ('os', (OSError,)), ('runtime', (RuntimeError,)),
    ):
        if isinstance(error, classes):
            return kind
    return 'other'


def validate_diagnostic(value):
    """Validate again at persistence, so no caller can use this as a log sink."""
    required = {'stage', 'kind', 'boundary', 'frames'}
    if not isinstance(value, dict) or set(value) not in (required, required | {'provider_failure'}):
        return False
    if 'provider_failure' in value:
        original = value['provider_failure']
        if not isinstance(original, dict) or set(original) != required | {'http_status', 'reason'}:
            return False
        if not validate_diagnostic({k: original[k] for k in required}) or len(original['frames']) > 4:
            return False
        if original['stage'] != 'provider_request':
            return False
        status, reason = original['http_status'], original['reason']
        if status is not None and (type(status) is not int or status not in HTTP_STATUSES):
            return False
        if reason is not None and (type(reason) is not str or reason not in PROVIDER_REASONS):
            return False
    if not isinstance(value['stage'], str) or not isinstance(value['kind'], str) or value['stage'] not in STAGES or value['kind'] not in KINDS:
        return False
    if value['boundary'] is not None and (not isinstance(value['boundary'], str) or value['boundary'] not in BOUNDARIES):
        return False
    frames = value['frames']
    if not isinstance(frames, list) or len(frames) > 8:
        return False
    for frame in frames:
        if not isinstance(frame, dict) or set(frame) != {'source', 'file', 'line'}:
            return False
        allowed = HERMES_FILES if frame['source'] == 'hermes' else BRIDGE_FILES if frame['source'] == 'bridge' else ()
        if not isinstance(frame['file'], str) or frame['file'] not in allowed or type(frame['line']) is not int or not 1 <= frame['line'] <= 100_000:
            return False
    return True


class FailureStage:
    """In-memory coordinates for one run; no response text or token is retained."""
    def __init__(self):
        self.stage = 'load_runtime'
        self.first_provider_failure = None
        self.provider_classified = False

    def at(self, stage):
        if stage not in STAGES:
            raise ValueError('invalid diagnostic stage')
        self.stage = stage

    def provider_error(self, error):
        """Keep the first thrown request error, before upstream error handling."""
        if self.first_provider_failure is None:
            value = self._coordinates(error)
            value['frames'] = value['frames'][-4:]
            value.update(http_status=None, reason=None)
            self.first_provider_failure = value

    def provider_classification(self, status, reason):
        """Read only closed status/enum values; never retain error_context."""
        if self.first_provider_failure is None or self.provider_classified:
            return
        self.provider_classified = True
        value = self.first_provider_failure
        if value['http_status'] is None and type(status) is int and status in HTTP_STATUSES:
            value['http_status'] = status
        reason = reason.value if isinstance(reason, Enum) else None
        if value['reason'] is None and type(reason) is str and reason in PROVIDER_REASONS:
            value['reason'] = reason

    def capture(self, error):
        value = self._coordinates(error)
        if self.first_provider_failure is not None:
            value['provider_failure'] = self.first_provider_failure
        return value

    def _coordinates(self, error):
        kind = exception_kind(error)
        # Only the known BridgeError owns this closed code property.
        boundary = error.code if kind == 'bridge' and error.code in BOUNDARIES else None
        frames, trace = [], error.__traceback__
        for _ in range(64):
            if trace is None:
                break
            name = trace.tb_frame.f_code.co_filename
            source, relative = None, None
            if name.startswith('/opt/hermes/') and name[len('/opt/hermes/'):] in HERMES_FILES:
                source, relative = 'hermes', name[len('/opt/hermes/'):]
            elif name.startswith(_BRIDGE_ROOT) and name[len(_BRIDGE_ROOT):] in BRIDGE_FILES:
                source, relative = 'bridge', name[len(_BRIDGE_ROOT):]
            if source is not None and 1 <= trace.tb_lineno <= 100_000:
                frames.append({'source': source, 'file': relative, 'line': trace.tb_lineno})
            trace = trace.tb_next
        return {'stage': self.stage, 'kind': kind, 'boundary': boundary, 'frames': frames[-8:]}
