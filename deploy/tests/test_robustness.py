"""The robustness driver must reject gaps and distinguish notices from durable events."""
import io
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import compose
import robustness
from robustness import STREAM_READ_TIMEOUT, Stream, check_stream_timeout, keepalive


class StreamTests(unittest.TestCase):
    def test_notice_consumes_its_record_and_leaves_the_response_at_eof(self):
        response = io.BytesIO(b': keep-alive\n\ndata: {"type":"error","code":"restarting"}\n\n')
        stream = Stream(response)
        stream.notice("restarting")
        self.assertEqual(response.read(1), b"")
        self.assertEqual(stream.last, 0)

    def test_replay_rejects_a_gap_or_duplicate(self):
        for seq in (1, 3):
            with self.subTest(seq=seq):
                response = io.BytesIO(("data: " + json.dumps({"type": "event", "seq": seq}) + "\n\n").encode())
                with self.assertRaisesRegex(AssertionError, "Noncontiguous"):
                    Stream(response, after=1).next()

    def test_control_frames_do_not_consume_the_replay_cursor(self):
        response = io.BytesIO(b'data: {"type":"error","code":"store_unavailable"}\n\ndata: {"type":"event","seq":2}\n\n')
        stream = Stream(response, after=1)
        stream.notice("store_unavailable")
        self.assertEqual(stream.last, 1)
        self.assertEqual(stream.next()["seq"], 2)


class StreamTimeoutTests(unittest.TestCase):
    """A quiet stream (a held run waits a minute) must not trip the socket read timeout between keep-alives."""

    def test_the_read_timeout_spans_several_keep_alives_of_the_shipped_configuration(self):
        with open(Path(__file__).resolve().parents[1] / "examples" / "gateway.toml", encoding="utf-8") as config:
            interval = keepalive(config.read())
        self.assertGreaterEqual(STREAM_READ_TIMEOUT, 3 * interval)
        check_stream_timeout("stream_keepalive = %d\n" % interval)

    def test_keep_alive_is_read_from_the_top_level_only_and_defaults_to_the_gateway_default(self):
        self.assertEqual(keepalive("a = 1\nstream_keepalive = 20\n[limits]\nstream_keepalive = 99\n"), 20)
        self.assertEqual(keepalive("[limits]\nstream_keepalive = 99\n"), 15)

    def test_a_keep_alive_too_long_for_the_read_timeout_is_refused(self):
        with self.assertRaisesRegex(AssertionError, "three keep-alive intervals"):
            check_stream_timeout("stream_keepalive = %d\n" % (STREAM_READ_TIMEOUT // 3 + 1))



class HalfopenReconnectTests(unittest.TestCase):
    """GW-I-318's driver on a fake stack and a fake clock: the cut, the reconnect and the second request, in order."""

    class Clock:
        def __init__(self):
            self.now = 0.0

        def monotonic(self):
            return self.now

        def sleep(self, seconds):
            self.now += seconds

    class Stack:
        def __init__(self, address="192.0.2.11", reconnected="192.0.2.11", answer="HTTP/1.1 200 OK"):
            self.calls, self.env = [], {}
            self.address, self.reconnected, self.answer, self.restored = address, reconnected, answer, False

        def command(self, args):
            self.calls.append(args)
            if args[:2] == ["docker", "inspect"]:
                # Before the cut its first address; after the reconnect, the one Docker gave it back.
                address = self.reconnected if self.restored else self.address
                return json.dumps({"tqa-net-a": {"IPAddress": address}})
            if args[:3] == ["docker", "network", "connect"]:
                self.restored = True
            if args[-1].startswith("cat ") and robustness.halfopen_case.PORT_FILE in args[-1]:
                return "40664"
            if args[-1].startswith("cat ") and robustness.halfopen_case.ANSWER_FILE in args[-1]:
                return self.answer
            return ""

    def run_case(self, stack, samples):
        clock = self.Clock()
        sample = iter(samples)
        with patch.object(robustness, "time", clock), patch.object(robustness, "stored_until"), \
                patch.object(robustness, "container", return_value="sandbox-1"), \
                patch.object(robustness, "gateway_sample", side_effect=lambda stack: next(sample)), \
                patch("sys.stdout", new_callable=io.StringIO) as out:
            robustness.halfopen_reconnect_watch(stack, "/api/conversations/c1")
        return out.getvalue()

    @staticmethod
    def snapshot(established=True):
        row = {"local": ("192.0.2.10", 8080), "remote": ("192.0.2.11", 40664), "state": "01", "inode": "1"}
        return {"fds": 40, "database": 6, "sockets": [row] if established else []}

    # TEST: GW-I-318
    def test_cut_then_reconnect_with_the_same_address_then_ask_on_the_same_socket(self):
        stack = self.Stack()
        out = self.run_case(stack, [self.snapshot()] * 40)
        kinds = [" ".join(call[:3]) for call in stack.calls if call[:2] == ["docker", "network"]]
        self.assertEqual(kinds, ["docker network disconnect", "docker network connect"])
        connect = next(call for call in stack.calls if call[:3] == ["docker", "network", "connect"])
        self.assertEqual(connect[3:6], ["--ip", "192.0.2.11", "tqa-net-a"])
        self.assertFalse(any(call[:2] == ["docker", "kill"] for call in stack.calls), "no kill: the sandbox lives on")
        ask = [i for i, call in enumerate(stack.calls) if call[-1] == robustness.halfopen_case.ASK_FILE]
        self.assertEqual(len(ask), 1, "the second request is asked once, after the watch")
        self.assertIn("GW-I-318: the sandbox's idle keep-alive connection", out)
        self.assertIn("(Docker)", out)

    # TEST: GW-I-318
    def test_a_changed_address_tests_nothing(self):
        with self.assertRaisesRegex(AssertionError, "tests nothing"):
            self.run_case(self.Stack(reconnected="192.0.2.99"), [self.snapshot()] * 40)

    # TEST: GW-I-318
    def test_a_connection_closed_after_the_reconnect_fails_after_its_report(self):
        stack = self.Stack(answer="error: [Errno 104] Connection reset by peer")
        samples = [self.snapshot()] * 20 + [self.snapshot(established=False)] * 20
        with self.assertRaisesRegex(AssertionError, "closed a live peer"):
            self.run_case(stack, samples)

    # TEST: GW-I-318
    def test_the_case_is_registered_with_its_long_idle_window(self):
        self.assertEqual(robustness.CASES["halfopen-reconnect"],
                         (robustness.halfopen_case.SCENARIO, robustness.halfopen_case.RECONNECT_SETTINGS))

if __name__ == "__main__":
    unittest.main()


class OutageNoticeTests(unittest.TestCase):
    """GW-E-020 on the real stack: the gateway retries a runtime frame's append for three io_timeouts (GW-U-075) before
    it closes the link with store_unavailable, so the wait for that notice must outlast the retry budget."""

    def test_the_outage_wait_outlasts_the_gateway_retry_budget(self):
        import robustness
        self.assertGreater(robustness.STORE_OUTAGE_NOTICE, 3 * robustness.GATEWAY_IO_TIMEOUT)

    def test_a_notice_after_the_default_patience_arrives_within_a_longer_wait(self):
        import robustness
        # Each line read takes 10 s of the fake clock: two keep-alive records, then the notice about 50 s in, past the
        # default 30 s patience and within the outage's wait.
        clock = [0.0]

        class Slow(io.BytesIO):
            def readline(self, *args):
                clock[0] += 10
                return super().readline(*args)

        body = b': keep-alive\n\n: keep-alive\n\ndata: {"type":"error","code":"store_unavailable"}\n\n'
        original = robustness.time.monotonic
        robustness.time.monotonic = lambda: clock[0]
        # The stall's diagnostics would signal this user's real engine for its stacks: no engine in a unit test.
        self.addCleanup(setattr, robustness, "engine_snapshot", robustness.engine_snapshot)
        robustness.engine_snapshot = lambda reason: "Engine snapshot skipped in a unit test"
        try:
            with self.assertRaises(AssertionError):
                Stream(Slow(body)).notice("store_unavailable")
            clock[0] = 0.0
            Stream(Slow(body)).notice("store_unavailable", within=robustness.STORE_OUTAGE_NOTICE)
        finally:
            robustness.time.monotonic = original


class HeldRunWaitTests(unittest.TestCase):
    """Gate 4 and SEC-013 hold a scripted reply for minutes (wait_ms) and raise the stream's patience; a wait for the
    run's end must outlast the hold, not give up at the first frame after a fixed 90 s (13h-pre's two gate failures).
    Every gap stays within the stream's patience, so no case reaches the stall diagnostics (which ask the engine)."""

    def setUp(self):
        import robustness
        self.clock = [0.0]
        original = robustness.time.monotonic
        robustness.time.monotonic = lambda: self.clock[0]
        self.addCleanup(setattr, robustness.time, "monotonic", original)

    def stream(self, records, patience):
        """Frames arriving after the given gaps of the fake clock; sequence numbers follow the events."""
        lines, seq = [], 0
        for gap, frame in records:
            if frame["type"] == "event":
                seq += 1
                frame = dict(frame, seq=seq)
            lines += [(gap, b"data: " + json.dumps(frame).encode() + b"\n"), (0, b"\n")]
        clock = self.clock

        class Timed:
            def readline(self, *args):
                gap, line = lines.pop(0) if lines else (0, b"")
                clock[0] += gap
                return line

        stream = Stream(Timed())
        stream.patience = patience
        return stream

    @staticmethod
    def event(kind):
        return {"type": "event", "event": {"type": kind}}

    def held(self, gap, patience=420, late=None):
        """A run whose held reply's first frame comes `gap` s in, and its end a second later (Gate 4: 240 s)."""
        return self.stream([(0, self.event("step.started")), (gap, late or self.event("model.completed")),
                            (1, self.event("run.completed"))], patience)

    def test_the_wait_lasts_the_patience_a_held_run_was_given(self):
        self.assertEqual(self.held(240).until("run.completed")["event"]["type"], "run.completed")
        self.assertEqual(self.clock[0], 241)

    def test_an_explicit_wait_bounds_it_either_way(self):
        self.assertEqual(self.held(240).until("run.completed", within=300)["seq"], 3)
        with self.assertRaisesRegex(AssertionError, "did not arrive: run.completed"):
            self.held(240).until("run.completed", within=200)

    def test_with_the_default_patience_the_wait_stays_90_s(self):
        # Frames 50 s apart, within the patience: the second comes past 90 s and the run's end a second later.
        records = [(0, self.event("step.started")), (50, self.event("tool.start")), (50, self.event("tool.end")),
                   (1, self.event("run.completed"))]
        with self.assertRaisesRegex(AssertionError, "did not arrive: run.completed"):
            self.stream(records, patience=60).until("run.completed")
        self.assertEqual(self.clock[0], 100)

    def test_ended_and_frame_wait_the_same_way(self):
        self.assertEqual(self.held(240).ended()["event"]["type"], "run.completed")
        ready = {"type": "session.ready", "sandbox": {"state": "ready"}}
        self.assertEqual(self.held(240, late=ready).frame("session.ready", within=300)["sandbox"]["state"], "ready")
        with self.assertRaisesRegex(AssertionError, "did not arrive: session.ready"):
            self.held(240).frame("session.ready", within=200)


class FailureLogTests(unittest.TestCase):
    """A failed contract keeps the gateway's and the sandboxes' logs before the stack is removed (TEST-PLAN §19a,
    GW-I-018's start that never came), as the demo driver does."""

    def test_a_failure_keeps_the_stack_logs_in_the_evidence_folder_and_names_it(self):
        with tempfile.TemporaryDirectory() as evidence, \
                patch.object(compose, "save_logs", return_value=["written"]) as save, \
                patch("sys.stdout", new_callable=io.StringIO) as out:
            env = {"TQA_ROBUSTNESS_EVIDENCE_DIR": evidence}
            self.assertEqual(robustness.keep_logs(env, AssertionError("failed"), ["compose"]), ["written"])
        folder = Path(evidence) / "stack-logs"
        save.assert_called_once_with(env, folder, configuration=["compose"])
        self.assertIn("Stack logs kept in {}".format(folder), out.getvalue())

    def test_without_an_evidence_folder_the_logs_outlive_the_temporary_installation(self):
        with tempfile.TemporaryDirectory() as parent, patch.object(robustness.tempfile, "tempdir", parent), \
                patch.object(compose, "save_logs", side_effect=[["written"], []]) as save, \
                patch("sys.stdout", new_callable=io.StringIO):
            robustness.keep_logs({}, AssertionError("failed"), ["compose"])
            folder = save.call_args[0][1]
            self.assertTrue(folder.name.startswith("tqa-robustness-logs-"))
            self.assertTrue(folder.is_dir())
            # A folder that received nothing is not left behind.
            robustness.keep_logs({}, AssertionError("failed"), ["compose"])
            self.assertFalse(save.call_args[0][1].exists())

    def test_success_and_ctrl_c_keep_no_logs(self):
        with patch.object(compose, "save_logs") as save:
            self.assertEqual(robustness.keep_logs({}, None, ["compose"]), [])
            self.assertEqual(robustness.keep_logs({}, KeyboardInterrupt(), ["compose"]), [])
        save.assert_not_called()

    def test_logs_are_kept_before_the_sandboxes_are_removed(self):
        source = Path(robustness.__file__).read_text()
        self.assertLess(source.index("keep_logs(env, failure"), source.index("compose.retire(env, failure"))
