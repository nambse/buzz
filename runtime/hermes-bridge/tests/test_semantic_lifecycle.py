"""Owned listener admission and process-entry cleanup, including failure paths."""
import asyncio
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import AsyncMock, patch

import httpx

from ortak_hermes_bridge.semantic import __main__ as entry
from ortak_hermes_bridge.semantic.credentials import Maintenance
from ortak_hermes_bridge.semantic.listener import Listener
from ortak_hermes_bridge.semantic.transport import CodexTransport
from test_semantic import Fixture, Stream, TOKEN, completed, local_helpers


class Lifecycle(unittest.IsolatedAsyncioTestCase):
    async def asyncSetUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.fixture = Fixture(Path(self.temporary.name).resolve())
        self.streams = []

    def transport(self):
        async def wire(request):
            stream = Stream([b'data: ' + json.dumps(completed()).encode() + b'\n\n'], delay=1)
            self.streams.append(stream)
            return httpx.Response(200, headers={'content-type': 'text/event-stream'}, stream=stream)
        return CodexTransport(client=httpx.AsyncClient(transport=httpx.MockTransport(wire)),
            helpers=local_helpers())

    async def test_owned_handler_aborts_exact_socket_after_graceful_close_deadline(self):
        reader = asyncio.StreamReader()
        reader.feed_data(b'GET /unknown HTTP/1.1\r\n\r\n')
        release = asyncio.Event()
        class StalledWriter:
            def __init__(self):
                self.transport = self
                self.aborted = False
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
        writer = StalledWriter()
        listener = Listener(self.fixture.selection, TOKEN, self.transport())
        listener.accept(reader, writer)
        owned = tuple(listener.tasks)
        try:
            async with asyncio.timeout(0.5):
                results = await asyncio.gather(*owned, return_exceptions=True)
            self.assertTrue(writer.aborted)
            self.assertIsInstance(results[0], TimeoutError)
            self.assertEqual(listener.tasks, set())
            await listener.close()
        finally:
            release.set()
            await listener.close()

    async def test_shutdown_cancels_active_provider_before_waiting_for_connections(self):
        listener = Listener(self.fixture.selection, TOKEN, self.transport())
        server = await listener.start('127.0.0.1', 0)
        reader, writer = await asyncio.open_connection('127.0.0.1', server.sockets[0].getsockname()[1])
        data = json.dumps(self.fixture.request()).encode()
        writer.write(f'POST /v1/semantic/score HTTP/1.1\r\nContent-Type: application/json\r\nAuthorization: Bearer {TOKEN}\r\nContent-Length: {len(data)}\r\n\r\n'.encode() + data)
        await writer.drain()
        original_wait = server.wait_closed
        async def wait_for_connections():
            # Python 3.13's wait_closed waits for connected clients. Model that
            # documented boundary on older local Python without replacing HTTP.
            self.assertEqual(listener.scoring, 0)
            self.assertTrue(self.streams[0].closed)
            await original_wait()
        try:
            async with asyncio.timeout(1):
                while not self.streams:
                    await asyncio.sleep(0.001)
            with patch.object(server, 'wait_closed', wait_for_connections):
                await listener.close()
            self.assertEqual(await reader.read(), b'')
            self.assertEqual(listener.tasks, set())
        finally:
            writer.close()
            await listener.close()

    async def test_four_connection_admission_rejects_burst_without_writing_or_new_handlers(self):
        listener = Listener(self.fixture.selection, TOKEN, self.transport())
        server = await listener.start('127.0.0.1', 0)
        port = server.sockets[0].getsockname()[1]
        clients = []
        blocked = asyncio.Event()
        async def stalled_write(*args):
            await blocked.wait()
        try:
            for _ in range(4):
                clients.append(await asyncio.open_connection('127.0.0.1', port))
            async with asyncio.timeout(1):
                while len(listener.tasks) < 4:
                    await asyncio.sleep(0.001)
            owners = set(listener.tasks)
            with patch.object(listener, 'write', AsyncMock(side_effect=stalled_write)) as write:
                for _ in range(20):
                    reader, writer = await asyncio.open_connection('127.0.0.1', port)
                    clients.append((reader, writer))
                    async with asyncio.timeout(0.2):
                        self.assertEqual(await reader.read(), b'')
                    self.assertEqual(listener.tasks, owners)
                write.assert_not_called()
        finally:
            blocked.set()
            for _, writer in clients:
                writer.close()
            await listener.close()
        self.assertEqual(listener.tasks, set())

    async def test_production_serve_always_joins_real_maintenance_after_transport_cleanup_failure(self):
        transport = self.transport()
        maintenance = Maintenance(self.fixture.selection.store)
        listener = Listener(self.fixture.selection, TOKEN, transport, maintenance)
        started = asyncio.Event()
        original_start = listener.start
        original_close = transport.close
        async def start(host, port):
            result = await original_start(host, port)
            started.set()
            return result
        async def failed_close():
            await original_close()
            raise RuntimeError('synthetic close failure')
        loop = asyncio.get_running_loop()
        with patch.object(entry, 'CodexTransport', return_value=transport), \
                patch.object(entry, 'Maintenance', return_value=maintenance), \
                patch.object(entry, 'Listener', return_value=listener), \
                patch.object(loop, 'add_signal_handler'), \
                patch.object(listener, 'start', start), \
                patch.object(transport, 'close', failed_close):
            task = asyncio.create_task(entry.serve(self.fixture.selection, TOKEN, '127.0.0.1', 0))
            try:
                async with asyncio.timeout(1):
                    await started.wait()
                task.cancel()
                with self.assertRaisesRegex(RuntimeError, 'synthetic close failure'):
                    await task
                self.assertFalse(maintenance.thread.is_alive())
                self.assertTrue(maintenance.stopping.is_set())
                self.assertEqual(maintenance.status, 'stopped')
            finally:
                if not task.done():
                    task.cancel()
                    await asyncio.gather(task, return_exceptions=True)
                maintenance.close()
