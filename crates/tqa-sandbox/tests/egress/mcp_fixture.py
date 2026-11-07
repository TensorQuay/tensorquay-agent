"""WEB-E2E-010 (connector half): a remote MCP server the gateway's connector proxy reaches over HTTPS.

Port 8443 serves MCP's streamable HTTP with JSON answers at /mcp: initialize, notifications/initialized, ping,
tools/list (one tool, create_issue; the connector's class, network, makes the task ask) and tools/call; DELETE
ends a session. Every request
must carry the connector credential in X-Api-Key; the expected value is read from /fixture/connector-token and is never
written anywhere. Port 8081 (fixture side only) answers GET /record with each request's method, each tools/call's
arguments, whether its credential was valid and whether any Tqa-* header reached it (the proxy must strip them), never
the credential itself. Stdlib only, as push_server.py; the Git fixture's certificate (its IP as the name) and readiness
marker, since the connector URL names the same IP literal (the proxy refuses a private address reached by name, SEC-024).
"""
import hmac
import http.server
import json
import secrets
import ssl
import threading
import time
from pathlib import Path

# A request body is small JSON-RPC; the proxy's own bound is 1 MiB.
MAX_BODY_BYTES = 1024 * 1024
# The certificate is written by the test driver after the container starts.
CERTIFICATE_WAIT_SECONDS = 90
PROTOCOL_VERSION = "2025-06-18"
TOOL = {
    "name": "create_issue",
    "description": "Create an issue in the tracker.",
    "inputSchema": {
        "type": "object",
        "properties": {
            "title": {"type": "string"},
            "labels": {"type": "array", "items": {"type": "string"}},
            "body": {"type": "string"},
        },
        "required": ["title"],
    },
}
# The UX review's connectors without a credential: /mcp-slow's one tool answers after longer than the connector's call
# timeout (the timeout sentence); /mcp-down lists create_issue, then drops the connection on the call (unreachable).
SLOW_TOOL = {"name": "wait", "description": "Answer after a long pause.", "inputSchema": {"type": "object", "properties": {}}}
SLOW_SECONDS = 20
CALLS = []
REQUESTS = []
LOCK = threading.Lock()


class Server(http.server.BaseHTTPRequestHandler):
    def do_POST(self):
        if self.path not in ("/mcp", "/mcp-slow", "/mcp-down"):
            return self.answer(404, None)
        slow, down = self.path == "/mcp-slow", self.path == "/mcp-down"
        valid = slow or down or hmac.compare_digest(self.headers.get("X-Api-Key", "").encode(), self.server.token)
        if not valid:
            return self.answer(401, {"error": "unauthorized"})
        length = int(self.headers.get("Content-Length", "0"))
        if length > MAX_BODY_BYTES:
            return self.answer(413, None)
        try:
            message = json.loads(self.rfile.read(length) or b"{}")
        except ValueError:
            message = None
        if not isinstance(message, dict):
            # A batch or other non-object body is not part of this fixture's protocol; refuse it without crashing.
            return self.answer(400, {"error": "a single JSON-RPC object is expected"})
        method, ident = message.get("method"), message.get("id")
        # The call headers the runtime sends the proxy; the proxy must not forward them (ADR-0031).
        tqa = any(name.lower().startswith("tqa-") for name in self.headers.keys())
        if not (slow or down):
            with LOCK:
                REQUESTS.append({"method": method, "tqa_headers": tqa})
        if ident is None:
            return self.answer(202, None)
        if method == "initialize":
            result = {"protocolVersion": PROTOCOL_VERSION, "capabilities": {"tools": {}},
                      "serverInfo": {"name": "tracker-fixture", "version": "1"}}
            return self.answer(200, {"jsonrpc": "2.0", "id": ident, "result": result},
                               {"Mcp-Session-Id": self.server.session})
        if method == "ping":
            return self.answer(200, {"jsonrpc": "2.0", "id": ident, "result": {}})
        if method == "tools/list":
            return self.answer(200, {"jsonrpc": "2.0", "id": ident, "result": {"tools": [SLOW_TOOL if slow else TOOL]}})
        if method == "tools/call" and down:
            # The UX review's unreachable connector: a remote that goes away mid-call (the session started fine).
            self.close_connection = True
            return None
        if method == "tools/call" and slow:
            time.sleep(SLOW_SECONDS)
            result = {"content": [{"type": "text", "text": "Done waiting"}], "isError": False}
            return self.answer(200, {"jsonrpc": "2.0", "id": ident, "result": result})
        if method == "tools/call":
            params = message.get("params", {})
            with LOCK:
                CALLS.append({"name": params.get("name"), "arguments": params.get("arguments"), "auth_valid": valid,
                              "tqa_headers": tqa, "peer": self.client_address[0]})
            result = {"content": [{"type": "text", "text": "Created issue #7"}], "isError": False}
            return self.answer(200, {"jsonrpc": "2.0", "id": ident, "result": result})
        error = {"code": -32601, "message": "Method not found"}
        return self.answer(200, {"jsonrpc": "2.0", "id": ident, "error": error})

    def do_DELETE(self):
        # The client may end its session; the fixture keeps no session state.
        return self.answer(200 if self.path in ("/mcp", "/mcp-slow", "/mcp-down") else 404, None)

    def answer(self, status, body, headers=None):
        payload = b"" if body is None else json.dumps(body).encode()
        self.send_response(status)
        for name, value in (headers or {}).items():
            self.send_header(name, value)
        if payload:
            self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(payload)))
        self.end_headers()
        self.wfile.write(payload)

    def log_message(self, *_args):
        # Nothing about a request is logged: the absence search reads this container's logs too.
        pass


class Record(http.server.BaseHTTPRequestHandler):
    def do_GET(self):
        if self.path != "/record":
            self.send_response(404)
            self.end_headers()
            return
        with LOCK:
            payload = json.dumps({"requests": REQUESTS, "calls": CALLS}).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(payload)))
        self.end_headers()
        self.wfile.write(payload)

    def log_message(self, *_args):
        pass


def prepared(fixture):
    """The host owns the fixture while it writes the certificates, so an unreadable fixture is not ready yet."""
    try:
        return (fixture / "certificate-ready").exists()
    except PermissionError:
        return False


def main():
    fixture = Path("/fixture")
    deadline = time.monotonic() + CERTIFICATE_WAIT_SECONDS
    while not prepared(fixture):
        if time.monotonic() >= deadline:
            raise TimeoutError("MCP fixture certificate was not prepared")
        time.sleep(0.05)
    context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
    context.load_cert_chain(fixture / "fixture-chain.pem", fixture / "fixture-key.pem")
    remote = http.server.ThreadingHTTPServer(("0.0.0.0", 8443), Server)
    remote.token = (fixture / "connector-token").read_text().strip().encode()
    remote.session = secrets.token_hex(16)
    remote.socket = context.wrap_socket(remote.socket, server_side=True)
    record = http.server.ThreadingHTTPServer(("0.0.0.0", 8081), Record)
    threads = [threading.Thread(target=server.serve_forever) for server in (remote, record)]
    for thread in threads:
        thread.start()
    for thread in threads:
        thread.join()


if __name__ == "__main__":
    main()
