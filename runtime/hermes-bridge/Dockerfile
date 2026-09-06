# Uses the selected BuildKit's built-in frontend; record its version with the
# artifact receipt. No floating frontend image is fetched during this build.
# Build with --build-context hermes_source=<verified extracted pinned archive>.
# This private tool-less image supports explicit API-key or access-only OAuth
# execution; it contains no running upstream init/gateway.
FROM ghcr.io/astral-sh/uv:0.11.6-python3.13-trixie@sha256:b3c543b6c4f23a5f2df22866bd7857e5d304b67a564f4feab6ac22044dde719b AS sqlite_build
USER root
RUN apt-get -o Acquire::Retries=3 update && \
    apt-get -o Acquire::Retries=3 install -y --no-install-recommends build-essential ca-certificates curl && \
    rm -rf /var/lib/apt/lists/*
RUN curl --fail --location --retry 2 --connect-timeout 15 --max-time 120 \
      https://sqlite.org/2026/sqlite-autoconf-3530400.tar.gz -o /tmp/sqlite.tar.gz && \
    echo '0e9483900e92cd5de8fd48d16bf9200145a61f7fd5be542a5ac81d8a9516eb9c  /tmp/sqlite.tar.gz' | sha256sum -c - && \
    tar -xzf /tmp/sqlite.tar.gz -C /tmp && \
    cd /tmp/sqlite-autoconf-3530400 && \
    CFLAGS='-O2 -DSQLITE_ENABLE_FTS5 -DSQLITE_THREADSAFE=1 -DSQLITE_MAX_VARIABLE_NUMBER=250000' \
      ./configure --prefix=/opt/sqlite-fixed --disable-static && make -j2 && make install

FROM ghcr.io/astral-sh/uv:0.11.6-python3.13-trixie@sha256:b3c543b6c4f23a5f2df22866bd7857e5d304b67a564f4feab6ac22044dde719b
LABEL org.ortak.hermes.revision="29112bef099274229cadff79cdff7bf7b99c4b77"
ENV PYTHONUNBUFFERED=1 PYTHONDONTWRITEBYTECODE=1 \
    PYTHONPATH=/opt/bridge:/opt/hermes \
    LD_LIBRARY_PATH=/opt/sqlite-fixed/lib \
    PATH=/opt/hermes/.venv/bin:/usr/local/bin:/usr/bin:/bin \
    UV_PYTHON_DOWNLOADS=never UV_NO_CACHE=1
COPY --from=sqlite_build /opt/sqlite-fixed /opt/sqlite-fixed
WORKDIR /opt/hermes
COPY --from=hermes_source . /opt/hermes
COPY ortak_hermes_bridge /opt/bridge/ortak_hermes_bridge
COPY checks/conversation-context-v1.json checks/run_loop_check.py checks/codex_oauth_check.py checks/codex_sdk_fixture.py checks/semantic_transport_check.py checks/semantic_lifecycle_check.py checks/workspace_tool_check.py /opt/bridge/checks/
COPY hermes-source-lock.json /opt/bridge/hermes-source-lock.json
RUN python -m ortak_hermes_bridge.verify_source --write-marker && \
    uv sync --python /usr/local/bin/python --frozen --no-dev --no-install-project && \
    python -c 'import sqlite3; assert sqlite3.sqlite_version_info >= (3,51,3), sqlite3.sqlite_version'
# Source is read-only and separate from each employee's run home. No secrets,
# config or persistent profile data is baked into this image.
RUN chmod -R a+rX,go-w /opt/hermes /opt/bridge /opt/sqlite-fixed
USER 10001:10001
WORKDIR /tmp
# This executes the real pinned AIAgent constructor and all denial boundaries.
# Build success is a guard smoke, not remote-provider or containment acceptance.
RUN --network=none python -m ortak_hermes_bridge.candidate_smoke
ENTRYPOINT ["python"]
CMD ["-m", "ortak_hermes_bridge.candidate_smoke"]
