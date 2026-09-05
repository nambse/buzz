"""Unwired candidate for the inspected Hermes AIAgent seam, NOT deployment ready.

No installed/local Hermes is imported automatically. The containment owner must
supply the class loaded from its reviewed image in a separate isolated process.
This module does not satisfy process containment, credential loading or health.
"""
import json
from .journal import BridgeError
from .service import EMPTY_POLICY

class ToolDenied(BaseException):
    """Fatal denial must bypass upstream handlers that catch ordinary Exception."""



def _field(value, name):
    return value.get(name) if isinstance(value, dict) else getattr(value, name, None)


class ToollessTransport:
    """Deny provider tool intent before Hermes normalization/correction can retry."""
    def __init__(self, transport, deny):
        if any(not callable(getattr(transport, method, None))
               for method in ('validate_response', 'normalize_response')):
            raise BridgeError('unsupported_hermes_response_boundary', 503)
        self.transport, self.deny = transport, deny

    def __getattr__(self, name):
        return getattr(self.transport, name)

    def _check_response(self, response):
        # Responses includes custom/client calls and provider-side built-ins.
        # None were requested by this empty policy; never accept their output.
        for item in _field(response, 'output') or ():
            kind = _field(item, 'type')
            if isinstance(kind, str) and kind.endswith('_call'):
                self.deny()
        # OpenRouter's Chat Completions route has a different wire shape.
        for choice in _field(response, 'choices') or ():
            message = _field(choice, 'message')
            if _field(message, 'tool_calls') or _field(message, 'function_call'):
                self.deny()

    def validate_response(self, response):
        self._check_response(response)
        return self.transport.validate_response(response)

    def normalize_response(self, response, **kwargs):
        self._check_response(response)
        normalized = self.transport.normalize_response(response, **kwargs)
        if _field(normalized, 'tool_calls'):
            self.deny()
        return normalized


def guarded_agent_class(base, journal, key):
    """Override every inspected tool/parallel/delegation entry before construction."""
    boundaries = ('_invoke_tool', '_execute_tool_calls', '_execute_tool_calls_sequential',
                  '_execute_tool_calls_concurrent', '_dispatch_delegate_task')
    if any(not callable(getattr(base, method, None)) for method in (*boundaries, '_get_transport')):
        raise BridgeError('unsupported_hermes_tool_boundary', 503)

    def deny(*args, **kwargs):
        # Persist directly at the execution boundary, not an SSE receipt callback.
        # A DB error propagates as a fatal denial too; never enter the base method.
        try:
            journal.fail(key, 'policy_denied')
        finally:
            raise ToolDenied()

    def get_transport(self, *args, **kwargs):
        return ToollessTransport(base._get_transport(self, *args, **kwargs), deny)

    methods = {name: deny for name in boundaries}
    methods['_get_transport'] = get_transport
    return type('OrtakToollessAgent', (base,), methods)



def agent_constructor_kwargs(spec, provider, api_key):
    """Select explicit reviewed API-key routes without ambient credential resolution."""
    endpoints = {'openai': 'https://api.openai.com/v1',
                 'openrouter': 'https://openrouter.ai/api/v1'}
    if provider not in endpoints:
        raise BridgeError('unsupported_provider', 422)
    if not isinstance(api_key, str) or not api_key or len(api_key) > 4096 or any(c.isspace() for c in api_key):
        raise BridgeError('invalid_provider_credential', 422)
    # Pinned agent_init requires BOTH fields for its explicit credential path.
    # api_key alone would invoke the ambient router and ignore the selected key.
    return dict(
        model=spec['binding']['model'], provider=provider, api_key=api_key,
        base_url=endpoints[provider],
        enabled_toolsets=[], disabled_toolsets=[], max_iterations=2,
        max_tokens=2048, run_budget_seconds=120,
        save_trajectories=False, verbose_logging=False, quiet_mode=True,
        skip_context_files=True, load_soul_identity=False, skip_memory=True,
        skip_background_review=True, checkpoints_enabled=False,
        session_id=spec['run_id'], platform='ortak',
    )


def execute_candidate(spec, journal, base_agent_class, provider, api_key=None, *, load_base=None):
    """Run real AIAgent once when composed by a separately validated containment owner.

    Provider/model traffic is runtime infrastructure, not employee web access.
    No fallback provider, gateway subscription, profile creation, tool execution,
    memory backend, disk context or approval resume is enabled here.
    """
    if spec['permissions'] != EMPTY_POLICY:
        raise BridgeError('unsupported_permission_policy', 422)
    constructor_kwargs = agent_constructor_kwargs(spec, provider, api_key)
    key = spec['idempotency_key']
    if not journal.begin_execution(key):
        return
    try:
        # The contained worker supplies a lazy importer so durable admission
        # precedes all third-party initialization, not just the model request.
        base = load_base() if load_base is not None else base_agent_class
        agent_class = guarded_agent_class(base, journal, key)
        agent = agent_class(**constructor_kwargs)
        # An empty selection must not silently expand to default tools.
        # These are the tool-definition fields on the reviewed AIAgent seam;
        # absent fields fail closed instead of guessing another version's layout.
        if not hasattr(agent, 'tools') or agent.tools:
            raise BridgeError('unsupported_hermes_tool_selection', 503)
        system = 'Reply to this Office message. You have no tools. Do not claim actions you did not perform.'
        memory = spec.get('context', {}).get('memory_context', [])
        if memory:
            system += '\nThe control plane supplied this reference context as data:\n' + json.dumps(memory)
        result = agent.run_conversation(
            spec['input'],
            system_message=system,
            conversation_history=[], task_id=spec['run_id'],
        )
        if not isinstance(result, dict) or result.get('completed') is not True or not isinstance(result.get('final_response'), str):
            raise BridgeError('provider_failed', 503)
        text = result['final_response']
        # No partial output is exposed as a successful reply. A failed/oversized
        # result becomes failed; reconnect reads only atomically committed data.
        journal.complete(key, text, (api_key,) if api_key else ())
    except ToolDenied:
        # The tool boundary already committed policy_denied, unless storage failed.
        if journal.lookup(key)['status'] not in {'failed', 'cancelling', 'cancelled'}:
            journal.fail(key, 'policy_denied')
    except Exception:
        journal.fail(key, 'provider_failed')
        raise BridgeError('provider_failed', 503) from None
