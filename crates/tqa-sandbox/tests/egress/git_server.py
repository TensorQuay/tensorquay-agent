"""Local TLS Git origin with distinct branch/tag content and a bounded peer witness."""
import functools
import http.server
import json
from pathlib import Path
import ssl
import subprocess
import threading
import time


def git(*arguments, **options):
    subprocess.run(["git"] + list(arguments), check=True, stdout=subprocess.PIPE,
                   stderr=subprocess.PIPE, timeout=15, **options)


def commit(reference, mark, text, parent=None):
    header = ("commit " + reference + "\nmark :" + str(mark)
              + "\ncommitter Fixture <fixture@example.invalid> 0 +0000\ndata 7\nfixture\n")
    if parent is not None:
        header += "from :" + str(parent) + "\n"
    body = text.encode()
    return header.encode() + b"M 100644 inline README.md\ndata " + str(len(body)).encode() + b"\n" + body + b"\n\n"


def repository(directory):
    destination = directory / "repo.git"
    git("init", "--bare", "--initial-branch=main", "--template=", str(destination))
    data = commit("refs/heads/main", 1, "seed\n")
    data += commit("refs/heads/main", 2, "main\n", 1)
    data += commit("refs/heads/feature/example", 3, "branch\n", 1)
    data += b"reset refs/tags/v1\nfrom :1\n\ndone\n"
    git("-C", str(destination), "fast-import", "--quiet", input=data)
    git("-C", str(destination), "update-server-info")


class Handler(http.server.SimpleHTTPRequestHandler):
    receipts = []

    def do_GET(self):
        if self.path == "/witness":
            body = json.dumps(self.receipts).encode()
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)
            return
        if len(self.receipts) >= 256:
            self.send_error(429)
            return
        self.receipts.append({"peer": self.client_address[0], "path": self.path,
                              "authorization": "Authorization" in self.headers,
                              "proxy_authorization": "Proxy-Authorization" in self.headers})
        super().do_GET()

    def log_message(self, *_arguments):
        return


class Server(http.server.HTTPServer):
    def get_request(self):
        connection, address = super().get_request()
        connection.settimeout(5)
        return connection, address


def prepared(fixture):
    """The host owns the fixture while it writes the certificates, so an unreadable fixture is not ready yet."""
    try:
        return (fixture / "certificate-ready").exists()
    except PermissionError:
        return False


def main():
    fixture, content = Path("/fixture"), Path("/tmp/git-origin")
    content.mkdir()
    repository(content)
    deadline = time.monotonic() + 90
    while not prepared(fixture):
        if time.monotonic() >= deadline:
            raise TimeoutError("Git fixture certificate was not prepared")
        time.sleep(0.05)
    context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
    context.load_cert_chain(fixture / "fixture-ca.pem", fixture / "fixture-key.pem")
    handler = functools.partial(Handler, directory=str(content))
    servers = [Server(("0.0.0.0", port), handler) for port in (8081, 8443)]
    servers[1].socket = context.wrap_socket(servers[1].socket, server_side=True)
    threads = [threading.Thread(target=server.serve_forever) for server in servers]
    for thread in threads:
        thread.start()
    try:
        for thread in threads:
            thread.join()
    finally:
        for server in servers:
            server.shutdown()
            server.server_close()
        for thread in threads:
            thread.join(timeout=5)


if __name__ == "__main__":
    main()
