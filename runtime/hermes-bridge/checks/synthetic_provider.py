"""Deterministic OpenAI Responses fixture; never a model or credential service."""
import json
import os
import signal
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

TOKEN = 'ortak-synthetic-not-a-provider-credential'
ANSWER = 'Synthetic bridge fixture answer.'
COUNTS = {'normal': 0, 'tool': 0, 'slow': 0, 'metadata': 0, 'invalid': 0}
LOCK = threading.Lock()
ERRORS = []


def response(tool=False):
    """A fixed Responses object parsed by the real pinned OpenAI SDK."""
    item = ({'type': 'function_call', 'id': 'fc_synthetic', 'call_id': 'call_synthetic',
             'status': 'completed', 'name': 'terminal', 'arguments': '{"command":"never execute"}'} if tool else
            {'type': 'message', 'id': 'msg_synthetic', 'role': 'assistant', 'status': 'completed',
             'content': [{'type': 'output_text', 'text': ANSWER, 'annotations': []}]})
    return {'id': 'resp_synthetic', 'object': 'response', 'created_at': 1788585600,
            'status': 'completed', 'model': 'gpt-4o-mini', 'output': [item], 'error': None,
            'incomplete_details': None, 'usage': {'input_tokens': 1, 'output_tokens': 1, 'total_tokens': 2}}


def events(value):
    """Emit both content and completion events used by Responses streaming."""
    item = value['output'][0]
    initial = {**value, 'status': 'in_progress', 'output': [], 'usage': None}
    yield {'type': 'response.created', 'response': initial}
    if item['type'] == 'function_call':
        yield {'type': 'response.output_item.added', 'output_index': 0, 'item': {**item, 'arguments': '', 'status': 'in_progress'}}
        yield {'type': 'response.function_call_arguments.delta', 'item_id': item['id'], 'output_index': 0, 'delta': item['arguments']}
        yield {'type': 'response.function_call_arguments.done', 'item_id': item['id'], 'output_index': 0, 'arguments': item['arguments']}
    else:
        part = item['content'][0]
        yield {'type': 'response.output_item.added', 'output_index': 0, 'item': {**item, 'content': [], 'status': 'in_progress'}}
        yield {'type': 'response.content_part.added', 'item_id': item['id'], 'output_index': 0, 'content_index': 0,
               'part': {**part, 'text': ''}}
        yield {'type': 'response.output_text.delta', 'item_id': item['id'], 'output_index': 0, 'content_index': 0, 'delta': ANSWER}
        yield {'type': 'response.output_text.done', 'item_id': item['id'], 'output_index': 0, 'content_index': 0, 'text': ANSWER}
        yield {'type': 'response.content_part.done', 'item_id': item['id'], 'output_index': 0, 'content_index': 0, 'part': part}
    yield {'type': 'response.output_item.done', 'output_index': 0, 'item': item}
    yield {'type': 'response.completed', 'response': value}


class Handler(BaseHTTPRequestHandler):
    """Bounded fixture endpoint with public counters and no request-content logs."""
    def log_message(self, *args): pass

    def setup(self):
        super().setup()
        self.connection.settimeout(5)

    def send_error(self, code, message=None, explain=None):
        # BaseHTTPRequestHandler otherwise answers unsupported methods without
        # entering do_POST, silently hiding discovery GETs from traffic evidence.
        with LOCK:
            COUNTS['invalid'] += 1
            ERRORS.append({'method': getattr(self, 'command', '')[:16], 'path': getattr(self, 'path', '')[:100], 'status': code})
            Path('/fixture/invalid-methods.json').write_text(json.dumps(ERRORS[:8]))
            Path('/fixture/counts.json').write_text(json.dumps(COUNTS))
        return super().send_error(code, message, explain)

    def do_GET(self):
        # Hermes' optional model-catalog lookup is deliberately unavailable.
        # A counted 404 is neither fabricated catalog data nor a healthy report.
        if self.path not in {'/v1/models', '/models'} or self.headers.get_all('Authorization') != ['Bearer ' + TOKEN]:
            self.send_error(404)
            return
        with LOCK:
            COUNTS['metadata'] += 1
            Path('/fixture/counts.json').write_text(json.dumps(COUNTS))
        self.send_response(404)
        self.send_header('Content-Length', '0')
        self.send_header('Connection', 'close')
        self.end_headers()

    def do_POST(self):
        category = 'invalid'
        try:
            lengths = self.headers.get_all('Content-Length', [])
            size = int(lengths[0]) if len(lengths) == 1 else -1
            if (self.path != '/v1/responses' or self.headers.get('Transfer-Encoding') is not None
                    or self.headers.get_all('Authorization') != ['Bearer ' + TOKEN] or not 0 < size <= 262144):
                raise ValueError()
            raw = self.rfile.read(size)
            body = json.loads(raw)
            if body.get('model') != 'gpt-4o-mini' or body.get('tools') not in (None, []):
                raise ValueError()
            category = 'tool' if b'ORTAK_SYNTHETIC_TOOL' in raw else 'slow' if b'ORTAK_SYNTHETIC_SLOW' in raw else 'normal'
            with LOCK:
                COUNTS[category] += 1
                Path('/fixture/counts.json').write_text(json.dumps(COUNTS))
                if sum(COUNTS.values()) > 8: raise ValueError()
            if category == 'slow': time.sleep(30)
            value = response(category == 'tool')
            if body.get('stream'):
                chunks = []
                for sequence, event in enumerate(events(value)):
                    event['sequence_number'] = sequence
                    chunks.append(f"event: {event['type']}\ndata: {json.dumps(event)}\n\n")
                payload = ''.join(chunks).encode()
                content_type = 'text/event-stream'
            else:
                payload = json.dumps(value).encode()
                content_type = 'application/json'
            self.send_response(200)
            self.send_header('Content-Type', content_type)
            self.send_header('Content-Length', str(len(payload)))
            self.send_header('Connection', 'close')
            self.end_headers()
            self.wfile.write(payload)
        except (OSError, ValueError, KeyError, TypeError):
            if category == 'invalid':
                with LOCK:
                    COUNTS['invalid'] += 1
                    Path('/fixture/invalid.json').write_text(json.dumps({'path': self.path[:100], 'size': size, 'authorized': self.headers.get_all('Authorization') == ['Bearer ' + TOKEN]}))
                    Path('/fixture/counts.json').write_text(json.dumps(COUNTS))
            self.close_connection = True


def main():
    """The test owns this whole container and the kernel ends it after240s."""
    os.umask(0o077)
    os.environ.clear()
    os.environ.update(PATH='/usr/local/bin:/usr/bin:/bin', HOME='/tmp')
    if os.getpid() == 1: raise RuntimeError('init required')
    signal.signal(signal.SIGALRM, signal.SIG_DFL)
    signal.alarm(240)
    Path('/fixture/counts.json').write_text(json.dumps(COUNTS))
    server = ThreadingHTTPServer(('0.0.0.0', 8080), Handler)
    server.request_queue_size = 4
    Path('/fixture/ready').write_text('ready')
    server.serve_forever(poll_interval=0.2)


if __name__ == '__main__': main()
