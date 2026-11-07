"""H4: the engine-stall proxy's request framing, its fault decisions, and a real round trip through a fake engine on a
Unix socket (no Docker)."""
from pathlib import Path
import socket
import tempfile
import threading
import time
import unittest

import engine_proxy as proxy
from soak_audit import AuditError

CREATE = b"POST /v1.47/containers/create?name=x HTTP/1.1\r\nHost: docker\r\nContent-Length: 2\r\n\r\n{}"
LIST = b"GET /v1.47/containers/json HTTP/1.1\r\nHost: docker\r\n\r\n"


class Framing(unittest.TestCase):
    def test_a_request_with_a_body_is_split_from_what_follows(self):
        request, rest = proxy.split_request(CREATE + LIST)
        self.assertEqual(request.raw, CREATE)
        self.assertEqual((request.method, request.path, request.upgrade), ("POST", "/v1.47/containers/create", False))
        self.assertEqual(rest, LIST)

    def test_a_chunked_body_ends_at_its_zero_chunk(self):
        chunked = b"POST /v1.47/build HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n2\r\nab\r\n0\r\n\r\n"
        request, rest = proxy.split_request(chunked + LIST)
        self.assertEqual(request.raw, chunked)
        self.assertEqual(rest, LIST)

    def test_an_incomplete_request_waits_for_more(self):
        self.assertIsNone(proxy.split_request(CREATE[:-1]))
        self.assertIsNone(proxy.split_request(LIST[:10]))

    def test_an_upgrade_is_recognised_and_oversized_headers_fail(self):
        attach = b"POST /v1.47/containers/x/attach HTTP/1.1\r\nConnection: Upgrade\r\nUpgrade: tcp\r\n\r\n"
        request, _ = proxy.split_request(attach)
        self.assertTrue(request.upgrade)
        with self.assertRaises(AuditError):
            proxy.split_request(b"GET / HTTP/1.1\r\nX: " + b"a" * proxy.MAX_HEADER + b"\r\n\r\n")


class Decisions(unittest.TestCase):
    def test_only_matching_calls_inside_the_window_are_affected(self):
        stall = proxy.Stall(action="hold", seconds=30, until=100.0)
        self.assertEqual(proxy.decide("POST", "/v1.47/containers/create", stall, 50.0), ("hold", 30))
        self.assertEqual(proxy.decide("POST", "/v1.47/containers/abc/start", stall, 50.0), ("hold", 30))
        self.assertEqual(proxy.decide("GET", "/v1.47/containers/json", stall, 50.0), ("forward", 0))
        self.assertEqual(proxy.decide("POST", "/v1.47/containers/create", stall, 100.0), ("forward", 0))
        self.assertEqual(proxy.decide("POST", "/containers/create", None, 50.0), ("forward", 0))

    def test_a_hold_never_outlasts_its_window(self):
        stall = proxy.Stall(action="hold", seconds=30, until=60.0)
        self.assertEqual(proxy.decide("DELETE", "/v1.47/containers/abc", stall, 50.0), ("hold", 10.0))

    def test_error_and_drop_are_decided_per_call(self):
        for action in ("error", "drop"):
            stall = proxy.Stall(action=action, seconds=0, until=100.0)
            self.assertEqual(proxy.decide("POST", "/v1.47/containers/abc/stop", stall, 1.0), (action, 0))
        with self.assertRaises(AuditError):
            proxy.Stall(action="explode", seconds=0, until=1.0)


class Privacy(unittest.TestCase):
    def test_the_socket_must_live_in_a_directory_only_this_user_can_enter(self):
        with tempfile.TemporaryDirectory() as directory:
            shared = Path(directory) / "shared"
            shared.mkdir(mode=0o755)
            shared.chmod(0o755)
            with self.assertRaisesRegex(AuditError, "0700"):
                proxy.EngineProxy(shared / "proxy.sock", Path(directory) / "engine.sock").start()


class FakeEngine:
    """Answers every request with 200 and its path; records what reached it."""

    def __init__(self, path):
        self.seen = []
        self.server = socket.socket(socket.AF_UNIX)
        self.server.bind(str(path))
        self.server.listen()
        threading.Thread(target=self.serve, daemon=True).start()

    def serve(self):
        while True:
            try:
                connection, _ = self.server.accept()
            except OSError:
                return
            threading.Thread(target=self.handle, args=(connection,), daemon=True).start()

    def handle(self, connection):
        buffer = b""
        with connection:
            while True:
                data = connection.recv(65536)
                if not data:
                    return
                buffer += data
                while (split := proxy.split_request(buffer)) is not None:
                    request, buffer = split
                    self.seen.append(request.path)
                    body = request.path.encode()
                    connection.sendall(b"HTTP/1.1 200 OK\r\nContent-Length: %d\r\n\r\n%s" % (len(body), body))


def exchange(path, request, timeout=5):
    """Sends one request and reads one Content-Length response; empty when the proxy closed the connection."""
    with socket.socket(socket.AF_UNIX) as client:
        client.settimeout(timeout)
        client.connect(str(path))
        client.sendall(request)
        data = b""
        while True:
            chunk = client.recv(65536)
            if not chunk:
                return data
            data += chunk
            head, found, body = data.partition(b"\r\n\r\n")
            if found:
                lengths = [line.split(b":", 1)[1] for line in head.split(b"\r\n")
                           if line.lower().startswith(b"content-length:")]
                if len(body) >= int(lengths[0] if lengths else b"0"):
                    return data


class RoundTrip(unittest.TestCase):
    def setUp(self):
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        base = Path(directory.name)
        base.chmod(0o700)
        self.engine = FakeEngine(base / "engine.sock")
        self.proxy = proxy.EngineProxy(base / "proxy.sock", base / "engine.sock")
        self.proxy.start()
        self.addCleanup(self.proxy.stop)
        self.path = base / "proxy.sock"

    def test_without_a_stall_every_call_passes_through(self):
        self.assertIn(b"200 OK", exchange(self.path, CREATE))
        self.assertIn(b"200 OK", exchange(self.path, LIST))
        self.assertEqual(self.engine.seen, ["/v1.47/containers/create", "/v1.47/containers/json"])

    def test_a_held_call_arrives_late(self):
        self.proxy.stall("hold", seconds=0.4, window=5)
        started = time.monotonic()
        self.assertIn(b"200 OK", exchange(self.path, CREATE))
        self.assertGreaterEqual(time.monotonic() - started, 0.4)
        self.assertEqual(self.proxy.affected, 1)

    def test_other_calls_do_not_wait_for_a_held_one(self):
        # A create held 30 s is in flight while a list is answered: a proxy that made others wait would take the 30 s,
        # so a bound of a few seconds tells the two apart whatever the host's load. Stopping the proxy ends the hold.
        self.proxy.stall("hold", seconds=30, window=60)
        held = threading.Thread(target=exchange, args=(self.path, CREATE, 60), daemon=True)
        held.start()
        deadline = time.monotonic() + 10
        while self.proxy.affected < 1:
            self.assertLess(time.monotonic(), deadline, "the create never reached the proxy")
            time.sleep(0.01)
        started = time.monotonic()
        self.assertIn(b"200 OK", exchange(self.path, LIST))
        self.assertLess(time.monotonic() - started, 2.0, "the list waited for the held create")
        self.assertEqual(self.engine.seen, ["/v1.47/containers/json"], "the held create hasn't reached the engine")
        self.proxy.stop()
        held.join(10)
        self.assertFalse(held.is_alive(), "stopping the proxy ends the hold")

    def test_an_error_answers_500_without_reaching_the_engine(self):
        self.proxy.stall("error", seconds=0, window=5)
        self.assertIn(b"500", exchange(self.path, CREATE))
        self.assertEqual(self.engine.seen, [])

    def test_a_drop_closes_the_connection(self):
        self.proxy.stall("drop", seconds=0, window=5)
        self.assertEqual(exchange(self.path, CREATE), b"")
        self.assertEqual(self.engine.seen, [])

    def test_the_stall_ends_with_its_window(self):
        self.proxy.stall("error", seconds=0, window=0.2)
        time.sleep(0.3)
        self.assertIn(b"200 OK", exchange(self.path, CREATE))


def proxy_threads():
    """The proxy's own connection threads (Python names a thread after its target)."""
    return [thread for thread in threading.enumerate()
            if thread.name.endswith(("(_serve)", "(_pump)")) and thread.is_alive()]


class ThreadBound(unittest.TestCase):
    """The proxy's threads end with their connection, whatever the engine does with its side: the engine keeps an
    HTTP connection open after answering (keep-alive), so the proxy must end it. R2 leaked one thread per engine
    connection until the user's process limit was near."""

    def test_connections_that_close_leave_no_proxy_thread_behind(self):
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        base = Path(directory.name)
        base.chmod(0o700)
        FakeEngine(base / "engine.sock")
        engine_proxy = proxy.EngineProxy(base / "proxy.sock", base / "engine.sock")
        engine_proxy.start()
        self.addCleanup(engine_proxy.stop)
        # The threads this test starts, not a count: a thread from an earlier test that is still ending must not move
        # the baseline (seen as '0 != 1' under load).
        before = set(proxy_threads())
        left = lambda: [thread for thread in proxy_threads() if thread not in before]  # noqa: E731
        for _ in range(30):
            self.assertIn(b"200 OK", exchange(base / "proxy.sock", LIST))
        deadline = time.monotonic() + 5
        while left() and time.monotonic() < deadline:
            time.sleep(0.05)
        self.assertEqual(left(), [], "threads left after 30 closed connections")


if __name__ == "__main__":
    unittest.main()
