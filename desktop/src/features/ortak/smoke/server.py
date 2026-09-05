"""Loopback-only fixture server: retain errors, omit successful asset GET noise."""
import functools
import http.server
import os


class Handler(http.server.SimpleHTTPRequestHandler):
    def log_request(self, code="-", size="-"):
        if not isinstance(code, int) or code >= 400:
            super().log_request(code, size)


output = "dist-ortak-smoke" if os.environ.get("ORTAK_SMOKE_ISOLATED") == "1" else "dist"
with http.server.ThreadingHTTPServer(
    ("127.0.0.1", 4177), functools.partial(Handler, directory=output)
) as server:
    server.serve_forever()
