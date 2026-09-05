# Actual HTTP service smoke on an isolated test database

This recipe starts the real API entrypoint and lifespan. It exercises full-text
memory, authentication, receipt replay, and scoped provenance over TCP. It does
not validate an embedding/derivation provider and must not be described as doing
so. The helper requires `EMBED_MESSAGES=false` and creates new disposable
UUID-named resources; it never adopts, deletes, or inspects existing remote
memory. No token value is printed.

1. Build the runtime target with `python3 build_image.py runtime` after preparing
   the pinned `vendor/` source. Record the returned image digest.
2. Use a new private Docker network and fresh pgvector database, or the already
   created isolated `ortak-honcho-test-20260905` network/test database. Do not point
   this recipe at the existing external Honcho deployment.
3. Create a private test-only environment file outside Git. Set
   `DB_CONNECTION_URI` for that fresh database, `AUTH_USE_AUTH=true`, a newly
   generated `AUTH_JWT_SECRET`, a nonfunctional generated test-only
   `LLM_OPENAI_API_KEY`, `EMBED_MESSAGES=false`, `CACHE_ENABLED=false`,
   `METRICS_ENABLED=false`, `TELEMETRY_ENABLED=false`, and `SENTRY_ENABLED=false`.
   The test provider value only satisfies configuration parsing; no provider
   request is made or claimed healthy.
4. Start the runtime image on the private network with that environment file;
   expose no host port. Its default entrypoint runs native migrations, explicit
   extension table initialization, and Uvicorn with the native lifespan.
5. Once startup completes, execute the helper inside that new API container:

```sh
docker exec -i \
  -e ORTAK_HONCHO_SMOKE_URL=http://127.0.0.1:8000 \
  '<new isolated API container>' /app/.venv/bin/python \
  < runtime/honcho-adapter/smoke_http.py
```

Run the last command from the Ortak repository root. The helper uses only the
new container's test JWT secret to make an admin JWT in memory. It checks native
health/OpenAPI, denied unauthenticated access, authenticated protocol, fresh
create and receipt replay, strict collision refusal, exact IDs via read-only
native lists, one stored RunScratch fact, stable replay, nonempty scoped recall,
matching provenance, wrong-scope refusal, and wrong-workspace JWT denial.

Success prints public workspace/session/record IDs with `recall_mode: full_text`
and `external_provider_validated: false`. Retain that receipt with the exact
image digest. Any failure exits unsuccessfully; it is not a capability claim or
an empty successful recall. No cleanup runs automatically because the extension
does not advertise deletion ownership or a deletion API.
