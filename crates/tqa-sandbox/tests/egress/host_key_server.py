"""A keyed host for the host-keys stack suite (ADR-0037): HTTPS on 8443 with a certificate from the fixture's own
authority, for an exact address (Amendment 2). `/echo` answers the Authorization it received in a header and the body
(so the proxy's scrubbing shows), and prints one line per request with what it received, for the test to read from the
container's log: the proof that the proxy swapped the placeholder for the value.

Two bare Git repositories, `/team/repo.git` and `/other/repo.git`, are served by `git http-backend` with pushes on
(SEC-049): a request needs the key's value as its bearer credential or basic password (from
/fixture/host-key-value, never written again), and each prints one `git` line with its method, path and whether it
was authorised.
"""
import base64
import hmac
import http.server
import json
import os
from pathlib import Path
import ssl
import subprocess
import sys
import threading
import time

ROOT = Path("/tmp/git-host")
REPOSITORIES = ("team", "other")
MAX_BODY = 16 * 1024 * 1024


def git(*arguments, **options):
    return subprocess.run(["git"] + list(arguments), check=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                          timeout=15, **options).stdout


def repositories():
    body = b"keyed host fixture\n"
    data = (b"commit refs/heads/main\nmark :1\n"
            b"committer Fixture <fixture@example.invalid> 0 +0000\ndata 7\nfixture\n"
            b"M 100644 inline README.md\ndata " + str(len(body)).encode() + b"\n" + body + b"\n\ndone\n")
    for name in REPOSITORIES:
        destination = ROOT / name / "repo.git"
        destination.parent.mkdir(parents=True)
        git("init", "--bare", "--initial-branch=main", "--template=", str(destination))
        git("-C", str(destination), "fast-import", "--quiet", input=data)
        git("-C", str(destination), "config", "http.receivepack", "true")


def authorised(header, value):
    """Whether `header` carries the key's value: as a bearer credential, or as a basic one's password."""
    if header.startswith("Bearer "):
        return hmac.compare_digest(header[7:].encode(), value)
    if header.startswith("Basic "):
        try:
            _, _, password = base64.b64decode(header[6:], validate=True).partition(b":")
        except ValueError:
            return False
        return hmac.compare_digest(password, value)
    return False


class Handler(http.server.BaseHTTPRequestHandler):
    # Keep-alive, so a connection outlives a change between its requests (SEC-049's rule removed mid-run).
    protocol_version = "HTTP/1.1"

    def do_GET(self):
        if self.path.split("/")[1] in REPOSITORIES:
            self.git()
            return
        authorization = self.headers.get("Authorization", "")
        print("received " + json.dumps({"path": self.path, "authorization": authorization}), flush=True)
        body = json.dumps({"you sent": authorization}).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("X-Echo", authorization)
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def do_POST(self):
        self.git()

    def body(self):
        if self.headers.get("Transfer-Encoding", "").lower() == "chunked":
            data = bytearray()
            while True:
                size = int(self.rfile.readline().split(b";")[0].strip(), 16)
                if size == 0:
                    while self.rfile.readline() not in (b"\r\n", b"\n", b""):
                        pass
                    return bytes(data)
                data += self.rfile.read(size)
                self.rfile.readline()
                if len(data) > MAX_BODY:
                    raise ValueError("body too large")
        length = int(self.headers.get("Content-Length", "0"))
        if length > MAX_BODY:
            raise ValueError("body too large")
        return self.rfile.read(length)

    def reply(self, status, headers, payload):
        self.send_response(status)
        for name, value in headers:
            self.send_header(name, value)
        self.send_header("Content-Length", str(len(payload)))
        self.end_headers()
        self.wfile.write(payload)

    def git(self):
        valid = authorised(self.headers.get("Authorization", ""), self.server.value)
        path, _, query = self.path.partition("?")
        print("git " + json.dumps({"method": self.command, "path": path, "authorised": valid}), flush=True)
        payload = self.body() if self.command == "POST" else b""
        if not valid:
            # Basic, as Git hosts ask: a credential helper's password goes as Basic, and curl answers no Bearer challenge.
            self.reply(401, [("WWW-Authenticate", 'Basic realm="git"')], b"")
            return
        environment = {"PATH": os.environ.get("PATH", "/usr/bin:/bin"), "GIT_PROJECT_ROOT": str(ROOT),
                       "GIT_HTTP_EXPORT_ALL": "1", "REQUEST_METHOD": self.command, "PATH_INFO": path,
                       "QUERY_STRING": query, "CONTENT_TYPE": self.headers.get("Content-Type", ""),
                       "CONTENT_LENGTH": str(len(payload)), "REMOTE_USER": "fixture",
                       "REMOTE_ADDR": self.client_address[0], "GIT_CONFIG_NOSYSTEM": "1", "HOME": str(ROOT)}
        result = subprocess.run(["git", "http-backend"], input=payload, env=environment, capture_output=True,
                                timeout=60, check=False)
        head, separator, content = result.stdout.partition(b"\r\n\r\n")
        if not separator:
            head, _, content = result.stdout.partition(b"\n\n")
        status, headers = 200, []
        for line in head.decode("latin-1").splitlines():
            name, _, value = line.partition(":")
            if name.lower() == "status":
                status = int(value.strip().split(" ")[0])
            elif name:
                headers.append((name, value.strip()))
        self.reply(status, headers, content)

    def log_message(self, *_args):
        return


# A port of the same host that no key is for: its tunnel is plain, so a client sees this host's own certificate.
UNKEYED_PORT = 7443


class Server(http.server.ThreadingHTTPServer):
    def get_request(self):
        request, address = super().get_request()
        request.settimeout(30)
        return request, address


def ready(fixture):
    """The host owns the fixture while it writes the certificates, so an unreadable fixture is not ready yet."""
    try:
        return (fixture / "certificate-ready").exists()
    except PermissionError:
        return False


def main():
    directory = Path("/fixture")
    ROOT.mkdir()
    repositories()
    deadline = time.monotonic() + 90
    while not ready(directory):
        if time.monotonic() >= deadline:
            raise TimeoutError("The test certificate was not prepared")
        time.sleep(0.05)
    context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
    context.load_cert_chain(directory / "fixture-chain.pem", directory / "fixture-key.pem")
    value = (directory / "host-key-value").read_text().strip().encode()
    servers = []
    # 8443 is keyed; 7443, the same host and certificate, is a port no key is for (SBX-C-019's unkeyed host).
    for port in (8443, UNKEYED_PORT):
        server = Server(("0.0.0.0", port), Handler)
        server.value = value
        server.socket = context.wrap_socket(server.socket, server_side=True)
        servers.append(server)
    for server in servers[1:]:
        threading.Thread(target=server.serve_forever, daemon=True).start()
    print("keyed host ready", flush=True)
    try:
        servers[0].serve_forever()
    finally:
        for server in servers:
            server.server_close()
    return 0


if __name__ == "__main__":
    sys.exit(main())
