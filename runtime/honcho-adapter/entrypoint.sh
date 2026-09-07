#!/bin/sh
set -eu
/app/.venv/bin/python scripts/provision_db.py
/app/.venv/bin/python -m ortak_honcho.init_db
exec /app/.venv/bin/uvicorn ortak_honcho.app:app --host 0.0.0.0 --port 8000
