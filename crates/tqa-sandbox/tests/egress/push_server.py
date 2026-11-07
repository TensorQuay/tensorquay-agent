"""Local authenticated TLS smart-HTTP Git origin that accepts pushes, with a bounded peer witness.

Port 8443 serves `git http-backend` for /repo.git with receive-pack enabled; every request must carry the
bearer credential from /fixture/push-token, and the value is never written anywhere. Port 8081 (fixture side
only) answers /witness with one record per request and /refs with the repository's branches. While the file
/tmp/hold-receive exists, a receive-pack upload waits before it is applied, so a test can stop the runtime
in the middle of a push.
"""
import hmac
import http.server
import json
import os
from pathlib import Path
import ssl
import subprocess
import threading
import time

ROOT = Path("/tmp/git-origin")
HOLD = Path("/tmp/hold-receive")
MAX_BODY = 16 * 1024 * 1024
MAX_RECEIPTS = 256


def git(*arguments, **options):
    return subprocess.run(["git"] + list(arguments), check=True, stdout=subprocess.PIPE,
                          stderr=subprocess.PIPE, timeout=15, **options).stdout


def repository():
    destination = ROOT / "repo.git"
    git("init", "--bare", "--initial-branch=main", "--template=", str(destination))
    body = b"push fixture\n"
    data = (b"commit refs/heads/main\nmark :1\n"
            b"committer Fixture <fixture@example.invalid> 0 +0000\ndata 7\nfixture\n"
            b"M 100644 inline README.md\ndata " + str(len(body)).encode() + b"\n" + body + b"\n\ndone\n")
    git("-C", str(destination), "fast-import", "--quiet", input=data)
    git("-C", str(destination), "config", "http.receivepack", "true")


def branches():
    output = git("-C", str(ROOT / "repo.git"), "for-each-ref", "--format=%(refname) %(objectname)", "refs/heads")
    return dict(line.split(" ", 1) for line in output.decode().splitlines())


class Origin(http.server.BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"
    receipts = []
    lock = threading.Lock()

    def log_message(self, *_arguments):
        return

    def reply(self, status, headers, payload):
        self.send_response(status)
        for name, value in headers:
            self.send_header(name, value)
        self.send_header("Content-Length", str(len(payload)))
        self.end_headers()
        self.wfile.write(payload)

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

    def serve(self):
        header = self.headers.get("Authorization", "")
        valid = header.startswith("Bearer ") and hmac.compare_digest(header[7:].encode(), self.server.token)
        path, _, query = self.path.partition("?")
        with self.lock:
            if len(self.receipts) < MAX_RECEIPTS:
                self.receipts.append({"method": self.command, "path": path, "query": query,
                                      "peer": self.client_address[0], "auth_valid": valid,
                                      "proxy_authorization": "Proxy-Authorization" in self.headers})
        payload = self.body() if self.command == "POST" else b""
        if not valid:
            self.reply(401, [("WWW-Authenticate", 'Bearer realm="git"')], b"")
            return
        if path.endswith("/git-receive-pack"):
            deadline = time.monotonic() + 60
            while HOLD.exists() and time.monotonic() < deadline:
                time.sleep(0.1)
        environment = {"PATH": os.environ.get("PATH", "/usr/bin:/bin"), "GIT_PROJECT_ROOT": str(ROOT),
                       "GIT_HTTP_EXPORT_ALL": "1", "REQUEST_METHOD": self.command, "PATH_INFO": path,
                       "QUERY_STRING": query, "CONTENT_TYPE": self.headers.get("Content-Type", ""),
                       "CONTENT_LENGTH": str(len(payload)), "REMOTE_USER": "fixture",
                       "REMOTE_ADDR": self.client_address[0], "GIT_CONFIG_NOSYSTEM": "1", "HOME": str(ROOT)}
        result = subprocess.run(["git", "http-backend"], input=payload, env=environment,
                                capture_output=True, timeout=60, check=False)
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

    do_GET = serve
    do_POST = serve


class Witness(http.server.BaseHTTPRequestHandler):
    def log_message(self, *_arguments):
        return

    def do_GET(self):
        if self.path == "/witness":
            with Origin.lock:
                body = json.dumps(Origin.receipts).encode()
        elif self.path == "/refs":
            body = json.dumps(branches()).encode()
        else:
            self.send_error(404)
            return
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)


def prepared(fixture):
    """The host owns the fixture while it writes the certificates, so an unreadable fixture is not ready yet."""
    try:
        return (fixture / "certificate-ready").exists()
    except PermissionError:
        return False


def main():
    fixture = Path("/fixture")
    ROOT.mkdir()
    repository()
    deadline = time.monotonic() + 90
    while not prepared(fixture):
        if time.monotonic() >= deadline:
            raise TimeoutError("Git fixture certificate was not prepared")
        time.sleep(0.05)
    context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
    context.load_cert_chain(fixture / "fixture-chain.pem", fixture / "fixture-key.pem")
    origin = http.server.ThreadingHTTPServer(("0.0.0.0", 8443), Origin)
    origin.token = (fixture / "push-token").read_text().strip().encode()
    origin.socket = context.wrap_socket(origin.socket, server_side=True)
    witness = http.server.ThreadingHTTPServer(("0.0.0.0", 8081), Witness)
    threads = [threading.Thread(target=server.serve_forever) for server in (origin, witness)]
    for thread in threads:
        thread.start()
    for thread in threads:
        thread.join()


if __name__ == "__main__":
    main()
