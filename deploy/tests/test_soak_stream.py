"""SOAK-002: SSE framing, cursor boundaries and intentional reconnect classification."""
import json
from pathlib import Path
import tempfile
import time
import unittest
from unittest.mock import patch

from soak_audit import AuditError, ReceiptLog
from soak_stream import (CREATE_RETRY_GAP_SECONDS, DISK_RECOVERY_SECONDS, RecoveryNotices, Refused, SseClient,
                         SseDecoder, expected_notice)


class DecoderTests(unittest.TestCase):
    def test_empty_comments_and_crlf_do_not_create_receipts(self):
        decoder = SseDecoder()
        for line in (b"\n", b": keep-alive\r\n", b"\r\n"):
            self.assertIsNone(decoder.line(line))
        self.assertIsNone(decoder.line(b"id: 7\r\n"))
        self.assertIsNone(decoder.line(b'data: {"type":"event",\r\n'))
        self.assertIsNone(decoder.line(b'data: "seq":7}\r\n'))
        self.assertEqual(decoder.line(b"\r\n"), (7, {"type": "event", "seq": 7}))

    def test_partial_record_is_not_delivered_until_its_blank_line(self):
        decoder = SseDecoder()
        decoder.line(b"id: 1\n")
        decoder.line(b'data: {"type":"event","seq":1}\n')
        with self.assertRaisesRegex(AuditError, "partial"):
            decoder.finish()
        self.assertEqual(decoder.line(b"\n")[0], 1)
        decoder.finish()

    def test_malformed_and_oversized_records_fail(self):
        records = [[b"id: no\n"], [b"data: {\n", b"\n"],
                   [b"data: []\n", b"\n"], [b"data: " + b"x" * 33 + b"\n"],
                   [b'data: {"value":NaN}\n', b"\n"]]
        for lines in records:
            with self.subTest(lines=lines), self.assertRaises(AuditError):
                decoder = SseDecoder(max_bytes=32)
                for line in lines:
                    decoder.line(line)

    def test_control_frame_has_no_sse_id_and_record_state_is_reset(self):
        decoder = SseDecoder()
        value = {"type": "session.ready"}
        decoder.line(b"data: " + json.dumps(value).encode() + b"\n")
        self.assertEqual(decoder.line(b"\n"), (None, value))
        decoder.line(b"id: 1\n")
        decoder.line(b'data: {"type":"event","seq":1}\n')
        self.assertEqual(decoder.line(b"\n")[0], 1)
        decoder.line(b"data: " + json.dumps(value).encode() + b"\n")
        self.assertIsNone(decoder.line(b"\n")[0])

    def test_only_restarting_inside_the_planned_window_is_expected(self):
        for code in ("restarting", "seq_gap", "store_unavailable", "internal"):
            for when in (9, 10, 11, 12):
                self.assertEqual(expected_notice({"code": code}, when, [(10, 11)]),
                                 code == "restarting" and when in (10, 11))

    def test_store_unavailable_is_expected_only_inside_a_database_fault_window(self):
        for code in ("restarting", "store_unavailable", "internal"):
            for when in (9, 10, 11, 12):
                with self.subTest(code=code, when=when):
                    self.assertEqual(expected_notice({"code": code}, when, [], [[10, 11]]),
                                     code == "store_unavailable" and when in (10, 11))

    def test_io_is_expected_only_inside_a_disk_full_window_and_its_margin(self):
        disk = [[10, 20]]
        self.assertTrue(expected_notice({"code": "io"}, 15, [], [], disk))
        self.assertTrue(expected_notice({"code": "io"}, 20 + DISK_RECOVERY_SECONDS, [], [], disk))
        self.assertFalse(expected_notice({"code": "io"}, 21 + DISK_RECOVERY_SECONDS, [], [], disk))
        self.assertFalse(expected_notice({"code": "io"}, 9, [], [], disk))
        self.assertFalse(expected_notice({"code": "internal"}, 15, [], [], disk))
        self.assertFalse(expected_notice({"code": "io"}, 15, [], [[10, 20]], []), "a database window allows no io")

    def test_sandbox_failed_is_the_other_full_disk_notice_and_nothing_else_is(self):
        disk = [[10, 20]]
        self.assertTrue(expected_notice({"code": "sandbox_failed"}, 15, [], [], disk))
        self.assertFalse(expected_notice({"code": "sandbox_failed"}, 9, [], [], disk))
        for code in ("action_failed", "runtime_start_failed", "internal", "state_quota"):
            with self.subTest(code):
                self.assertFalse(expected_notice({"code": code}, 15, [], [], disk))

    def test_a_sandbox_failed_notice_is_remembered_like_io(self):
        import tempfile
        from pathlib import Path
        with tempfile.TemporaryDirectory() as temporary:
            receipts = ReceiptLog(Path(temporary) / "client.jsonl")
            self.addCleanup(receipts.output.close)
            client = SseClient(("http://localhost", "/events", "", "5/0"), receipts, None, [], [], [[0, float("inf")]])
            client.receive(None, {"type": "error", "seq": None, "code": "sandbox_failed", "retryable": True,
                                  "message": "create failed"})
            self.assertEqual(len(client.io_times), 1)

    def test_an_io_notice_is_remembered_for_the_recovery_check(self):
        import tempfile
        from pathlib import Path
        with tempfile.TemporaryDirectory() as temporary:
            receipts = ReceiptLog(Path(temporary) / "client.jsonl")
            self.addCleanup(receipts.output.close)
            client = SseClient(("http://localhost", "/events", "", "2/0"), receipts, None, [], [], [[0, float("inf")]])
            client.receive(None, {"type": "error", "seq": None, "code": "io", "message": "runtime storage I/O failed"})
            self.assertEqual(len(client.io_times), 1)

    def test_store_unavailable_outside_a_database_window_is_accepted_and_recorded_as_load(self):
        # On a busy host the gateway's store can miss its bound with no database fault (R1 on candidate 6): the notice
        # is honest, so it is accepted and recorded, and the driver then requires recovery and caps how often.
        with tempfile.TemporaryDirectory() as temporary:
            receipts = ReceiptLog(Path(temporary) / "client.jsonl")
            self.addCleanup(receipts.output.close)
            client = SseClient(("http://localhost", "/events", "", "6/0"), receipts, None, [], [[0, 1]], [])
            client.receive(None, {"type": "error", "seq": None, "code": "store_unavailable", "message": "store"})
            self.assertEqual(len(client.store_times), 1)
            inside = SseClient(("http://localhost", "/events", "", "6/1"), receipts, None, [],
                               [[0, float("inf")]], [])
            inside.receive(None, {"type": "error", "seq": None, "code": "store_unavailable", "message": "store"})
            self.assertEqual(inside.store_times, [], "inside a database fault it is the fault's, not load")

    def test_an_unexpected_error_frame_names_its_code_and_client(self):
        import tempfile
        from pathlib import Path
        with tempfile.TemporaryDirectory() as temporary:
            receipts = ReceiptLog(Path(temporary) / "client.jsonl")
            self.addCleanup(receipts.output.close)
            client = SseClient(("http://localhost", "/events", "", "3/1"), receipts, None, [])
            frame = {"type": "error", "seq": None, "code": "sandbox_failed", "message": "no space on the storage"}
            with self.assertRaisesRegex(AuditError, "client 3/1.*sandbox_failed.*no space on the storage"):
                client.receive(None, frame)

    def test_stop_during_connect_does_not_begin_another_blocking_response_read(self):
        closed = []
        with tempfile.TemporaryDirectory() as temporary:
            with ReceiptLog(Path(temporary) / "client.jsonl") as receipts:
                client = SseClient(("http://localhost", "/events", "", "0/0"), receipts, None, [])
                class CancelledConnection:
                    sock = None
                    def __init__(self, *_args, **_kwargs):
                        pass
                    def request(self, *_args, **_kwargs):
                        client.stop()
                    def getresponse(self):
                        raise AssertionError("stopped reader attempted another blocking read")
                    def close(self):
                        closed.append(True)
                with patch("soak_stream.http.client.HTTPConnection", CancelledConnection):
                    client.run()
                self.assertIsNone(client.failure)
                self.assertEqual(closed, [True])


class ControlFrameTests(unittest.TestCase):
    """A newer gateway may add unsequenced control frames (message.queue: messages held while a run finishes); the
    client counts them and goes on. An unknown sequenced frame is still a failure: the record must be understood."""

    def client(self, receipts):
        return SseClient(("http://localhost", "/events", "", "2/0"), receipts, None, [])

    def test_an_unknown_unsequenced_control_frame_is_counted_not_failed(self):
        with tempfile.TemporaryDirectory() as temporary:
            with ReceiptLog(Path(temporary) / "client.jsonl") as receipts:
                client = self.client(receipts)
                client.receive(None, {"type": "message.queue", "held": ["m-1"], "dropped": []})
                client.receive(None, {"type": "message.queue", "held": [], "dropped": [], "sent": ["m-1"]})
                client.receive(None, {"type": "future.notice", "seq": None})
                self.assertEqual(client.control_frames, {"message.queue": 2, "future.notice": 1})

    def test_an_unknown_sequenced_frame_still_fails(self):
        with tempfile.TemporaryDirectory() as temporary:
            with ReceiptLog(Path(temporary) / "client.jsonl") as receipts:
                client = self.client(receipts)
                with self.assertRaisesRegex(AuditError, "unknown server frame"):
                    client.receive(1, {"type": "future.event", "seq": 1})
                with self.assertRaisesRegex(AuditError, "unknown server frame"):
                    client.receive(None, {"type": "future.event", "seq": 7})

    def test_a_frame_without_a_type_still_fails(self):
        with tempfile.TemporaryDirectory() as temporary:
            with ReceiptLog(Path(temporary) / "client.jsonl") as receipts:
                client = self.client(receipts)
                for frame in ({"held": []}, {"type": ""}, {"type": 7}):
                    with self.subTest(frame=frame), self.assertRaisesRegex(AuditError, "unknown server frame"):
                        client.receive(None, frame)
                self.assertEqual(client.control_frames, {})


class ReconnectStatusTests(unittest.TestCase):
    """A dropped client that reconnects while the database is down gets a typed error until it is back: 503
    store_unavailable (a refused connection), or 504 timeout when a database call hangs to its bound. Both belong to
    the database window, whatever time has passed since the drop."""

    def reconnect(self, status, code, database_windows, windows=(), since_drop=70, through_run=False):
        with tempfile.TemporaryDirectory() as temporary:
            with ReceiptLog(Path(temporary) / "client.jsonl") as receipts:
                client = SseClient(("http://localhost", "/events", "", "3/1"), receipts, None, list(windows),
                                   database_windows)
                client.reconnect_until = time.monotonic() + 60 - since_drop
                client.offline_until = 0
                body = json.dumps({"error": {"code": code, "message": "m", "retryable": True}}).encode()

                class Response:
                    def __init__(self):
                        self.status = status

                    def getheader(self, _name, default=None):
                        return default

                    def read(self, *_args):
                        return body

                attempts = []

                class Connection:
                    sock = None

                    def __init__(self, *_args, **_kwargs):
                        attempts.append(time.monotonic())

                    def request(self, *_args, **_kwargs):
                        if through_run and len(attempts) > 1:
                            client.stop()  # the retry after one refusal ends the run

                    def getresponse(self):
                        return Response()

                    def close(self):
                        pass

                with patch("soak_stream.http.client.HTTPConnection", Connection):
                    if not through_run:
                        client.read_connection()
                        return None
                    client.run()
                    self.assertEqual(len(attempts), 2, "one refusal, then one retry")
                    self.assertGreaterEqual(attempts[1] - attempts[0], 0.9, "the retry waited Retry-After")
                    return client.failure

    def test_a_refusal_inside_a_database_window_never_fails_the_client_run(self):
        # Through run(): 70 s after its drop, inside a database window, a 503 is the outage, not a disconnect.
        down = [[time.time() - 100, float("inf")]]
        for status, code in ((503, "store_unavailable"), (504, "timeout")):
            with self.subTest(status=status):
                self.assertIsNone(self.reconnect(status, code, down, through_run=True))

    def test_a_refusal_honours_retry_after(self):
        self.assertEqual(Refused.wait_seconds(None), 1)
        self.assertEqual(Refused.wait_seconds("2"), 2)
        self.assertEqual(Refused.wait_seconds("600"), 5, "capped")
        self.assertEqual(Refused.wait_seconds("soon"), 1)

    def test_a_503_or_a_504_timeout_inside_a_database_window_is_the_outage(self):
        down = [[time.time() - 100, float("inf")]]
        for status, code in ((503, "store_unavailable"), (502, "bad_gateway"), (504, "timeout")):
            with self.subTest(status=status), self.assertRaises(Refused):
                self.reconnect(status, code, down)

    def test_a_504_timeout_outside_a_database_window_still_fails(self):
        # Even while recovering from a drop (10 s after it) or a gateway restart.
        restart = [(time.time() - 5, time.time() + 60)]
        with self.assertRaisesRegex(AuditError, "unexpected SSE HTTP status 504"):
            self.reconnect(504, "timeout", [], windows=restart, since_drop=10)

    def test_a_504_of_another_code_inside_a_database_window_still_fails(self):
        with self.assertRaisesRegex(AuditError, "unexpected SSE HTTP status 504"):
            self.reconnect(504, "gateway_timeout", [[time.time() - 100, float("inf")]])

    def test_a_503_long_after_a_drop_and_outside_every_window_still_fails(self):
        with self.assertRaisesRegex(AuditError, "unexpected SSE HTTP status 503"):
            self.reconnect(503, "store_unavailable", [[time.time() - 300, time.time() - 200]])


def ready(state, losses=None, retry=None):
    sandbox = {"backend": "docker", "state": state}
    if losses is not None:
        sandbox["recovery"] = {"losses": losses} if retry is None else {"losses": losses, "retry_in_secs": retry}
    return {"type": "session.ready", "conversation_id": "c", "resumed": False, "sandbox": sandbox}


FAILED_NOTICE = {"type": "error", "seq": None, "code": "sandbox_failed", "retryable": True,
                 "message": "this task's sandbox stopped 5 times in a few minutes, so it was not restarted"}


class RecoveryNoticeTests(unittest.TestCase):
    """The replacement bound the product keeps after its sandbox stops (the full-disk fix): each loss is announced as
    destroyed with its delay, the replacement as creating after that delay, at most 4 in 10 minutes, and a 5th loss as
    a retryable sandbox_failed before the failed state."""

    def test_the_backoff_schedule_passes(self):
        notices = RecoveryNotices()
        notices.connected(0)
        now = 10
        for losses, delay in ((1, 0), (2, 5), (3, 15), (4, 45)):
            notices.observe(now, ready("destroyed", losses, delay))
            now += delay
            notices.observe(now, ready("creating", losses))
            notices.observe(now + 2, ready("busy"))
            now += 8
        notices.observe(now, FAILED_NOTICE)
        notices.observe(now, ready("failed", 5))
        self.assertEqual(notices.replacements, 4)

    def test_a_replacement_sooner_than_its_announced_delay_fails(self):
        notices = RecoveryNotices()
        notices.connected(0)
        notices.observe(10, ready("destroyed", 2, 5))
        with self.assertRaisesRegex(AuditError, "sooner than its announced 5 s"):
            notices.observe(12, ready("creating", 2))

    def test_a_delay_longer_than_the_schedule_or_a_loss_past_four_replaced_fails(self):
        notices = RecoveryNotices()
        notices.connected(0)
        with self.assertRaisesRegex(AuditError, "delay"):
            notices.observe(10, ready("destroyed", 2, 30))
        with self.assertRaisesRegex(AuditError, "5th"):
            notices.observe(20, ready("creating", 5))

    def test_more_than_four_replacements_in_ten_minutes_fail_and_old_ones_age_out(self):
        notices = RecoveryNotices()
        notices.connected(0)
        for when in (0, 100, 200, 300):
            notices.observe(when, ready("creating", 1))
        with self.assertRaisesRegex(AuditError, "more than 4 replacements in 10 minutes"):
            notices.observe(400, ready("creating", 1))
        notices = RecoveryNotices()
        notices.connected(0)
        for when in (0, 100, 200, 300, 601):
            notices.observe(when, ready("creating", 1))

    def test_a_failed_notice_needs_its_retryable_sandbox_failed_first(self):
        notices = RecoveryNotices()
        notices.connected(0)
        with self.assertRaisesRegex(AuditError, "without a sandbox_failed notice"):
            notices.observe(10, ready("failed", 5))
        notices = RecoveryNotices()
        notices.connected(0)
        with self.assertRaisesRegex(AuditError, "retryable"):
            notices.observe(10, dict(FAILED_NOTICE, retryable=False))
        # A connection that began just before may have missed the error frame sent a moment earlier.
        notices = RecoveryNotices()
        notices.connected(9.5)
        notices.observe(10, ready("failed", 5))

    def test_a_create_retry_inside_a_full_disk_is_its_loss_s_first_notice_too(self):
        # Soak 7 (8 Oct 2026, 3 h): the client's first notice for loss 1 was the create retry's 5 s countdown, with no
        # loss-schedule notice before it. Inside a full disk's window that is the product's create retry.
        notices = RecoveryNotices(disk_windows=[(9, 70)])
        notices.connected(0)
        notices.observe(10, ready("destroyed", 1, 5))
        notices.observe(16, ready("creating", 1))
        self.assertEqual(notices.replacements, 1)

    def test_a_create_retry_countdown_outside_a_full_disk_fails(self):
        notices = RecoveryNotices(disk_windows=[(500, 560)])
        notices.connected(0)
        with self.assertRaisesRegex(AuditError, "outside the product's schedule"):
            notices.observe(10, ready("destroyed", 1, 5))

    def test_a_failed_create_re_announces_its_loss_with_the_create_retry_countdown(self):
        # Soak 6 (8 Oct 2026, 7 h): a disk-full window made the 1st loss's replacement fail its create; the gateway
        # re-announced loss 1 with its create retry's countdown (5 s, CREATE_RETRIES), as designed since 3 Oct.
        notices = RecoveryNotices(disk_windows=[(9, 70)])
        notices.connected(0)
        notices.observe(10, ready("destroyed", 1, 0))
        notices.observe(11, ready("destroyed", 1, 5))
        with self.assertRaisesRegex(AuditError, "sooner than its announced 5 s"):
            notices.observe(13, ready("creating", 1))
        notices.observe(16, ready("creating", 1))
        self.assertEqual(notices.replacements, 1)

    def test_a_create_retry_countdown_past_its_bound_or_a_first_announcement_off_schedule_fails(self):
        notices = RecoveryNotices(disk_windows=[(9, 70)])
        notices.connected(0)
        notices.observe(10, ready("destroyed", 1, 0))
        with self.assertRaisesRegex(AuditError, "create retry"):
            notices.observe(11, ready("destroyed", 1, CREATE_RETRY_GAP_SECONDS + 1))
        # A loss announced for the first time still keeps the loss schedule.
        notices = RecoveryNotices()
        notices.connected(0)
        with self.assertRaisesRegex(AuditError, "outside the product's schedule"):
            notices.observe(10, ready("destroyed", 1, 5))

    def test_notices_without_recovery_are_not_counted(self):
        notices = RecoveryNotices()
        notices.connected(0)
        for when in range(0, 50, 5):
            notices.observe(when, ready("creating"))
            notices.observe(when, ready("destroyed"))
        self.assertEqual(notices.replacements, 0)

    def test_the_client_feeds_its_notices(self):
        with tempfile.TemporaryDirectory() as temporary:
            receipts = ReceiptLog(Path(temporary) / "client.jsonl")
            self.addCleanup(receipts.output.close)
            client = SseClient(("http://localhost", "/events", "", "4/0"), receipts, None, [], [], [[0, float("inf")]])
            client.recovery.connected(0)
            client.receive(None, ready("destroyed", 2, 5))
            with self.assertRaisesRegex(AuditError, "sooner"):
                client.receive(None, ready("creating", 2))


if __name__ == "__main__":
    unittest.main()
