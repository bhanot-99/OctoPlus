"""HTTPS fixture server for the C7 network-namespace test (rules.md §8.4).

Runs only inside a private network namespace made by tests/scripts/fetcher-netns-test.sh,
so nothing it serves or receives can leave the machine. Every request's line and headers
are appended to the log file as one JSON object per line, so the test can check exactly
what the fetcher sent (FR-FET-06).
"""

import http.server
import json
import socketserver
import ssl
import sys
import time

CERT, KEY, LOG = sys.argv[1], sys.argv[2], sys.argv[3]

PAGE = (
    "<!DOCTYPE html><html><head><title>Netns Test Page</title>"
    "<script>var secret = 1;</script></head>"
    "<body><p>Readable marker text NETNS-OK.</p></body></html>"
)
SEARCH = (
    '<html><body><a href="https://search.example/settings">Settings</a>'
    '<a class="result__a" href="//search.example/l/?uddg=https%3A%2F%2Ftest.example%2Fpage&amp;rut=1">'
    "First <b>result</b></a>"
    '<a class="result__snippet" href="//search.example/l/?uddg=https%3A%2F%2Ftest.example%2Fpage&amp;rut=1">'
    "Snippet one</a>"
    '<a class="result__a" href="//search.example/l/?uddg=https%3A%2F%2Ftest.example%2Fpage2&amp;rut=2">'
    "Second</a>"
    '<a class="result__a" href="//search.example/l/?uddg=https%3A%2F%2Fprivate.example%2F&amp;rut=3">'
    "Private</a>"
    "</body></html>"
)


class Handler(http.server.BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, *args):  # quiet; requests go to LOG instead
        pass

    def send_body(self, status, ctype, body, extra=()):
        data = body.encode() if isinstance(body, str) else body
        self.send_response(status)
        self.send_header("Content-Type", ctype)
        self.send_header("Content-Length", str(len(data)))
        for k, v in extra:
            self.send_header(k, v)
        self.end_headers()
        self.wfile.write(data)

    def redirect(self, location):
        self.send_response(302)
        self.send_header("Location", location)
        self.send_header("Content-Length", "0")
        self.end_headers()

    def do_GET(self):  # noqa: N802 (http.server naming)
        with open(LOG, "a", encoding="utf-8") as f:
            f.write(json.dumps({"path": self.path, "headers": dict(self.headers.items())}) + "\n")
        p = self.path
        if p in ("/page", "/page2"):
            self.send_body(200, "text/html; charset=utf-8", PAGE, [("Set-Cookie", "track=1")])
        elif p == "/redirect":
            self.redirect("/page")
        elif p == "/redirect-private":
            self.redirect("https://private.example/")
        elif p == "/redirect-ip":
            self.redirect("https://10.9.9.9/")
        elif p == "/redirect-http":
            self.redirect("http://test.example/page")
        elif p == "/redirect-loop":
            self.redirect("/redirect-loop")
        elif p == "/json":
            self.send_body(200, "application/json", '{"a": 1}')
        elif p == "/big":
            self.send_body(200, "text/html", b"a" * (6 * 1024 * 1024))
        elif p == "/big-chunked":
            self.send_response(200)
            self.send_header("Content-Type", "text/html")
            self.send_header("Connection", "close")
            self.end_headers()
            for _ in range(7):
                self.wfile.write(b"b" * (1024 * 1024))
        elif p == "/slow":
            time.sleep(10)
            self.send_body(200, "text/html", PAGE)
        elif p.startswith("/html/?q="):
            self.send_body(200, "text/html", SEARCH)
        else:
            self.send_body(404, "text/plain", "not found")


class Server(socketserver.ThreadingMixIn, http.server.HTTPServer):
    daemon_threads = True
    allow_reuse_address = True


ctx = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
ctx.load_cert_chain(CERT, KEY)
httpd = Server(("0.0.0.0", 443), Handler)
httpd.socket = ctx.wrap_socket(httpd.socket, server_side=True)
print("ready", flush=True)
httpd.serve_forever()
