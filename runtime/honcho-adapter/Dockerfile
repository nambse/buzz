# syntax=docker/dockerfile:1
# Use digest-valued build args and record the final digest for deployment.
ARG BASE_IMAGE
FROM ${BASE_IMAGE} AS builder
WORKDIR /app
ENV UV_PYTHON=/usr/local/bin/python UV_LINK_MODE=copy PYTHONDONTWRITEBYTECODE=1 PYTHONUNBUFFERED=1 TIKTOKEN_CACHE_DIR=/app/tiktoken-cache
COPY vendor/uv.lock vendor/pyproject.toml ./
RUN --mount=type=cache,target=/root/.cache/uv /usr/local/bin/uv sync --python /usr/local/bin/python --frozen --no-install-project --no-group dev

RUN /app/.venv/bin/python -c "import tiktoken; tiktoken.get_encoding('o200k_base'); tiktoken.get_encoding('cl100k_base')"

FROM builder AS tests
COPY vendor/sdks/python/ /app/sdks/python/
RUN --mount=type=cache,target=/root/.cache/uv /usr/local/bin/uv sync --python /usr/local/bin/python --frozen --no-install-project
COPY vendor/ /app/
COPY ortak_honcho/ /app/ortak_honcho/
COPY tests/ /app/ortak_tests/
COPY run_tests.py /app/ortak_run_tests.py
ENV PATH="/app/.venv/bin:$PATH"
CMD ["python", "/app/ortak_run_tests.py"]

FROM ${BASE_IMAGE} AS runtime
WORKDIR /app
ENV PATH="/app/.venv/bin:$PATH" PYTHONDONTWRITEBYTECODE=1 PYTHONUNBUFFERED=1 TIKTOKEN_CACHE_DIR=/app/tiktoken-cache
RUN addgroup --system app && adduser --system --group app && chown app:app /app
COPY --from=builder --chown=app:app /app/.venv /app/.venv
COPY --from=builder --chown=app:app /app/tiktoken-cache /app/tiktoken-cache
COPY --chown=app:app vendor/src/ /app/src/
COPY --chown=app:app vendor/migrations/ /app/migrations/
COPY --chown=app:app vendor/scripts/ /app/scripts/
COPY --chown=app:app vendor/alembic.ini vendor/pyproject.toml vendor/config.toml.example vendor/LICENSE /app/
COPY --chown=app:app ortak_honcho/ /app/ortak_honcho/
COPY --chown=app:app entrypoint.sh /app/entrypoint.sh
USER app
EXPOSE 8000
ENTRYPOINT ["sh", "/app/entrypoint.sh"]
