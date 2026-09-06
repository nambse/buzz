"""Installed-image scoring gate: real pinned Hermes helpers and HTTPX transport.

Only the socket transport returns synthetic Responses SSE. No AIAgent, provider
call, account entitlement or relevance-quality acceptance is implied.
"""
import asyncio
import base64
import hashlib
import json
from pathlib import Path
import sys
import tempfile
import time
from uuid import uuid4

from ortak_hermes_bridge import HERMES_REVISION
from ortak_hermes_bridge.journal import BridgeError
from ortak_hermes_bridge.oauth_credentials import OAuthStore, oauth_identity
from ortak_hermes_bridge.semantic import PROMPT_VERSION, SCHEMA_VERSION
from ortak_hermes_bridge.semantic.contract import Selection
from ortak_hermes_bridge.semantic.credentials import ready_token
from ortak_hermes_bridge.semantic.transport import CodexTransport, ENDPOINT
from ortak_hermes_bridge.worker import arm_deadline, prepare_home


async def check(root, permitted):
    import httpx
    counts = []
    variants = []
    for index, (model, effort, failure) in enumerate((
        ('gpt-5.6-sol', 'high', None), ('gpt-6-astra', 'max', None),
        ('gpt-5.6-sol', 'low', 'missing_content_type'),
        ('gpt-5.6-sol', 'high', 'tool'), ('gpt-5.6-sol', 'high', 'auth'),
        ('gpt-5.6-sol', 'high', 'timeout'))):
        company, deployment = str(uuid4()), str(uuid4())
        binding = {'adapter': 'hermes', 'profile_ref': f'semantic-check-{index}',
            'workspace': '/synthetic-only', 'credential_refs': ['credential://semantic/check'],
            'model': model, 'options': {'reasoning_effort': effort}}
        store = OAuthStore.create(root / f'oauth-{index}', oauth_identity(company, 'fixture', binding))
        claims = {'exp': int(time.time()) + 3600,
            'https://api.openai.com/auth': {'chatgpt_account_id': 'synthetic-semantic-account'}}
        token = 'fixture.' + base64.urlsafe_b64encode(json.dumps(claims).encode()).decode().rstrip('=') + '.fixture'
        store.enroll(lambda: {'tokens': {'access_token': token, 'refresh_token': 'fixture-refresh-never-used'}})
        selected = Selection({'company_id': company,
            'profiles': [{'employee_id': 'fixture', 'binding': binding, 'oauth_directory': str(store.directory)}],
            'semantic': {'deployment_id': deployment, 'response_model': model,
                'binding_sha256': hashlib.sha256(json.dumps(binding, sort_keys=True, separators=(',', ':')).encode()).hexdigest()}})
        body = {'deployment_id': deployment, 'binding_sha256': selected.binding_sha256,
            'request_id': str(uuid4()), 'prompt_version': PROMPT_VERSION, 'schema_version': SCHEMA_VERSION,
            'budget_ms': 70 if failure == 'timeout' else 1000,
            'input': {'message': 'Deployment plan', 'candidates': [{'employee_id': 'ada', 'name': 'Ada',
                'title': 'Engineer', 'biography': '', 'responsibilities': ['Deployment'], 'domains': ['Engineering']}]}}
        calls, streams = [], []
        class Stream(httpx.AsyncByteStream):
            closed = False
            async def __aiter__(self):
                if failure == 'timeout':
                    await asyncio.sleep(1)
                score_text = json.dumps({'scores': [{'employee_id': 'ada', 'score': 0.9, 'evidence': 'domain_match'}]})
                item = ({'type': 'function_call', 'name': 'terminal', 'arguments': '{}'} if failure == 'tool' else
                    {'type': 'message', 'id': 'msg_fixture', 'role': 'assistant', 'status': 'completed', 'phase': 'final_answer',
                     'content': [{'type': 'output_text', 'text': score_text}]})
                event = {'type': 'response.completed', 'response': {'status': 'completed', 'model': model,
                    'id': 'resp_fixture', 'output': [item], 'usage': {'input_tokens': 20, 'output_tokens': 20, 'total_tokens': 40}}}
                yield b'data: ' + json.dumps(event).encode() + b'\n\n'
            async def aclose(self):
                self.closed = True
        async def wire(request):
            if calls:
                raise RuntimeError('synthetic scoring retried provider request')
            calls.append(True)
            actual = json.loads(request.content)
            if (str(request.url) != ENDPOINT or actual['model'] != model
                    or actual['reasoning'] != {'effort': effort} or actual['tools'] != []
                    or actual['store'] is not False or actual['stream'] is not True
                    or request.headers.get('originator') != 'hermes-agent'
                    or request.headers.get('chatgpt-account-id') != 'synthetic-semantic-account'
                    or request.headers.get('authorization') != 'Bearer ' + token):
                raise RuntimeError('actual selected Codex request changed')
            if failure == 'auth':
                return httpx.Response(401, json={'error': 'synthetic-only'})
            stream = Stream(); streams.append(stream)
            headers = {} if failure == 'missing_content_type' else {'content-type': 'text/event-stream'}
            return httpx.Response(200, headers=headers, stream=stream)
        transport = CodexTransport(client=httpx.AsyncClient(transport=httpx.MockTransport(wire),
            trust_env=False, follow_redirects=False))
        try:
            result = await transport.score(selected, body, selected.request(body), ready_token(store),
                asyncio.get_running_loop().time() + body['budget_ms'] / 1000)
            if failure not in {None, 'missing_content_type'} or result['scores'][0]['score'] != 0.9:
                raise RuntimeError('unexpected synthetic score outcome')
            variants.append({'model': model, 'effort': effort})
        except (BridgeError, TimeoutError):
            if failure in {None, 'missing_content_type'}:
                raise
        finally:
            await transport.close()
        if len(calls) != 1 or any(not stream.closed for stream in streams):
            raise RuntimeError('provider I/O was not single and closed')
        counts.append(len(calls))
    from semantic_lifecycle_check import check as lifecycle_check
    lifecycle_cases = await lifecycle_check(selected, body, permitted)
    return {'source_revision': HERMES_REVISION, 'selected_variants': variants,
        'fixture_requests': sum(counts), 'cases': len(counts), 'provider_requests': 0,
        'lifecycle_cases': lifecycle_cases,
        'scope': 'installed pinned format/header/normalization with synthetic HTTPX socket transport; not provider acceptance'}


def main():
    arm_deadline(30)
    attempts = []
    permitted = set()
    def audit(event, args):
        if event == 'socket.connect' and args[1] in permitted:
            return
        if event in {'socket.connect', 'socket.getaddrinfo', 'subprocess.Popen', 'os.system',
                     'os.exec', 'os.posix_spawn', 'os.fork'}:
            attempts.append(event)
            raise RuntimeError('installed semantic gate attempted external I/O')
    sys.addaudithook(audit)
    with tempfile.TemporaryDirectory(prefix='ortak-semantic-gate-') as temporary:
        root = Path(temporary)
        prepare_home(root / 'home')
        result = asyncio.run(check(root, permitted))
    if attempts:
        raise RuntimeError('installed semantic gate attempted external I/O')
    print(json.dumps(result, sort_keys=True))


if __name__ == '__main__':
    main()
