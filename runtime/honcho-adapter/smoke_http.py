"""Explicit fresh-resource smoke through an actual, isolated Honcho HTTP service.

Run inside the new API container so its freshly generated test JWT secret stays
inside that container. No credential value is printed. Requires full-text mode;
this checks persisted memory behavior, not provider/embedding/derivation health.
"""

import json
import os
from urllib.error import HTTPError
from urllib.parse import urlparse
from urllib.request import HTTPRedirectHandler, Request, build_opener
from uuid import uuid4

from src.config import settings
from src.security import JWTParams, create_admin_jwt, create_jwt

origin = os.environ.get("ORTAK_HONCHO_SMOKE_URL", "")
parsed = urlparse(origin)
if (
    parsed.scheme != "http"
    or parsed.hostname not in {"127.0.0.1", "localhost", "honcho-test-api"}
    or parsed.path not in {"", "/"}
    or parsed.username
    or parsed.password
    or parsed.query
    or parsed.fragment
):
    raise SystemExit("Set explicit ORTAK_HONCHO_SMOKE_URL for the new local test API")
if not settings.AUTH.USE_AUTH or settings.EMBED_MESSAGES:
    raise SystemExit("Smoke requires enabled authentication and EMBED_MESSAGES=false")
origin = origin.rstrip("/")


class NoRedirects(HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        return None


client = build_opener(NoRedirects())
token = create_admin_jwt()


def request(method, path, body=None, *, credential=token):
    headers = {"Accept": "application/json"}
    if credential:
        headers["Authorization"] = "Bearer " + credential
    data = None
    if body is not None:
        headers["Content-Type"] = "application/json"
        data = json.dumps(body, separators=(",", ":")).encode()
    req = Request(origin + path, method=method, data=data, headers=headers)
    try:
        response = client.open(req, timeout=10)
    except HTTPError as error:
        response = error
    with response:
        payload = response.read(4 * 1024 * 1024 + 1)
        if len(payload) > 4 * 1024 * 1024:
            raise ValueError("bounded smoke response exceeded")
        return response.status, json.loads(payload)


def require(condition, label):
    if not condition:
        raise AssertionError(label)


status, health = request("GET", "/health", credential=None)
require(status == 200 and health == {"status": "ok"}, "API liveness")
status, schema = request("GET", "/openapi.json", credential=None)
require(
    status == 200 and schema["info"]["version"] == "3.1.1", "native OpenAPI version"
)
require("/v3/ortak/resources/create" in schema["paths"], "extension OpenAPI route")
status, _ = request("GET", "/v3/ortak/protocol", credential=None)
require(status in {401, 403}, "unauthenticated protocol request must fail")
status, protocol = request("GET", "/v3/ortak/protocol")
require(
    status == 200 and protocol["protocol"] == "ortak-honcho/1", "authenticated protocol"
)

identity = uuid4().hex
owner = {
    "idempotency_key": "http_create_" + identity,
    "company_id": str(uuid4()),
    "employee_id": "http-smoke-" + identity,
    "workspace_id": "http_smoke_" + identity,
    "user_peer": "operator",
    "employee_peer": "employee",
}
status, created = request("POST", "/v3/ortak/resources/create", owner)
require(
    status == 201 and created["ownership"] == "created", "fresh atomic resource create"
)
status, replayed = request("POST", "/v3/ortak/resources/create", owner)
require(status == 200 and replayed == created, "resource receipt replay")
status, _ = request(
    "POST", "/v3/ortak/resources/create", {**owner, "idempotency_key": "collision"}
)
require(status == 409, "existing workspace must not be adopted or overwritten")

for path, wanted in [
    ("/v3/workspaces/list?page=1&size=100", {owner["workspace_id"]}),
    (
        f"/v3/workspaces/{owner['workspace_id']}/peers/list?page=1&size=100",
        {"operator", "employee"},
    ),
]:
    seen = set()
    for page in range(1, 6):
        status, listed = request("POST", path.replace("page=1", f"page={page}"), {})
        require(status == 200, "native read-only resource inspection")
        seen.update(item["id"] for item in listed["items"])
        if wanted <= seen:
            break
        if page >= listed["pages"]:
            break
    require(wanted <= seen, "exact resource IDs in bounded native list")

run_id = str(uuid4())
scope = {"scope": "run_scratch", "run_id": run_id}
context = {
    "company_id": owner["company_id"],
    "employee_id": owner["employee_id"],
    "scope": scope,
}
body = {
    **context,
    "idempotency_key": "http_write_" + identity,
    "facts": [
        {
            "content": "Ortak HTTP scoped memory smoke preserves durable provenance.",
            "provenance": {
                "employee_id": owner["employee_id"],
                "run_id": run_id,
                "source": "isolated_http_smoke",
                "recorded_at": "2026-09-05T00:00:00Z",
            },
        }
    ],
}
prefix = f"/v3/ortak/workspaces/{owner['workspace_id']}/sessions/run_{identity}"
status, written = request("POST", prefix + "/remember", body)
require(status == 201 and len(written["record_refs"]) == 1, "durable remember")
status, replayed = request("POST", prefix + "/remember", body)
require(status == 200 and replayed == written, "stable memory receipt replay")
query = {
    **context,
    "query": "durable provenance",
    "max_records": 10,
    "max_bytes": 16384,
}
status, recalled = request("POST", prefix + "/recall", query)
require(
    status == 200 and len(recalled["records"]) == 1, "nonempty scoped full-text recall"
)
require(
    recalled["records"][0]["record_ref"] == written["record_refs"][0],
    "canonical returned ID",
)
require(
    recalled["records"][0]["provenance"]["run_id"] == run_id,
    "exact returned run provenance",
)
status, _ = request(
    "POST", prefix + "/recall", {**query, "scope": {"scope": "employee_experience"}}
)
require(status == 409, "scope mismatch refusal")
foreign = create_jwt(JWTParams(w="foreign-workspace", s="foreign-session"))
status, _ = request("POST", prefix + "/recall", query, credential=foreign)
require(status in {401, 403}, "native JWT workspace boundary")
print(
    json.dumps(
        {
            "result": "passed",
            "protocol": protocol["protocol"],
            "workspace_id": owner["workspace_id"],
            "session_id": "run_" + identity,
            "record_refs": written["record_refs"],
            "recall_mode": "full_text",
            "external_provider_validated": False,
        }
    )
)
