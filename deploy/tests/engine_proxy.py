"""H4: an engine-stall proxy for the soak. It sits on a Unix socket between the soak's gateway and the container
engine's socket and, during an `engine_stall` fault, holds, fails (500) or drops the calls that change containers
(create, start, stop, kill, wait, delete); everything else passes. It changes nothing on the host and loads nothing:
the effect of a slow or failing engine, reproduced deterministically.

Requests are framed (Content-Length, chunked, keep-alive); responses are passed through unread. That is safe because
the gateway's HTTP client does not pipeline: it sends a request only after the previous response ended, so an injected
500 can't interleave with a response still in flight. An upgraded connection (attach, exec) is piped raw both ways.
"""
from dataclasses import dataclass
from pathlib import Path
import re
import socket
import threading
import time

from soak_audit import require

MAX_HEADER = 64 * 1024
MAX_BODY = 16 * 1024 * 1024
ACTIONS = ("hold", "error", "drop")
# The engine calls that change a container's life; the API version prefix is optional.
MATCH = re.compile(r"^(/v[\d.]+)?/containers/(create|[^/]+/(start|stop|kill|wait|restart)|[^/]+)$")
CHANGING = {"POST", "DELETE"}
ERROR = (b"HTTP/1.1 500 Internal Server Error\r\nContent-Type: application/json\r\nContent-Length: 45\r\n\r\n"
         b'{"message":"injected engine failure (soak)"}\n')


@dataclass
class Request:
    method: str
    path: str
    upgrade: bool
    raw: bytes


@dataclass
class Stall:
    action: str
    seconds: float
    until: float

    def __post_init__(self):
        require(self.action in ACTIONS, "unknown engine stall action")


def _chunked_end(body):
    """The length of a complete chunked body at the start of `body`, or None when more is needed."""
    at = 0
    while True:
        line_end = body.find(b"\r\n", at)
        if line_end < 0:
            return None
        size = int(body[at:line_end].split(b";")[0], 16)
        at = line_end + 2
        if size == 0:
            end = body.find(b"\r\n\r\n", at - 2)
            return None if end < 0 else end + 4
        at += size + 2
        if at > len(body):
            return None


def split_request(buffer):
    """(the first complete request, the rest), or None while the request is incomplete."""
    head_end = buffer.find(b"\r\n\r\n")
    if head_end < 0:
        require(len(buffer) <= MAX_HEADER, "engine request headers exceed their cap")
        return None
    require(head_end <= MAX_HEADER, "engine request headers exceed their cap")
    lines = buffer[:head_end].decode("latin-1").split("\r\n")
    method, target, _ = lines[0].split(" ", 2)
    headers = {}
    for line in lines[1:]:
        name, _, value = line.partition(":")
        headers[name.strip().lower()] = value.strip()
    body_start = head_end + 4
    if "chunked" in headers.get("transfer-encoding", "").lower():
        length = _chunked_end(buffer[body_start:])
        if length is None:
            return None
    else:
        length = int(headers.get("content-length", "0"))
    require(length <= MAX_BODY, "engine request body exceeds its cap")
    if len(buffer) < body_start + length:
        return None
    upgrade = "upgrade" in headers.get("connection", "").lower() or "upgrade" in headers
    request = Request(method=method, path=target.split("?", 1)[0], upgrade=upgrade, raw=buffer[:body_start + length])
    return request, buffer[body_start + length:]


def decide(method, path, stall, now):
    """What happens to one call: forward, hold for seconds (never past the window), error or drop."""
    if stall is None or now >= stall.until or method not in CHANGING or not MATCH.match(path):
        return ("forward", 0)
    if stall.action == "hold":
        return ("hold", min(stall.seconds, stall.until - now))
    return (stall.action, 0)


class EngineProxy:
    def __init__(self, listen, upstream):
        self.listen, self.upstream = Path(listen), Path(upstream)
        self.current, self.affected = None, 0
        self.lock = threading.Lock()
        self.server = None
        # Set by stop(): a call held then ends at once instead of outliving the proxy.
        self.stopping = threading.Event()

    def start(self):
        # The socket is 0666 so the gateway's container user can open it; its directory keeps every other host user
        # out, since the socket reaches this user's whole engine.
        require(self.listen.parent.stat().st_mode & 0o777 == 0o700,
                "the engine proxy's directory must be 0700 (only this user)")
        self.server = socket.socket(socket.AF_UNIX)
        self.server.bind(str(self.listen))
        self.listen.chmod(0o666)
        self.server.listen(64)
        threading.Thread(target=self._accept, daemon=True).start()

    def stop(self):
        self.stopping.set()
        if self.server is not None:
            self.server.close()
            self.server = None

    def stall(self, action, seconds, window):
        """From now until `window` seconds from now, matching calls are held, failed or dropped."""
        with self.lock:
            self.current = Stall(action=action, seconds=seconds, until=time.monotonic() + window)

    def _accept(self):
        while self.server is not None:
            try:
                client, _ = self.server.accept()
            except OSError:
                return
            threading.Thread(target=self._serve, args=(client,), daemon=True).start()

    def _serve(self, client):
        upstream = socket.socket(socket.AF_UNIX)
        try:
            upstream.connect(str(self.upstream))
        except OSError:
            client.close()
            return
        writes = threading.Lock()
        threading.Thread(target=self._pump, args=(upstream, client, writes), daemon=True).start()
        buffer = b""
        try:
            while True:
                data = client.recv(65536)
                if not data:
                    return
                buffer += data
                while (split := split_request(buffer)) is not None:
                    request, buffer = split
                    with self.lock:
                        stall = self.current
                    action, seconds = decide(request.method, request.path, stall, time.monotonic())
                    if action != "forward":
                        with self.lock:
                            self.affected += 1
                    if action == "hold" and self.stopping.wait(seconds):
                        return  # the proxy stopped while it held this call
                    elif action == "error":
                        with writes:
                            client.sendall(ERROR)
                        continue
                    elif action == "drop":
                        return
                    upstream.sendall(request.raw)
                    if request.upgrade:
                        upstream.sendall(buffer)
                        self._pump(client, upstream, threading.Lock())
                        return
        except OSError:
            return
        finally:
            # Shut both sides down before closing them: a close alone doesn't wake the pump blocked reading the engine,
            # and the engine keeps its side open (keep-alive), so the pump's thread would never end (R2's leak).
            for side in (client, upstream):
                try:
                    side.shutdown(socket.SHUT_RDWR)
                except OSError:
                    pass
                try:
                    side.close()
                except OSError:
                    pass

    @staticmethod
    def _pump(source, target, writes):
        try:
            while True:
                data = source.recv(65536)
                if not data:
                    break
                with writes:
                    target.sendall(data)
        except OSError:
            pass
        finally:
            try:
                target.shutdown(socket.SHUT_WR)
            except OSError:
                pass
