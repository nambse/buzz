"""Separate bounded private HTTP lane; employee run/cancel service is unaffected."""
import asyncio
import hmac
import json

from ..journal import BridgeError
from . import MAX_BYTES
from .contract import strict_json
from .credentials import ready_token


class Listener:
    """Four total HTTP connections and two immediate scoring slots, without a queue."""
    def __init__(self, selection, token, transport, maintenance=None):
        if (not isinstance(token, str) or not 32 <= len(token) <= 4096
                or any(not 33 <= ord(c) <= 126 for c in token)):
            raise BridgeError('invalid_service_credential', 422)
        self.selection, self.token, self.transport = selection, token, transport
        self.maintenance = maintenance
        self.tasks = set()
        self.scoring = 0
        self.server = None
        self.closing = False

    async def start(self, host, port):
        if host not in {'127.0.0.1', '0.0.0.0'}:
            raise BridgeError('invalid_listen_address', 422)
        self.server = await asyncio.start_server(self.accept, host, port, limit=8192, backlog=2)
        return self.server

    def accept(self, reader, writer):
        # Reserve synchronously: rejected sockets cannot create an untracked
        # coroutine waiting to write a busy response under connection pressure.
        if self.closing or len(self.tasks) >= 4:
            writer.close()
            return
        task = asyncio.create_task(self.handle(reader, writer))
        self.tasks.add(task)
        def retired(completed):
            # Also closes a task cancelled before its coroutine first ran.
            writer.close()
            self.tasks.discard(completed)
        task.add_done_callback(retired)

    async def close(self):
        self.closing = True
        if self.server is not None:
            self.server.close()
        tasks = tuple(self.tasks)
        for task in tasks:
            task.cancel()
        try:
            if tasks:
                await asyncio.gather(*tasks, return_exceptions=True)
            # Python 3.13 waits for active connections here. Their handlers
            # must first be cancelled and joined, including upstream cleanup.
            if self.server is not None:
                await self.server.wait_closed()
        finally:
            await self.transport.close()

    async def write(self, writer, status, body):
        data = json.dumps(body, separators=(',', ':'), allow_nan=False).encode()
        if len(data) > MAX_BYTES:
            raise BridgeError('semantic_response_bounds', 503)
        header = (f'HTTP/1.1 {status} Result\r\nContent-Type: application/json\r\n'
            f'Content-Length: {len(data)}\r\nConnection: close\r\nCache-Control: no-store\r\n\r\n').encode()
        writer.write(header + data)
        async with asyncio.timeout(0.1):
            await writer.drain()

    async def request(self, reader):
        async with asyncio.timeout(1):
            raw = await reader.readuntil(b'\r\n\r\n')
            if len(raw) > 8192:
                raise BridgeError('invalid_http', 400)
            lines = raw.decode('ascii').split('\r\n')
            status_request = lines[0] == 'GET /v1/semantic/status HTTP/1.1'
            if len(lines) > 64 or lines[0] not in {
                    'POST /v1/semantic/score HTTP/1.1', 'GET /v1/semantic/status HTTP/1.1'}:
                raise BridgeError('not_found', 404)
            headers = {}
            for line in lines[1:-2]:
                if ':' not in line or line[:1].isspace():
                    raise BridgeError('invalid_http', 400)
                name, value = line.split(':', 1)
                name = name.lower()
                if not name or name in headers:
                    raise BridgeError('invalid_http', 400)
                headers[name] = value.strip()
            authorization = headers.get('authorization', '').encode()
            if not hmac.compare_digest(authorization, ('Bearer ' + self.token).encode()):
                raise BridgeError('unauthorized', 401)
            if status_request:
                if 'transfer-encoding' in headers or headers.get('content-length', '0') != '0':
                    raise BridgeError('invalid_http', 400)
                return None
            if ('transfer-encoding' in headers or headers.get('content-type') != 'application/json'
                    or not headers.get('content-length', '').isascii()
                    or not headers.get('content-length', '').isdecimal()):
                raise BridgeError('invalid_http', 400)
            size = int(headers['content-length'])
            if not 1 <= size <= MAX_BYTES:
                raise BridgeError('semantic_bounds', 413)
            return strict_json(await reader.readexactly(size))

    async def handle(self, reader, writer):
        operation = disconnect = None
        acquired = False
        try:
            body = await self.request(reader)
            if body is None:
                await self.write(writer, 200, {'deployment_id': self.selection.deployment_id,
                    'binding_sha256': self.selection.binding_sha256, 'accepting': not self.closing,
                    'active_scores': self.scoring,
                    'last_maintenance_status': self.maintenance.status if self.maintenance is not None else 'unconfigured'})
                return
            expected = self.selection.request(body)
            if self.scoring >= 2:
                raise BridgeError('semantic_busy', 503)
            self.scoring += 1
            acquired = True
            deadline = asyncio.get_running_loop().time() + body['budget_ms'] / 1000
            # A nonblocking local lock/read cannot start refresh or wait behind it.
            token = ready_token(self.selection.store)
            operation = asyncio.create_task(self.transport.score(self.selection, body, expected, token, deadline))
            disconnect = asyncio.create_task(reader.read(1))
            done, _ = await asyncio.wait((operation, disconnect), return_when=asyncio.FIRST_COMPLETED)
            if disconnect in done:
                # EOF or pipelined bytes both retire this exact request.
                raise asyncio.CancelledError()
            result = operation.result()
            if asyncio.get_running_loop().time() >= deadline:
                raise TimeoutError()
            await self.write(writer, 200, result)
        except asyncio.CancelledError:
            raise
        except TimeoutError:
            try:
                await self.write(writer, 408, {'error': 'semantic_timeout'})
            except Exception:
                pass
        except BridgeError as error:
            try:
                await self.write(writer, error.status, {'error': error.code})
            except Exception:
                pass
        except Exception:
            try:
                await self.write(writer, 503, {'error': 'semantic_unavailable'})
            except Exception:
                pass
        finally:
            pending = [item for item in (operation, disconnect) if item is not None]
            for item in pending:
                if not item.done():
                    item.cancel()
            if pending:
                await asyncio.gather(*pending, return_exceptions=True)
            if acquired:
                self.scoring -= 1
            writer.close()
            try:
                async with asyncio.timeout(0.1):
                    await writer.wait_closed()
            except BaseException:
                # Graceful close may keep buffered output alive for a peer
                # that never reads. Retire this exact socket before its owner
                # leaves the bounded task set or Server.wait_closed can hang.
                writer.transport.abort()
                raise
