"""Explicit extension schema initialization, after native Honcho migrations."""

import asyncio

from sqlalchemy import text
from src.db import Base, engine

from .models import TABLES


async def initialize():
    async with engine.begin() as connection:
        await connection.execute(text("SET LOCAL lock_timeout = '5s'"))
        await connection.execute(text("SET LOCAL statement_timeout = '30s'"))
        await connection.execute(
            text("SELECT pg_advisory_xact_lock(728896592459485701)")
        )
        await connection.run_sync(
            lambda sync: Base.metadata.create_all(sync, tables=TABLES)
        )
    await engine.dispose()


if __name__ == "__main__":
    asyncio.run(initialize())
