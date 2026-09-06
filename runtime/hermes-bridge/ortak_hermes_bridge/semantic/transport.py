"""One cancellable Codex Responses HTTP request using the pinned Hermes leaf seams."""
import asyncio
import inspect
import json
from pathlib import Path
from types import SimpleNamespace

from ..journal import BridgeError
from ..verify_source import verify_source
from . import INSTRUCTION, MAX_BYTES
from .contract import scores, strict_json

ENDPOINT = 'https://chatgpt.com/backend-api/codex/responses'
BASE_URL = 'https://chatgpt.com/backend-api/codex'


def loaded_helpers():
    """Verify/import format conversion and headers, never an AIAgent constructor."""
    source = Path('/opt/hermes')
    verify_source(source)
    from agent.codex_headers import codex_cloudflare_headers
    from agent.codex_responses_adapter import _chat_messages_to_responses_input, _normalize_codex_response
    helpers = (codex_cloudflare_headers, _chat_messages_to_responses_input, _normalize_codex_response)
    if any(not Path(inspect.getfile(helper)).resolve().is_relative_to(source) for helper in helpers):
        raise BridgeError('unexpected_hermes_source', 503)
    return helpers


def namespace(value):
    if isinstance(value, dict):
        return SimpleNamespace(**{key: namespace(item) for key, item in value.items()})
    if isinstance(value, list):
        return [namespace(item) for item in value]
    return value


def safe_output(items):
    if not isinstance(items, list) or len(items) > 64:
        raise BridgeError('semantic_response_bounds', 503)
    for item in items:
        if not isinstance(item, dict) or item.get('type') not in {'message', 'reasoning'}:
            raise BridgeError('semantic_tool_or_unknown_output', 503)
        if item['type'] == 'message':
            content = item.get('content')
            if (item.get('role') != 'assistant' or item.get('status') != 'completed'
                    or not isinstance(content, list) or not content or len(content) > 32
                    or any(not isinstance(part, dict) or part.get('type') != 'output_text'
                           or not isinstance(part.get('text'), str) for part in content)):
                raise BridgeError('semantic_incomplete', 503)


class CodexTransport:
    """Finite raw SSE consumption; zero SDK retry, tools, discovery or repair paths."""
    def __init__(self, *, client=None, helpers=None):
        import httpx
        self.client = client if client is not None else httpx.AsyncClient(
            trust_env=False, follow_redirects=False,
            limits=httpx.Limits(max_connections=2, max_keepalive_connections=2),
            timeout=httpx.Timeout(4.5, connect=1, pool=0.01))
        self.headers, self.convert, self.normalize = helpers if helpers is not None else loaded_helpers()

    async def close(self):
        await self.client.aclose()

    async def score(self, selected, body, expected, token, deadline):
        async with asyncio.timeout_at(deadline):
            converted = self.convert([{'role': 'user', 'content': json.dumps(body['input'],
                ensure_ascii=False, separators=(',', ':'))}], replay_encrypted_reasoning=False,
                current_issuer_kind='codex_backend', native_compaction_eligible=False)
            # This is the inspected Codex request shape. That backend does not
            # accept max_output_tokens in the pinned transport. Bound time and
            # raw SSE bytes instead of claiming an unenforced token ceiling.
            payload = {'model': selected.model, 'instructions': INSTRUCTION,
                'input': converted, 'store': False, 'stream': True,
                'reasoning': {'effort': selected.effort}, 'tools': []}
            encoded = json.dumps(payload, ensure_ascii=False, separators=(',', ':'), allow_nan=False).encode()
            if len(encoded) > MAX_BYTES:
                raise BridgeError('semantic_bounds', 422)
            headers = self.headers(token, base_url=BASE_URL)
            headers.update({'Authorization': 'Bearer ' + token, 'Content-Type': 'application/json',
                'Accept': 'text/event-stream', 'Accept-Encoding': 'identity',
                'session_id': body['request_id'], 'x-client-request-id': body['request_id']})
            async with self.client.stream('POST', ENDPOINT, content=encoded, headers=headers) as response:
                if response.status_code != 200:
                    raise BridgeError('semantic_provider_rejected', 503)
                # The selected endpoint has returned valid Responses SSE with
                # no Content-Type. Missing metadata can enter the same strict,
                # bounded parser; an explicitly different media type cannot.
                content_type = response.headers.get('content-type')
                if ((content_type is not None and content_type.split(';', 1)[0].strip() != 'text/event-stream')
                        or response.headers.get('content-encoding', 'identity') != 'identity'):
                    raise BridgeError('semantic_provider_protocol', 503)
                final = await self._consume(response)
            if (final.get('status') != 'completed' or final.get('model') != selected.response_model
                    or final.get('error') is not None or final.get('incomplete_details') is not None):
                raise BridgeError('semantic_incomplete', 503)
            safe_output(final.get('output'))
            normalized, finish = self.normalize(namespace(final), issuer_kind='codex_backend')
            if (finish != 'stop' or normalized is None or getattr(normalized, 'tool_calls', None)
                    or not isinstance(getattr(normalized, 'content', None), str)):
                raise BridgeError('semantic_incomplete', 503)
            result = scores(strict_json(normalized.content), expected)
            usage = final.get('usage')
            bounded_usage = None
            if usage is not None:
                if not isinstance(usage, dict):
                    raise BridgeError('semantic_usage_invalid', 503)
                bounded_usage = {name: usage.get(name) for name in ('input_tokens', 'output_tokens', 'total_tokens')}
                if any(value is not None and (type(value) is not int or not 0 <= value <= 1_000_000)
                       for value in bounded_usage.values()):
                    raise BridgeError('semantic_usage_invalid', 503)
            if asyncio.get_running_loop().time() >= deadline:
                raise TimeoutError()
            return selected.response(result, bounded_usage)

    async def _consume(self, response):
        total = 0
        buffer = b''
        data = []
        events = 0
        done = {}
        final = None
        async for chunk in response.aiter_raw():
            total += len(chunk)
            if total > MAX_BYTES:
                raise BridgeError('semantic_response_bounds', 503)
            buffer += chunk
            while b'\n' in buffer:
                line, buffer = buffer.split(b'\n', 1)
                line = line.removesuffix(b'\r')
                if line.startswith(b'data:'):
                    data.append(line[5:].lstrip(b' '))
                elif line == b'' and data:
                    events += 1
                    if events > 512:
                        raise BridgeError('semantic_response_bounds', 503)
                    raw = b'\n'.join(data)
                    data = []
                    if raw == b'[DONE]':
                        if final is None:
                            raise BridgeError('semantic_incomplete', 503)
                        continue
                    event = strict_json(raw)
                    if not isinstance(event, dict):
                        raise BridgeError('semantic_provider_protocol', 503)
                    kind = event.get('type')
                    if kind in {'error', 'response.failed', 'response.incomplete'} or event.get('error'):
                        raise BridgeError('semantic_provider_rejected', 503)
                    if isinstance(kind, str) and ('_call' in kind or '.refusal.' in kind):
                        raise BridgeError('semantic_tool_or_unknown_output', 503)
                    if kind in {'response.output_item.added', 'response.output_item.done'}:
                        item = event.get('item')
                        if not isinstance(item, dict) or item.get('type') not in {'message', 'reasoning'}:
                            raise BridgeError('semantic_tool_or_unknown_output', 503)
                        if kind.endswith('.done'):
                            index = event.get('output_index')
                            if type(index) is not int or not 0 <= index < 64 or index in done:
                                raise BridgeError('semantic_provider_protocol', 503)
                            safe_output([item])
                            done[index] = item
                    if kind == 'response.completed':
                        if final is not None or not isinstance(event.get('response'), dict):
                            raise BridgeError('semantic_provider_protocol', 503)
                        final = event['response']
                        if not final.get('output'):
                            final = dict(final, output=[done[index] for index in sorted(done)])
                    elif not isinstance(kind, str) or not kind.startswith('response.'):
                        raise BridgeError('semantic_provider_protocol', 503)
        if buffer or data or final is None:
            raise BridgeError('semantic_incomplete', 503)
        return final
