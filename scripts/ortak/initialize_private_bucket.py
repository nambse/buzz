#!/usr/bin/env python3
"""Create only the fixed private development bucket using bounded signed S3 HTTP.

No SDK, account configuration, inherited proxy or existing storage is consulted.
The retained relay performs its separate conditional-write conformance gate.
"""

import argparse
from datetime import datetime, timezone
import hashlib
import hmac
import json
from pathlib import Path
import re
from urllib.error import HTTPError, URLError
from urllib.request import HTTPRedirectHandler, ProxyHandler, Request, build_opener

from private_native_services import private_file, selected_root

HOST = "127.0.0.1:9008"
BUCKET = "ortak-private-media"


class NoRedirect(HTTPRedirectHandler):
    """Never forward the selected authentication to another endpoint."""

    def redirect_request(self, req, fp, code, msg, headers, newurl):
        return None


def request(method: str, credentials: dict[str, str]) -> int:
    """Sign one empty HEAD/PUT request for this exact bucket and origin."""
    if method not in {"HEAD", "PUT"}:
        raise ValueError("unsupported bucket initialization method")
    now = datetime.now(timezone.utc)
    timestamp, day = now.strftime("%Y%m%dT%H%M%SZ"), now.strftime("%Y%m%d")
    digest = hashlib.sha256(b"").hexdigest()
    signed_headers = "host;x-amz-content-sha256;x-amz-date"
    scope = f"{day}/us-east-1/s3/aws4_request"
    canonical = (f"{method}\n/{BUCKET}\n\nhost:{HOST}\n"
                 f"x-amz-content-sha256:{digest}\nx-amz-date:{timestamp}\n\n"
                 f"{signed_headers}\n{digest}")
    string_to_sign = (f"AWS4-HMAC-SHA256\n{timestamp}\n{scope}\n"
                      + hashlib.sha256(canonical.encode()).hexdigest())
    key = ("AWS4" + credentials["secret_key"]).encode()
    for value in (day, "us-east-1", "s3", "aws4_request"):
        key = hmac.new(key, value.encode(), hashlib.sha256).digest()
    signature = hmac.new(key, string_to_sign.encode(), hashlib.sha256).hexdigest()
    authorization = (f"AWS4-HMAC-SHA256 Credential={credentials['access_key']}/{scope}, "
                     f"SignedHeaders={signed_headers}, Signature={signature}")
    call = Request(f"http://{HOST}/{BUCKET}", method=method,
                   data=b"" if method == "PUT" else None,
                   headers={"Host": HOST, "X-Amz-Date": timestamp,
                            "X-Amz-Content-Sha256": digest, "Authorization": authorization})
    opener = build_opener(ProxyHandler({}), NoRedirect())
    try:
        response = opener.open(call, timeout=5)
    except HTTPError as error:
        response = error
    with response:
        if len(response.read(4097)) > 4096:
            raise ValueError("object-store response exceeds limit")
        return response.code


def main() -> None:
    """Preserve an existing owned bucket, creating it only after an explicit404."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--state-dir", type=Path, required=True)
    args = parser.parse_args()
    root = selected_root(args.state_dir)
    credentials = json.loads(private_file(root / "object-store/credentials.json", 4096))
    if (set(credentials) != {"access_key", "secret_key"}
            or not re.fullmatch(r"[0-9a-f]{32}", credentials["access_key"])
            or not re.fullmatch(r"[0-9a-f]{64}", credentials["secret_key"])):
        raise ValueError("invalid selected storage credentials")
    status = request("HEAD", credentials)
    if status == 404:
        if request("PUT", credentials) not in {200, 409}:
            raise ValueError("private bucket creation failed")
        status = request("HEAD", credentials)
    if status != 200:
        raise ValueError("private bucket ownership could not be verified")
    print("Authenticated private bucket verified; existing contents preserved.")


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, KeyError, URLError):
        raise SystemExit("Private bucket initialization failed; no credentials were logged.") from None
