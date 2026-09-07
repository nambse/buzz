"""Initialize only an explicit disposable Honcho DB, then run bounded tests."""

import os
import subprocess
import sys
from urllib.parse import urlparse
from uuid import uuid4

url = os.environ.get("ORTAK_HONCHO_TEST_DATABASE_URL", "")
parsed = urlparse(url)
if parsed.hostname not in {
    "127.0.0.1",
    "localhost",
    "host.docker.internal",
    "honcho-test-db",
} or not parsed.path.startswith("/ortak_honcho_"):
    raise SystemExit(
        "Set explicit local ORTAK_HONCHO_TEST_DATABASE_URL with database name ortak_honcho_*"
    )
os.environ.update(
    {
        "DB_CONNECTION_URI": url.replace(
            "postgres://", "postgresql+psycopg://", 1
        ).replace("postgresql://", "postgresql+psycopg://", 1),
        "AUTH_USE_AUTH": "true",
        "AUTH_JWT_SECRET": uuid4().hex + uuid4().hex,
        "LLM_OPENAI_API_KEY": "test-only-" + uuid4().hex,
        "CACHE_ENABLED": "false",
        "EMBED_MESSAGES": "false",
        "METRICS_ENABLED": "false",
        "TELEMETRY_ENABLED": "false",
        "SENTRY_ENABLED": "false",
    }
)
subprocess.run([sys.executable, "scripts/provision_db.py"], check=True, timeout=120)
subprocess.run(
    [sys.executable, "-m", "pytest", "-q", "-o", "addopts=", "ortak_tests"],
    check=True,
    timeout=120,
)
