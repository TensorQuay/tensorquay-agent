"""An authenticated TLS smart-HTTP Git server for the gateway's Git relay tests (065g, GW-I-014, SEC-019).

It runs `git http-backend` as CGI for bare repositories under --root. Every request must carry the expected
credential (HTTP Basic with --user, or Bearer); the expected value is read from --token-file and never printed.
Each request is appended to --witness as one JSON line: method, path, query, peer and whether authentication was
valid. The credential itself is never written anywhere.
"""

import argparse
import base64
import hmac
import json
import os
import ssl
import subprocess
import sys
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

MAX_BODY = 80 * 1024 * 1024


class Handler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, *args):
        pass

    def authenticated(self):
        header = self.headers.get("Authorization", "")
        expected = self.server.token
        if header.startswith("Bearer "):
            return self.server.user is None and hmac.compare_digest(header[7:].encode(), expected.encode())
        if header.startswith("Basic ") and self.server.user is not None:
            try:
                user, _, secret = base64.b64decode(header[6:], validate=True).decode().partition(":")
            except ValueError:
                return False
            return user == self.server.user and hmac.compare_digest(secret.encode(), expected.encode())
        return False

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

    def witness(self, valid):
        path, _, query = self.path.partition("?")
        entry = {"method": self.command, "path": path, "query": query, "peer": self.client_address[0],
                 "auth_valid": valid}
        with self.server.lock, open(self.server.witness, "a", encoding="utf-8") as out:
            out.write(json.dumps(entry) + "\n")

    def reply(self, status, headers, payload):
        self.send_response(status)
        for name, value in headers:
            self.send_header(name, value)
        self.send_header("Content-Length", str(len(payload)))
        self.end_headers()
        self.wfile.write(payload)

    def serve(self):
        valid = self.authenticated()
        self.witness(valid)
        payload = self.body() if self.command == "POST" else b""
        if not valid:
            self.reply(401, [("WWW-Authenticate", 'Basic realm="git"')], b"")
            return
        path, _, query = self.path.partition("?")
        environment = {
            "PATH": os.environ.get("PATH", "/usr/bin:/bin"),
            "GIT_PROJECT_ROOT": self.server.root,
            "GIT_HTTP_EXPORT_ALL": "1",
            "REQUEST_METHOD": self.command,
            "PATH_INFO": path,
            "QUERY_STRING": query,
            "CONTENT_TYPE": self.headers.get("Content-Type", ""),
            "CONTENT_LENGTH": str(len(payload)),
            "REMOTE_USER": "relay",
            "REMOTE_ADDR": self.client_address[0],
            "GIT_CONFIG_NOSYSTEM": "1",
            "HOME": self.server.root,
        }
        result = subprocess.run(["git", "http-backend"], input=payload, env=environment, capture_output=True,
                                timeout=60, check=False)
        head, _, content = result.stdout.partition(b"\r\n\r\n")
        if not _:
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


def main():
    parser = argparse.ArgumentParser()
    for option in ("--root", "--cert", "--key", "--token-file", "--witness", "--port-file"):
        parser.add_argument(option, required=True)
    parser.add_argument("--user")
    options = parser.parse_args()
    server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    server.root = options.root
    server.user = options.user
    with open(options.token_file, encoding="utf-8") as source:
        server.token = source.read().strip()
    server.witness = options.witness
    server.lock = threading.Lock()
    context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
    context.load_cert_chain(options.cert, options.key)
    server.socket = context.wrap_socket(server.socket, server_side=True)
    with open(options.port_file + ".tmp", "w", encoding="utf-8") as out:
        out.write(str(server.server_address[1]))
    os.replace(options.port_file + ".tmp", options.port_file)
    server.serve_forever()


if __name__ == "__main__":
    sys.exit(main())
