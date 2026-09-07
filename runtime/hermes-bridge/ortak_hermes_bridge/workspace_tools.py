"""One reviewed AIAgent dispatcher. Never imports or invokes upstream file tools."""
import copy
import time

from . import journal_tools
from .hermes_candidate import ToolDenied, ToollessTransport, _field, guarded_agent_class
from .journal import BridgeError
from .workspace_contract import SCHEMA, TOOL, arguments, canonical


def call(value, deny, *, responses=False):
    """Check raw/normalized provider intent before upstream correction or dispatch."""
    function = value if responses else _field(value, 'function')
    name, raw = _field(function, 'name'), _field(function, 'arguments')
    identifier = _field(value, 'call_id') if responses else _field(value, 'id')
    if name != TOOL or not isinstance(identifier, str):
        deny()
    try:
        parsed, hash_value = arguments(raw)
        # Validate the call ID before upstream can synthesize or alter it.
        from .workspace_contract import validate_request
        validate_request({'call_id': identifier, 'file_id': parsed['file_id'],
                          'arguments_hash': hash_value, 'ordinal': 1})
    except BridgeError:
        deny()
    return identifier, parsed['file_id'], hash_value


class WorkspaceTransport(ToollessTransport):
    """Keep exact model selection and permit only one reviewed client function."""
    def build_kwargs(self, model, messages, tools=None, **params):
        if tools != [SCHEMA]:
            self.deny()
        result = super().build_kwargs(model, messages, tools, **params)
        expected = ([{'type': 'function', **SCHEMA['function'], 'strict': False}]
                    if getattr(self.transport, 'api_mode', None) == 'codex_responses' else [SCHEMA])
        if result.get('tools') != expected:
            self.deny()
        result['parallel_tool_calls'] = False
        return result

    def _check_response(self, response):
        calls = []
        for item in _field(response, 'output') or ():
            kind = _field(item, 'type')
            if isinstance(kind, str) and kind.endswith('_call'):
                if kind != 'function_call':
                    self.deny()
                calls.append(call(item, self.deny, responses=True))
        for choice in _field(response, 'choices') or ():
            message = _field(choice, 'message')
            if _field(message, 'function_call'):
                self.deny()
            for item in _field(message, 'tool_calls') or ():
                calls.append(call(item, self.deny))
        if len(calls) > 1:
            self.deny()

    def normalize_response(self, response, **kwargs):
        self._check_response(response)
        normalized = self.transport.normalize_response(response, **kwargs)
        calls = _field(normalized, 'tool_calls') or ()
        if len(calls) > 1:
            self.deny()
        for item in calls:
            call(item, self.deny)
        return normalized


def workspace_agent_class(base, journal, key, selection, diagnostic, deadline):
    """Only the main sequential entry is replaced; all bypass methods stay fatal."""
    guarded = guarded_agent_class(base, journal, key, selection, diagnostic)

    def deny(*args, **kwargs):
        try:
            journal.fail(key, 'policy_denied')
        finally:
            raise ToolDenied()

    def alive():
        row = journal.lookup(key)
        if row is None or row['status'] != 'running' or time.monotonic() >= deadline:
            try:
                journal.fail(key, 'deadline_exceeded')
            finally:
                raise ToolDenied()

    def get_transport(self, *args, **kwargs):
        alive()
        return WorkspaceTransport(base._get_transport(self, *args, **kwargs), deny, selection, diagnostic)

    def dispatch(self, assistant_message, messages, effective_task_id, api_call_count):
        alive()
        calls = _field(assistant_message, 'tool_calls') or ()
        if len(calls) != 1:
            deny()
        identifier, file_id, hash_value = call(calls[0], deny)
        try:
            seconds = min(10, deadline - time.monotonic())
            request = journal_tools.reserve(journal, key, identifier, file_id, hash_value, seconds)
            stop = time.monotonic() + seconds
            while True:
                alive()
                result = journal_tools.consume(journal, key, request)
                if result is not None:
                    if result['status'] != 'completed':
                        raise BridgeError('workspace_tool_failed', 409)
                    # The only upstream interaction is an ordinary tool-result
                    # message. No environment, approval, shell or Files executor.
                    # Pinned Hermes strips outer message whitespace. JSON keeps
                    # every selected content byte inside an escaped string.
                    messages.append({'role': 'tool', 'name': TOOL,
                                     'tool_call_id': identifier,
                                     'content': canonical({**result, 'file_id': file_id})})
                    return
                remaining = stop - time.monotonic()
                if remaining <= 0:
                    raise BridgeError('tool_deadline_exceeded', 409)
                time.sleep(min(0.025, remaining))
        except Exception:
            # A failed read never becomes a fabricated successful model result.
            # Fatal denial bypasses upstream retries around tool execution.
            try:
                journal.fail(key, 'workspace_tool_failed')
            finally:
                raise ToolDenied() from None

    methods = {'_get_transport': get_transport, '_execute_tool_calls': dispatch}
    # Check cancellation immediately before both pinned provider request paths.
    for name in ('_interruptible_api_call', '_interruptible_streaming_api_call'):
        original = getattr(guarded, name, None)
        if callable(original):
            def checked(self, *args, _original=original, **kwargs):
                alive()
                return _original(self, *args, **kwargs)
            methods[name] = checked
    return type('OrtakWorkspaceReadAgent', (guarded,), methods)


def install(agent):
    """Install exactly one schema after verified empty upstream construction."""
    agent.tools = [copy.deepcopy(SCHEMA)]
    agent.valid_tool_names = {TOOL}
