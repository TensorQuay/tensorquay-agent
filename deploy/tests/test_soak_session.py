"""SOAK-001: the harness outlives its session. The gateway ends a session `session_ttl` after sign-in (24 h by default),
however much it is used, and the soak runs 24 h before it verifies: 15b's soak (10 Oct) failed its verify with 401.
A 401 renews the session only once it is past its known end; an earlier one is the product's failure, never hidden."""
import http.server
import json
from pathlib import Path
import tempfile
import threading
import unittest
from unittest import mock
import urllib.error

import soak_stack
from soak_audit import AuditError, ReceiptLog
from soak_stream import Refused, SseClient


class Gateway(http.server.ThreadingHTTPServer):
    """A gateway's sessions in miniature: each sign-in gives a new cookie; `expire()` ends every session so far."""
    def __init__(self):
        super().__init__(("127.0.0.1", 0), Handler)
        self.valid, self.sign_ins, self.lock = set(), 0, threading.Lock()
        self.password = "synthetic-session-password"

    def expire(self):
        with self.lock:
            self.valid.clear()

    @property
    def origin(self):
        return "http://127.0.0.1:{}".format(self.server_address[1])


class Handler(http.server.BaseHTTPRequestHandler):
    def log_message(self, *arguments):
        pass

    def answer(self, status, body, headers=()):
        data = json.dumps(body).encode()
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(data)))
        for name, value in headers:
            self.send_header(name, value)
        self.end_headers()
        self.wfile.write(data)

    def signed_in(self):
        cookie = self.headers.get("Cookie", "")
        return any(part.strip() in self.server.valid for part in cookie.split(";"))

    def do_POST(self):
        length = int(self.headers.get("Content-Length", "0"))
        body = json.loads(self.rfile.read(length) or b"{}")
        if self.path == "/api/v1/auth/login":
            if body.get("password") != self.server.password:
                return self.answer(401, {"error": {"code": "unauthorized"}})
            with self.server.lock:
                self.server.sign_ins += 1
                token = "tqa_session=s{}".format(self.server.sign_ins)
                self.server.valid.add(token)
            return self.answer(200, {"id": "user"}, [("Set-Cookie", token + "; Path=/; HttpOnly")])
        if not self.signed_in():
            return self.answer(401, {"error": {"code": "unauthorized"}})
        return self.answer(200, {"posted": self.path})

    def do_GET(self):
        if not self.signed_in():
            return self.answer(401, {"error": {"code": "unauthorized"}})
        return self.answer(200, {"read": self.path})


class SessionTests(unittest.TestCase):
    def setUp(self):
        self.gateway = Gateway()
        threading.Thread(target=self.gateway.serve_forever, daemon=True).start()
        self.addCleanup(self.gateway.server_close)
        self.addCleanup(self.gateway.shutdown)
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.stack = soak_stack.Stack(Path(temporary.name) / "stack", {}, Path(temporary.name))
        self.stack.origin = self.gateway.origin
        self.stack.account = {"email": "soak@example.invalid", "password": self.gateway.password}
        self.stack.session_ttl = 3600
        self.renewals = []
        self.stack.on_renewal = self.renewals.append
        self.stack.sign_in()

    def end_session(self):
        """The session's known end has passed (its sign-in an hour and a minute ago), and the gateway ended it."""
        self.stack.signed_in_at -= 3660
        self.gateway.expire()

    def test_a_call_after_the_session_ended_signs_in_again_and_is_retried_once(self):
        self.assertEqual(self.stack.request("/api/v1/conversations"), {"read": "/api/v1/conversations"})
        self.assertEqual(self.stack.cookie, "tqa_session=s1")
        self.end_session()
        self.assertEqual(self.stack.request("/api/v1/conversations/c/close", {}),
                         {"posted": "/api/v1/conversations/c/close"})
        self.assertEqual(self.gateway.sign_ins, 2)
        self.assertEqual(self.stack.cookie, "tqa_session=s2", "the streams read the new cookie")
        self.assertEqual(self.stack.sign_ins, 2, "the run's evidence counts its sign-ins")
        # The journal's session_renewed event: how old the session was, against its known lifetime.
        self.assertEqual(len(self.renewals), 1)
        self.assertGreaterEqual(self.renewals[0]["age_seconds"], 3660)
        self.assertEqual(self.renewals[0]["session_ttl_seconds"], 3600)

    def test_a_401_before_the_session_s_known_end_is_a_failure_never_renewed(self):
        # The lead's P1: a session the gateway drops early is a product bug the renewal must not hide.
        self.gateway.expire()
        with self.assertRaises(AuditError) as raised:
            self.stack.request("/api/v1/conversations")
        self.assertIn("session refused before its end", str(raised.exception))
        self.assertEqual((self.gateway.sign_ins, self.renewals), (1, []))

    def test_the_margin_allows_a_401_just_before_the_end_and_not_earlier(self):
        self.assertEqual(soak_stack.session_margin(86400), 60)
        self.assertEqual(soak_stack.session_margin(60), 6)
        self.stack.signed_in_at -= 3600 - 59  # inside the last minute's margin
        self.gateway.expire()
        self.assertEqual(self.stack.request("/api/v1/conversations"), {"read": "/api/v1/conversations"})

    def test_a_refusal_after_a_fresh_sign_in_is_a_failure_not_a_loop(self):
        self.stack.signed_in_at -= 3660
        # Every call refused, even with a fresh cookie.
        with mock.patch.object(Handler, "signed_in", lambda handler: False), \
                self.assertRaises(urllib.error.HTTPError) as raised:
            self.stack.request("/api/v1/conversations")
        self.assertEqual(raised.exception.code, 401)
        self.assertEqual(self.gateway.sign_ins, 2, "one sign-in again, then the refusal stands")

    def test_a_refused_sign_in_is_never_retried(self):
        self.stack.account = dict(self.stack.account, password="wrong")
        with self.assertRaises(urllib.error.HTTPError):
            self.stack.request("/api/v1/auth/login", self.stack.account)
        self.assertEqual(self.gateway.sign_ins, 1)

    def test_callers_holding_the_same_stale_cookie_sign_in_once(self):
        stale = self.stack.cookie
        self.end_session()
        first, second = self.stack.renew(stale), self.stack.renew(stale)
        self.assertEqual((first, second), ("tqa_session=s2", "tqa_session=s2"))
        self.assertEqual(self.gateway.sign_ins, 2)


class StreamSessionTests(unittest.TestCase):
    """A stream reconnecting after its session ended gets 401: it renews the stack's session and reconnects."""

    def setUp(self):
        self.gateway = Gateway()
        threading.Thread(target=self.gateway.serve_forever, daemon=True).start()
        self.addCleanup(self.gateway.server_close)
        self.addCleanup(self.gateway.shutdown)
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.receipts = ReceiptLog(Path(temporary.name) / "client.jsonl")
        self.addCleanup(self.receipts.output.close)
        self.stack = soak_stack.Stack(Path(temporary.name) / "stack", {}, Path(temporary.name))
        self.stack.origin = self.gateway.origin
        self.stack.account = {"email": "soak@example.invalid", "password": self.gateway.password}
        self.stack.session_ttl = 3600
        self.renewals = []
        self.stack.on_renewal = self.renewals.append
        self.stack.sign_in()

    def end_session(self):
        """The session's known end has passed (its sign-in an hour and a minute ago), and the gateway ended it."""
        self.stack.signed_in_at -= 3660
        self.gateway.expire()

    def client(self):
        address = self.gateway.origin, "/events", self.stack.cookie, "0/0"
        return SseClient(address, self.receipts, None, [], session=self.stack)

    def test_a_401_reconnect_renews_the_session_and_waits_like_a_refusal(self):
        client = self.client()
        self.end_session()
        with self.assertRaises(Refused):
            client.read_connection()
        self.assertEqual(self.gateway.sign_ins, 2)
        self.assertEqual(self.stack.cookie, "tqa_session=s2")

    def test_a_401_for_the_renewed_cookie_fails_the_stream(self):
        client = self.client()
        self.end_session()
        with self.assertRaises(Refused):
            client.read_connection()
        # Refused again before any stream opened with the new cookie: not an ended session, a failure.
        with mock.patch.object(Handler, "signed_in", lambda handler: False), self.assertRaises(AuditError):
            client.read_connection()

    def test_a_stream_refused_before_the_session_s_end_fails(self):
        client = self.client()
        self.gateway.expire()
        with self.assertRaises(AuditError):
            client.read_connection()
        self.assertEqual(self.gateway.sign_ins, 1)

    def test_without_a_session_a_401_stays_a_failure(self):
        client = SseClient((self.gateway.origin, "/events", "tqa_session=none", "0/1"), self.receipts, None, [])
        with self.assertRaises(AuditError):
            client.read_connection()


class SessionTtlTests(unittest.TestCase):
    """The session's lifetime is the stack's own: gateway.toml's top-level session_ttl, else the gateway's default."""

    def test_it_reads_the_stack_s_setting_or_the_default(self):
        self.assertEqual(soak_stack.session_ttl("session_ttl = 60\n[sandbox]\nready_seconds = 5\n"), 60)
        self.assertEqual(soak_stack.session_ttl("[sandbox]\nready_seconds = 5\n"), 86400)

    def test_it_refuses_what_the_gateway_would(self):
        for text in ("session_ttl = 59\n", "session_ttl = true\n", "session_ttl = \"60\"\n"):
            with self.subTest(text), self.assertRaises(AuditError):
                soak_stack.session_ttl(text)



class SessionTtlOverrideTests(unittest.TestCase):
    """The session-renewal rehearsal (SOAK-001): TQA_SOAK_SESSION_TTL shortens the stack's
    sessions as gateway.toml's first line (a top-level key: after a table header it would belong to that table), and a
    run with it is never gate evidence."""

    def stack(self, env):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        directory = Path(temporary.name) / "stack"
        directory.mkdir()
        (directory / "gateway.toml").write_text(
            "listen='0.0.0.0:8080'\n[sandbox]\nready_seconds=30\n[tasks]\nauto_title=false\n")
        (directory / "models.toml").write_text("[[routes]]\nname='local'\n")
        stack = soak_stack.Stack(directory, env, Path(temporary.name))
        stack.images = {"fake-model": "sha256:" + "0" * 64}
        return stack, directory

    def test_it_is_gateway_toml_s_first_line_and_read_back(self):
        stack, directory = self.stack({"TQA_SOAK_SESSION_TTL": "60"})
        stack.configure(rehearsal=False)
        text = (directory / "gateway.toml").read_text()
        self.assertEqual(text.splitlines()[0], "session_ttl=60")
        self.assertEqual(soak_stack.session_ttl(text), 60)
        self.assertIn("[tasks]\nauto_title=false\n", text, "the rest of the file as it was")
        self.assertFalse(stack.evidence(rehearsal=False), "a shortened session is never gate evidence")

    def test_without_it_the_file_and_the_evidence_stay_as_they_were(self):
        stack, directory = self.stack({})
        before = (directory / "gateway.toml").read_text()
        stack.configure(rehearsal=False)
        self.assertEqual((directory / "gateway.toml").read_text(), before)
        self.assertTrue(stack.evidence(rehearsal=False))
        self.assertFalse(stack.evidence(rehearsal=True))

    def test_it_refuses_what_the_gateway_would(self):
        for value in ("59", "abc", "", "60.5"):
            with self.subTest(value=value):
                stack, directory = self.stack({"TQA_SOAK_SESSION_TTL": value})
                before = (directory / "gateway.toml").read_text()
                with self.assertRaises(AuditError):
                    stack.configure(rehearsal=False)
                self.assertEqual((directory / "gateway.toml").read_text(), before)


if __name__ == "__main__":
    unittest.main()
