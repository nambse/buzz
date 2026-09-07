"""Production score protocol, OAuth ownership and actual private HTTP listener seams."""
import asyncio
import base64
import hashlib
import json
from pathlib import Path
import tempfile
import time
from types import SimpleNamespace
import unittest
from unittest.mock import Mock, patch
from uuid import uuid4

import httpx

from ortak_hermes_bridge.journal import BridgeError
from ortak_hermes_bridge.oauth_credentials import OAuthStore, STATE, atomic_write, oauth_identity
from ortak_hermes_bridge.semantic import PROMPT_VERSION, SCHEMA_VERSION
from ortak_hermes_bridge.semantic.contract import Selection, strict_json
from ortak_hermes_bridge.semantic.credentials import Maintenance, ready_token
from ortak_hermes_bridge.semantic.listener import Listener
from ortak_hermes_bridge.semantic.transport import CodexTransport, ENDPOINT

TOKEN = 'fixture-semantic-private-listener-key'


def access(seconds=3600):
    claims = {'exp': int(time.time()) + seconds,
              'https://api.openai.com/auth': {'chatgpt_account_id': 'semantic-fixture-account'}}
    return 'fixture.' + base64.urlsafe_b64encode(json.dumps(claims).encode()).decode().rstrip('=') + '.fixture'


class Fixture:
    def __init__(self, root):
        self.company = str(uuid4())
        self.binding = {'adapter': 'hermes', 'profile_ref': 'semantic-owned-profile',
            'workspace': '/owned-public-workspace', 'credential_refs': ['credential://semantic/fixture'],
            'model': 'gpt-5.6-sol', 'options': {'reasoning_effort': 'high'}}
        self.store = OAuthStore.create(root / 'oauth', oauth_identity(self.company, 'employee-fixture', self.binding))
        self.store.enroll(lambda: {'tokens': {'access_token': access(), 'refresh_token': 'fixture-refresh-only'}})
        self.config = {'company_id': self.company, 'profiles': [{'employee_id': 'employee-fixture',
            'binding': self.binding, 'oauth_directory': str(self.store.directory)}],
            'semantic': {'deployment_id': str(uuid4()), 'binding_sha256': hashlib.sha256(json.dumps(
                self.binding, sort_keys=True, separators=(',', ':')).encode()).hexdigest(),
                'response_model': self.binding['model']}}
        self.selection = Selection(self.config)

    def request(self, budget=1000):
        return {'deployment_id': self.selection.deployment_id, 'binding_sha256': self.selection.binding_sha256,
            'request_id': str(uuid4()), 'prompt_version': PROMPT_VERSION, 'schema_version': SCHEMA_VERSION,
            'budget_ms': budget, 'input': {'message': 'Deployment plan', 'candidates': [
                {'employee_id': 'ada', 'name': 'Ada', 'title': 'Engineer', 'biography': '',
                 'responsibilities': ['Deployment'], 'domains': ['Engineering']}]}}


def completed(tool=False):
    text = json.dumps({'scores': [{'employee_id': 'ada', 'score': 0.9, 'evidence': 'domain_match'}]})
    item = ({'type': 'function_call', 'name': 'terminal'} if tool else
        {'type': 'message', 'role': 'assistant', 'status': 'completed',
         'content': [{'type': 'output_text', 'text': text}]})
    return {'type': 'response.completed', 'response': {'status': 'completed', 'model': 'gpt-5.6-sol',
        'output': [item], 'usage': {'input_tokens': 50, 'output_tokens': 20, 'total_tokens': 70}}}


def local_helpers():
    """Unit fixtures; installed-artifact gate separately retains real Hermes helpers."""
    return (lambda token, **kwargs: {'originator': 'fixture'},
        lambda messages, **kwargs: [{'role': 'user', 'content': messages[0]['content']}],
        lambda response, **kwargs: (SimpleNamespace(content=response.output[0].content[0].text, tool_calls=None), 'stop'))


class Stream(httpx.AsyncByteStream):
    def __init__(self, chunks, delay=0):
        self.chunks, self.delay, self.closed = chunks, delay, False
        self.read = False

    async def __aiter__(self):
        self.read = True
        for chunk in self.chunks:
            if self.delay:
                await asyncio.sleep(self.delay)
            yield chunk

    async def aclose(self):
        self.closed = True


class Semantic(unittest.IsolatedAsyncioTestCase):
    async def asyncSetUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.fixture = Fixture(Path(self.temporary.name).resolve())
        self.calls = []
        self.streams = []

    def transport(self, event=None, delay=0, chunks=None, headers=None):
        async def wire(request):
            self.calls.append(request)
            data = chunks if chunks is not None else [b'data: ' + json.dumps(event or completed()).encode() + b'\n\n']
            stream = Stream(data, delay)
            self.streams.append(stream)
            return httpx.Response(200, headers={'content-type': 'text/event-stream'} if headers is None else headers,
                                  stream=stream)
        client = httpx.AsyncClient(transport=httpx.MockTransport(wire), trust_env=False, follow_redirects=False)
        return CodexTransport(client=client, helpers=local_helpers())

    async def score(self, transport, body=None):
        body = body or self.fixture.request()
        return await transport.score(self.fixture.selection, body, self.fixture.selection.request(body),
            ready_token(self.fixture.selection.store), asyncio.get_running_loop().time() + body['budget_ms'] / 1000)

    async def test_exact_owned_selection_one_actual_http_request_without_runtime_context(self):
        transport = self.transport()
        try:
            result = await self.score(transport)
        finally:
            await transport.close()
        self.assertEqual(result['scores'][0]['employee_id'], 'ada')
        self.assertEqual(len(self.calls), 1)
        request = self.calls[0]
        self.assertEqual(str(request.url), ENDPOINT)
        body = json.loads(request.content)
        self.assertEqual(body['model'], 'gpt-5.6-sol')
        self.assertEqual(body['reasoning'], {'effort': 'high'})
        self.assertEqual(body['tools'], [])
        self.assertIs(body['store'], False)
        self.assertNotIn('refresh-only', request.content.decode())
        for private in ('credential://', self.fixture.company, 'work_item_id', 'memory_context'):
            self.assertNotIn(private, request.content.decode())
        self.assertTrue(self.streams[0].closed)

    async def test_tools_incomplete_foreign_model_duplicate_scores_and_oversize_fail_closed(self):
        events = [completed(tool=True)]
        for field, value in [('status', 'incomplete'), ('model', 'foreign')]:
            event = completed(); event['response'][field] = value; events.append(event)
        event = completed()
        event['response']['output'][0]['content'][0]['text'] = '{"scores":[],"scores":[]}'
        events.append(event)
        for event in events:
            transport = self.transport(event)
            try:
                with self.assertRaises(BridgeError):
                    await self.score(transport)
            finally:
                await transport.close()
        transport = self.transport(chunks=[b':' + b'x' * 65536])
        try:
            with self.assertRaises(BridgeError):
                await self.score(transport)
        finally:
            await transport.close()
        self.assertEqual(len(self.calls), 5)
        self.assertTrue(all(stream.closed for stream in self.streams))

    async def test_missing_content_type_accepts_complete_responses_sse(self):
        # Real observed compatibility shape: HTTP200 without either header,
        # response.created/in_progress SSE before the final validated response.
        data = (b'event: response.created\ndata: {"type":"response.created"}\n\n'
                b'event: response.in_progress\ndata: {"type":"response.in_progress"}\n\n'
                b'event: response.completed\ndata: ' + json.dumps(completed()).encode() + b'\n\n')
        transport = self.transport(headers={}, chunks=[data[:37], data[37:]])
        try:
            result = await self.score(transport)
        finally:
            await transport.close()
        self.assertEqual(result['scores'], [{'employee_id': 'ada', 'score': 0.9, 'evidence': 'domain_match'}])
        self.assertEqual(len(self.calls), 1)
        self.assertTrue(self.streams[0].read)
        self.assertTrue(self.streams[0].closed)

    async def test_missing_content_type_html_malformed_or_incomplete_stays_refused(self):
        for body in (b'<!doctype html><html>Not a provider response</html>\n\n',
                     b'data: {not-json}\n\n',
                     b'event: response.created\ndata: {"type":"response.created"}\n\n'):
            transport = self.transport(headers={}, chunks=[body])
            try:
                with self.assertRaises(BridgeError):
                    await self.score(transport)
            finally:
                await transport.close()
        self.assertEqual(len(self.calls), 3)
        self.assertTrue(all(stream.read and stream.closed for stream in self.streams))

    async def test_explicit_wrong_media_type_or_encoding_refuses_before_body_read(self):
        for headers in ({'content-type': 'text/html'}, {'content-type': 'application/json'},
                        {'content-type': ''}, {'content-encoding': 'gzip'},
                        {'content-type': 'text/event-stream', 'content-encoding': 'br'}):
            # A valid SSE body cannot rescue explicitly incompatible metadata.
            transport = self.transport(headers=headers)
            try:
                with self.assertRaisesRegex(BridgeError, 'semantic_provider_protocol'):
                    await self.score(transport)
            finally:
                await transport.close()
        self.assertEqual(len(self.calls), 5)
        self.assertTrue(all(not stream.read and stream.closed for stream in self.streams))

    async def test_shared_deadline_closes_dribbling_stream_without_retry(self):
        transport = self.transport(delay=0.02, chunks=[b': ping\n\n'] * 30)
        started = asyncio.get_running_loop().time()
        try:
            with self.assertRaises(TimeoutError):
                await self.score(transport, self.fixture.request(55))
        finally:
            await transport.close()
        self.assertLess(asyncio.get_running_loop().time() - started, 0.3)
        self.assertEqual(len(self.calls), 1)
        self.assertTrue(self.streams[0].closed)

    async def test_listener_rejects_forgery_and_bounds_capacity_then_disconnect_releases_slot(self):
        transport = self.transport(delay=0.3)
        listener = Listener(self.fixture.selection, TOKEN, transport)
        server = await listener.start('127.0.0.1', 0)
        port = server.sockets[0].getsockname()[1]
        clients = []
        async def send(body, token=TOKEN):
            reader, writer = await asyncio.open_connection('127.0.0.1', port)
            clients.append(writer)
            data = json.dumps(body).encode()
            writer.write(f'POST /v1/semantic/score HTTP/1.1\r\nContent-Type: application/json\r\nAuthorization: Bearer {token}\r\nContent-Length: {len(data)}\r\n\r\n'.encode() + data)
            await writer.drain()
            return reader, writer
        try:
            reader, writer = await asyncio.open_connection('127.0.0.1', port)
            clients.append(writer)
            with patch('ortak_hermes_bridge.semantic.listener.ready_token', side_effect=AssertionError('status must not read tokens')):
                writer.write(f'GET /v1/semantic/status HTTP/1.1\r\nAuthorization: Bearer {TOKEN}\r\n\r\n'.encode())
                await writer.drain()
                status = await reader.read()
            self.assertIn(b' 200 ', status)
            self.assertNotIn(b'healthy', status)
            self.assertEqual(self.calls, [])
            reader, _ = await send(self.fixture.request(), 'wrong')
            self.assertIn(b' 401 ', await reader.read())
            forged = self.fixture.request(); forged['binding_sha256'] = 'f' * 64
            reader, _ = await send(forged)
            self.assertIn(b' 422 ', await reader.read())
            self.assertEqual(self.calls, [])
            first = await send(self.fixture.request())
            second = await send(self.fixture.request())
            async with asyncio.timeout(1):
                while len(self.calls) != 2:
                    await asyncio.sleep(0.001)
            reader, _ = await send(self.fixture.request())
            self.assertIn(b' 503 ', await reader.read())
            self.assertEqual(len(self.calls), 2)
            first[1].close(); await first[1].wait_closed()
            async with asyncio.timeout(1):
                while listener.scoring != 1:
                    await asyncio.sleep(0.001)
            self.assertTrue(self.streams[0].closed)
            self.assertIn(b' 200 ', await second[0].read())
        finally:
            for writer in clients:
                writer.close()
            await listener.close()
        self.assertEqual(listener.scoring, 0)
        self.assertEqual(listener.tasks, set())

    async def test_actual_http_deadline_returns_timeout_and_closes_provider_stream(self):
        transport = self.transport(delay=0.3)
        listener = Listener(self.fixture.selection, TOKEN, transport)
        server = await listener.start('127.0.0.1', 0)
        reader, writer = await asyncio.open_connection('127.0.0.1', server.sockets[0].getsockname()[1])
        body = json.dumps(self.fixture.request(30)).encode()
        writer.write(f'POST /v1/semantic/score HTTP/1.1\r\nContent-Type: application/json\r\nAuthorization: Bearer {TOKEN}\r\nContent-Length: {len(body)}\r\n\r\n'.encode() + body)
        await writer.drain()
        try:
            response = await reader.read()
            self.assertIn(b' 408 ', response)
            self.assertIn(b'semantic_timeout', response)
        finally:
            writer.close()
            await listener.close()
        self.assertEqual(len(self.calls), 1)
        self.assertTrue(self.streams[0].closed)
        self.assertEqual(listener.scoring, 0)

    async def test_ready_read_never_refreshes_and_maintenance_uses_retained_rotation_fence(self):
        store = self.fixture.selection.store
        driver = Mock(); store.driver = driver
        state = store.read(); state['tokens']['access_token'] = access(10)
        atomic_write(store.directory / STATE, state)
        with self.assertRaises(BridgeError):
            ready_token(store)
        driver.call.assert_not_called()
        def refresh(action, payload):
            self.assertEqual(store.read()['phase'], 'refreshing')
            return {'access_token': access(7200), 'refresh_token': 'fixture-rotated-refresh-token'}
        driver.call.side_effect = refresh
        maintenance = Maintenance(store); maintenance.start()
        try:
            async with asyncio.timeout(1):
                while maintenance.status != 'ready':
                    await asyncio.sleep(0.005)
        finally:
            maintenance.close()
        self.assertEqual(store.read()['generation'], 2)
        self.assertTrue(ready_token(store))
        driver.call.assert_called_once()
        self.assertEqual(maintenance.status, 'stopped')

    async def test_duplicate_control_fields_invalid_budget_and_static_variant_refused(self):
        for raw in ('{"scores":[],"scores":[]}', '{"scores":NaN}', '{"scores":Infinity}'):
            with self.assertRaises(BridgeError):
                strict_json(raw)
        for budget in (0, 4501, True):
            with self.assertRaises(BridgeError):
                self.fixture.selection.request(self.fixture.request(budget))
        config = json.loads(json.dumps(self.fixture.config))
        config['semantic']['binding_sha256'] = 'f' * 64
        config['profiles'][0]['oauth_directory'] = '/no/credential/lookup/permitted'
        with self.assertRaisesRegex(BridgeError, 'semantic_profile_not_found'):
            Selection(config)


if __name__ == '__main__':
    unittest.main()
