"""Local HTTP, TLS and UDP peers for the opt-in egress contract."""
import http.server
import json
from pathlib import Path
import socket
import ssl
import threading
import time


# SEC-059's site with a sign-in: /login sets a session cookie (kept on disk for an hour, as a real site's "stay signed
# in" would), and /whoami says in its title whether the browser sent it. /logins answers how many times /login was
# served, so the driver can tell a sign-in by hand that never reached the site from one whose cookie went missing.
SESSION = "tqa_fixture_session"
SIGNED_IN = "Signed in as fixture person"
SIGNED_OUT = "Signed out"


def signed_in(cookies):
    return any(part.strip() == SESSION + "=signed-in" for part in (cookies or "").split(";"))


class Handler(http.server.BaseHTTPRequestHandler):
    # The /login pages served so far; the server handles one request at a time.
    logins = 0

    def page(self, title, cookie=None):
        body = "<!doctype html><title>{0}</title><h1>{0}</h1>".format(title).encode()
        self.send_response(200)
        self.send_header("Content-Type", "text/html; charset=utf-8")
        self.send_header("Content-Length", str(len(body)))
        if cookie:
            self.send_header("Set-Cookie", cookie)
        self.end_headers()
        self.wfile.write(body)

    def do_GET(self):
        if self.path == "/login":
            Handler.logins += 1
            return self.page(SIGNED_IN, SESSION + "=signed-in; Max-Age=3600; Path=/; HttpOnly")
        if self.path == "/logins":
            return self.page(str(Handler.logins))
        if self.path == "/whoami":
            return self.page(SIGNED_IN if signed_in(self.headers.get("Cookie")) else SIGNED_OUT)
        body = json.dumps({"peer": self.client_address[0], "path": self.path,
                           "authorization": "Authorization" in self.headers,
                           "proxy_authorization": "Proxy-Authorization" in self.headers}).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, *_args):
        return


class Server(http.server.HTTPServer):
    def get_request(self):
        request, address = super().get_request()
        request.settimeout(5)
        return request, address


def datagrams(stop):
    with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as listener:
        listener.bind(("0.0.0.0", 9000))
        listener.settimeout(1)
        while not stop.is_set():
            try:
                data, address = listener.recvfrom(1024)
                listener.sendto(data, address)
            except socket.timeout:
                continue


def prepared(fixture):
    """The host owns the fixture while it writes the certificates, so an unreadable fixture is not ready yet."""
    try:
        return (fixture / "certificate-ready").exists()
    except PermissionError:
        return False


def main():
    directory = Path("/fixture")
    deadline = time.monotonic() + 90
    while not prepared(directory):
        if time.monotonic() >= deadline:
            raise TimeoutError("The test certificate was not prepared")
        time.sleep(0.05)
    context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
    context.load_cert_chain(directory / "fixture-ca.pem", directory / "fixture-key.pem")
    plain = Server(("0.0.0.0", 8081), Handler)
    secure = Server(("0.0.0.0", 8443), Handler)
    secure.socket = context.wrap_socket(secure.socket, server_side=True)
    stop = threading.Event()
    threads = [threading.Thread(target=server.serve_forever) for server in (plain, secure)]
    threads.append(threading.Thread(target=datagrams, args=(stop,)))
    for thread in threads:
        thread.start()
    try:
        for thread in threads:
            thread.join()
    finally:
        stop.set()
        for server in (plain, secure):
            server.shutdown()
            server.server_close()
        for thread in threads:
            thread.join(timeout=5)


if __name__ == "__main__":
    main()
