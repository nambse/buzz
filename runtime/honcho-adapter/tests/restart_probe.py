"""One request from a fresh interpreter, proving replay needs no process cache."""

import asyncio
import json
import sys

from httpx import ASGITransport, AsyncClient
from ortak_honcho.app import app
from src.db import engine
from src.security import create_admin_jwt


async def main():
    raw = sys.stdin.buffer.read(1152 * 1024 + 1)
    if len(raw) > 1152 * 1024:
        raise ValueError("bounded restart probe input exceeded")
    request = json.loads(raw)
    async with AsyncClient(
        transport=ASGITransport(app=app),
        base_url="http://honcho.test",
        headers={"Authorization": "Bearer " + create_admin_jwt()},
    ) as client:
        response = await client.post(request["path"], json=request["body"])
        print(json.dumps({"status": response.status_code, "body": response.json()}))
    await engine.dispose()


if __name__ == "__main__":
    asyncio.run(main())
