"""Installed Python listener ownership; only exact owned loopback is permitted."""
import asyncio
import json
from unittest.mock import AsyncMock, patch

import httpx

from ortak_hermes_bridge.semantic import __main__ as entry
from ortak_hermes_bridge.semantic.credentials import Maintenance
from ortak_hermes_bridge.semantic.listener import Listener
from ortak_hermes_bridge.semantic.transport import CodexTransport

TOKEN = 'synthetic-installed-private-listener-key'


async def check(selected, body, permitted):
    streams = []
    class Stream(httpx.AsyncByteStream):
        closed = False
        async def __aiter__(self):
            await asyncio.sleep(2)
            yield b': synthetic wait\n\n'
        async def aclose(self):
            self.closed = True
    async def wire(request):
        stream = Stream(); streams.append(stream)
        return httpx.Response(200, headers={'content-type': 'text/event-stream'}, stream=stream)
    def transport():
        # Real installed helpers and HTTPX path; no provider socket is opened.
        return CodexTransport(client=httpx.AsyncClient(transport=httpx.MockTransport(wire)))
    listener = Listener(selected, TOKEN, transport())
    server = await listener.start('127.0.0.1', 0)
    address = ('127.0.0.1', server.sockets[0].getsockname()[1])
    permitted.add(address)
    clients = []
    try:
        for _ in range(4):
            clients.append(await asyncio.open_connection(*address))
        async with asyncio.timeout(1):
            while len(listener.tasks) != 4:
                await asyncio.sleep(0.001)
        owners = set(listener.tasks)
        blocked = asyncio.Event()
        async def stalled_write(*args):
            await blocked.wait()
        with patch.object(listener, 'write', AsyncMock(side_effect=stalled_write)) as write:
            for _ in range(20):
                reader, writer = await asyncio.open_connection(*address)
                clients.append((reader, writer))
                async with asyncio.timeout(0.2):
                    assert await reader.read() == b''
                assert listener.tasks == owners
            write.assert_not_called()
        # A real active score must be cancelled before Python 3.13 wait_closed.
        data = json.dumps({**body, 'budget_ms': 1000}).encode()
        clients[0][1].write(f'POST /v1/semantic/score HTTP/1.1\r\nContent-Type: application/json\r\nAuthorization: Bearer {TOKEN}\r\nContent-Length: {len(data)}\r\n\r\n'.encode() + data)
        await clients[0][1].drain()
        async with asyncio.timeout(1):
            while not streams:
                await asyncio.sleep(0.001)
        async with asyncio.timeout(0.5):
            await listener.close()
        assert listener.scoring == 0 and not listener.tasks and streams[0].closed
    finally:
        for _, writer in clients:
            writer.close()
        await listener.close()
        permitted.discard(address)

    owned_transport = transport()
    maintenance = Maintenance(selected.store)
    listener = Listener(selected, TOKEN, owned_transport, maintenance)
    original_start, original_close = listener.start, owned_transport.close
    started = asyncio.Event()
    async def start(host, port):
        await original_start(host, port)
        started.set()
    async def failed_close():
        await original_close()
        raise RuntimeError('synthetic close failure')
    with patch.object(entry, 'CodexTransport', return_value=owned_transport), \
            patch.object(entry, 'Maintenance', return_value=maintenance), \
            patch.object(entry, 'Listener', return_value=listener), \
            patch.object(asyncio.get_running_loop(), 'add_signal_handler'), \
            patch.object(listener, 'start', start), \
            patch.object(owned_transport, 'close', failed_close):
        owner = asyncio.create_task(entry.serve(selected, TOKEN, '127.0.0.1', 0))
        try:
            async with asyncio.timeout(1):
                await started.wait()
            owner.cancel()
            try:
                await owner
            except RuntimeError as error:
                assert str(error) == 'synthetic close failure'
            else:
                raise AssertionError('cleanup failure swallowed')
            assert not maintenance.thread.is_alive() and maintenance.status == 'stopped'
        finally:
            if not owner.done():
                owner.cancel()
                await asyncio.gather(owner, return_exceptions=True)
            maintenance.close()
    # Controlled no-read socket binds the production handler's graceful-close
    # timeout. Real TCP timing must not make this containment assertion flaky.
    release = asyncio.Event()
    class StalledWriter:
        aborted = False
        def __init__(self):
            self.transport = self
        def write(self, data):
            pass
        async def drain(self):
            pass
        def close(self):
            pass
        async def wait_closed(self):
            await release.wait()
        def abort(self):
            self.aborted = True
            release.set()
    reader = asyncio.StreamReader()
    reader.feed_data(b'GET /unknown HTTP/1.1\r\n\r\n')
    writer = StalledWriter()
    listener = Listener(selected, TOKEN, transport())
    listener.accept(reader, writer)
    try:
        async with asyncio.timeout(0.5):
            results = await asyncio.gather(*tuple(listener.tasks), return_exceptions=True)
        assert writer.aborted and isinstance(results[0], TimeoutError) and not listener.tasks
    finally:
        release.set()
        await listener.close()
    return 4
