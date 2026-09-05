# Ortak Runtime Discovery, 2026-09-05

Status: partial slice A discovery; read-only; not adoption acceptance

Source: direct operator probes of the deployed Ortak Runtime environment,
recorded as reported. No further external calls were made while writing this
document.

Every statement below is a fact observed by the operator on 2026-09-05. Nothing
was restarted, saved, deployed, activated, created, deleted, run, or signed.
Account identifiers and private management URLs are intentionally not recorded.

The owner's decisions that follow from these observations are recorded in
`DEPLOYMENT_STRATEGY_V0.md` (the existing stack is disposable test
infrastructure; a clean separate stack is allowed) and
`UPSTREAM_MAINTENANCE.md` (pinning and upgrade handling for Buzz, Hermes, and
Honcho). This document records observations only.

## 1. Hosting

- The runtime host is a Hetzner server managed through Coolify, not the desktop
  Hermes install. Earlier assumptions about a local desktop Hermes are wrong.
- Coolify project `buzz-hermes`, environment `production`, one service resource,
  host label `nambse-hetzner`. The Coolify service identifier is a deployment
  detail and is not recorded here. `production` is a UI label only: the owner
  confirmed these are test instances, not Ortak production requirements.
- Coolify shows Hermes, Honcho API, Honcho DB, and Redis as healthy. The Honcho
  deriver is running but its health is unknown/excluded from the health view.
- `https://hermes.ortak.dev` redirects to a login. `https://buzz.ortak.dev`
  returns 200.

## 2. Hermes

- Profile directories `/opt/data/profiles/cem` and `/opt/data/profiles/zeynep`
  exist, matching the references in `BUZZ_BASELINE.md`.
- `/opt/hermes` has no git metadata. `pyproject` version is `0.21.0`.
- `GET localhost:8642/health` returns 200 with `status: ok`, `version: 0.21.0`.
- Processes include a dashboard and a gateway. Service listening ports: 8642
  and 9119 (an incidental port 38381 was also open and is not a service
  contract). The role of 9119 was not independently verified.
- A root API credential is resolvable from `/opt/data/.env` as
  `API_SERVER_KEY`. Its value was never printed or copied.
- Authenticated `GET /v1/capabilities` succeeded. Advertised features, all
  `true`: `run_submission`, `status`, `events`, `stop`, `steer`,
  `approval_response`, `approval_events`. `runs_idempotency`: supported `true`,
  durable `true`, `retention_seconds` 86400. Runtime mode `server_agent`,
  tool execution `server`, `split_runtime` `false`.
- Advertised is not exercised. Exact run routes as advertised:
  `POST /v1/runs`, `GET /v1/runs/{run_id}`, `GET /v1/runs/{run_id}/events`,
  `POST /v1/runs/{run_id}/approval`, `POST /v1/runs/{run_id}/steer`,
  `POST /v1/runs/{run_id}/stop`. Start, approval, cancel, and restart were
  **not** exercised.
- Profile-specific API keys are absent from both employee `.env` files.
  `API_SERVER_KEY` is absent from the employee-specific `.env` files (this is
  not proof that no other profile routing mechanism exists). Root/default API
  capabilities do **not** establish safe Cem/Zeynep profile selection or
  per-profile policy enforcement.
- The deployed source documents `/p/<profile>/...` API routing. Authenticated
  `GET /p/cem/v1/capabilities` and `GET /p/zeynep/v1/capabilities` each
  returned **404**. The owner confirmed that both employee gateways were
  intentionally stopped, so a 404 is consistent with unserved profiles; the
  exact cause (stopped gateway, routing not enabled, or a different
  mechanism) was **not** proven, and neither gateway was started to test it.
- Configured models, read from the profile configuration: Cem `gpt-5.6-sol`,
  Zeynep `gpt-5.6-terra`, both through `openai-codex`. This matches
  `config/employees/{cem,zeynep}.yaml` and does not prove current OAuth
  validity.
- Both profiles reference the shared workspace directory
  `/opt/data/workspace/company`, which exists on the host.
- Inspected the deployed `gateway/platforms/api_server_runs.py`,
  `_handle_run_events` from line 1072 onward: events are served from an
  in-memory `_run_streams` queue via `q.get()` with subscriber tracking, and
  the handler has no cursor or `Last-Event-ID` replay handling. **Durable
  cursor replay is not established.** A provider bridge or durable event
  capture is required before Ortak's cursor-resumed event ingestion can rely
  on this endpoint.

## 3. Honcho

- Both employee `honcho.json` files use `baseUrl` `http://honcho-api:8000`,
  host keys `hermes_cem` / `hermes_zeynep`, workspace `ortak`, `peerName`
  `sefa`, `aiPeer` `cem` / `zeynep`.
- `GET /health` returns 200. `GET /openapi.json` returns 200; title
  `Honcho API`, version `3.1.1`.
- The schema defines `POST /v3/workspaces` and
  `POST /v3/workspaces/{workspace_id}/peers` as get-or-create. These were
  **not** used.
- The documented read-only list endpoints
  `POST /v3/workspaces/list?size=100` and
  `POST /v3/workspaces/ortak/peers/list?size=100` with `{}` bodies were used.
  Workspace `ortak` found (total 1, page 1). Peers `sefa`, `cem`, `zeynep`
  all found (total 5, page 1), matching the `peerName` / `aiPeer` values in
  both `honcho.json` files. No memory contents were printed or modified.

## 4. Office identity and secrets

- Both `auth.json` files exist and are non-empty. OAuth validity was **not**
  tested.
- `BUZZ_PRIVATE_KEY` is present in each employee `.env`. The public key was
  derived in memory via secp256k1 and matched both manifest public keys
  (`config/employees/{cem,zeynep}.yaml`). No private values were printed or
  copied; no signing or publishing was attempted. This is a key-material /
  public-identity match, **not** end-to-end signer delivery proof.

## 5. What remains open for slice A

- Full provisioning dry-run saga against read-only adapters. Existing caveat:
  the saga dry-run still writes its own repository state and must be run with
  in-memory or local scratch bookkeeping while the identity, credential, and
  memory adapters stay read-only.
- A real credential-manager adapter (existence-only contract).
- Office membership checks for the Cem/Zeynep keys in the target cohort
  channel.
- Health gates (runtime, memory, signer) as durable evidence.
- Permission enforcement and profile-scoped runtime integration: only the
  root capability probe succeeded. Profile routes exist in source but the two
  tested profile probes returned 404; employee-specific execution and policy
  enforcement remain unverified.
- Durable run-event capture or provider bridge for Hermes event replay.

Slice A discovery is partial. Nothing above accepts adoption, and no employee
is activated. Per `DEPLOYMENT_STRATEGY_V0.md`, adopting these resources is
optional: slice B may proceed on a clean isolated stack with disposable test
employees, and the adoption gates above apply only if adoption is chosen.
