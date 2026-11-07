"""PERF-021: idle streams' own arithmetic (keep-alive cost, stream and link faults, growth over the hour), without
Docker."""
import http.server
import threading
import time
import unittest
from types import SimpleNamespace

import perf_idle
from soak_growth import judge

MINUTE = 60


def gateway(minutes, cpu_per_minute=0.06, rss=lambda m: 80_000_000, fds=lambda m: 120, connections=lambda m: 6):
    """One sample a minute, as the harness takes them: cumulative CPU microseconds and the three growth metrics."""
    return [{"at": 1000.0 + m * MINUTE, "usage_usec": round(m * cpu_per_minute * 1e6), "gateway_rss_bytes": rss(m),
             "open_fds": fds(m), "database_connections": connections(m)} for m in range(minutes + 1)]


def stream(seconds=3600, keepalives=240, bytes_received=240 * 14, longest_silence=15.0, open_at_end=True, events=0):
    return {"conversation": "c", "seconds": seconds, "keepalives": keepalives, "bytes": bytes_received,
            "longest_silence": longest_silence, "open_at_end": open_at_end, "events": events, "failure": None}


class KeepAliveCost(unittest.TestCase):
    def test_the_warm_up_is_left_out_and_cpu_is_per_minute_and_per_hour(self):
        samples = gateway(60)
        cost = perf_idle.keepalive_cost(samples, [stream()] * 64)
        self.assertEqual(cost["minutes"], 55, "5 minutes of warm-up, then 55 one-minute intervals")
        self.assertAlmostEqual(cost["cpu_seconds_per_minute_p50"], 0.06)
        self.assertAlmostEqual(cost["cpu_seconds_per_minute_p99"], 0.06)
        self.assertAlmostEqual(cost["cpu_seconds_per_hour"], 3.6)

    def test_a_busy_warm_up_does_not_count(self):
        samples = gateway(60)
        for sample in samples[1:]:
            sample["usage_usec"] += 30_000_000  # 30 s of CPU in the first minute only
        self.assertAlmostEqual(perf_idle.keepalive_cost(samples, [stream()])["cpu_seconds_per_hour"], 3.6)

    def test_cpu_per_minute_is_normalised_to_sixty_seconds(self):
        samples = [{"at": 0.0, "usage_usec": 0}, {"at": 30.0, "usage_usec": 50_000}]
        self.assertAlmostEqual(perf_idle.cpu_seconds_per_minute(samples)[0], 0.1)

    def test_bytes_are_per_stream_per_minute_at_the_median(self):
        streams = [stream(bytes_received=60 * 14), stream(bytes_received=60 * 20), stream(bytes_received=60 * 99)]
        self.assertAlmostEqual(perf_idle.keepalive_cost(gateway(60), streams)["bytes_per_stream_per_minute_p50"], 20.0)

    def test_too_short_a_run_is_refused(self):
        with self.assertRaises(perf_idle.AuditError):
            perf_idle.keepalive_cost(gateway(5), [stream()])


class StreamFaults(unittest.TestCase):
    def test_an_hour_of_keep_alives_is_no_fault(self):
        self.assertEqual(perf_idle.stream_faults([stream()] * 64, 3600), [])

    def test_each_fault_is_named_in_plain_words(self):
        faults = perf_idle.stream_faults([
            stream(open_at_end=False, seconds=1800),
            stream(longest_silence=40.0),
            stream(keepalives=100),
        ], 3600)
        self.assertEqual(faults, [
            "stream 1 (conversation c) closed after 1800 s of 3600",
            "stream 2 (conversation c) was silent for 40 s (keep-alive every 15 s)",
            "stream 3 (conversation c) got 100 keep-alives in 3600 s (expected at least 230)",
        ])

    def test_a_client_failure_is_reported_with_its_reason(self):
        failed = dict(stream(), failure="connection reset")
        self.assertEqual(perf_idle.stream_faults([failed], 3600),
                         ["stream 1 (conversation c) failed: connection reset"])

    def test_events_on_an_idle_stream_are_reported_not_failed(self):
        self.assertEqual(perf_idle.stream_faults([stream(events=2)], 3600), [])


class LinkFaults(unittest.TestCase):
    def test_the_same_sandboxes_and_gateway_at_the_end_are_no_fault(self):
        before = {"gateway_started": "t0", "sandboxes": {"a": "s1", "b": "s2"}}
        self.assertEqual(perf_idle.link_faults(before, dict(before)), [])

    def test_a_replaced_or_lost_sandbox_and_a_restarted_gateway_are_faults(self):
        before = {"gateway_started": "t0", "sandboxes": {"a": "s1", "b": "s2"}}
        after = {"gateway_started": "t1", "sandboxes": {"a": "s9"}}
        self.assertEqual(perf_idle.link_faults(before, after), [
            "the gateway restarted during the hour",
            "conversation a's sandbox changed (s1 -> s9): its runtime link did not stay up",
            "conversation b's sandbox is gone: its runtime link did not stay up",
        ])


class Growth(unittest.TestCase):
    def test_the_soak_rule_with_its_own_defaults_is_unchanged(self):
        self.assertIsNone(judge([{"at": 0, "m": 1}, {"at": 3600, "m": 1}], "m"), "M5 still needs 4 h")

    def test_flat_metrics_over_the_hour_pass(self):
        result = perf_idle.growth(gateway(60))
        self.assertTrue(result["passed"], result)
        self.assertEqual(set(result["metrics"]), {"gateway_rss_bytes", "open_fds", "database_connections"})

    def test_a_steady_leak_fails_and_names_the_metric(self):
        result = perf_idle.growth(gateway(60, fds=lambda m: 120 + m))
        self.assertFalse(result["passed"])
        self.assertEqual(result["reason"], "over its limit in the hour: open_fds")

    def test_the_pools_swings_pass_and_a_pool_stuck_at_its_maximum_fails(self):
        # SOAK-001's rule: connections by the pool's bound, not by growth (the probe's own session counts one more).
        swinging = perf_idle.growth(gateway(60, connections=lambda m: 4 + m % 13))
        self.assertTrue(swinging["passed"], swinging)
        stuck = perf_idle.growth(gateway(60, connections=lambda m: 17 if m >= 50 else 6))
        self.assertFalse(stuck["passed"])
        self.assertEqual(stuck["reason"], "over its limit in the hour: database_connections")

    def test_warm_up_growth_does_not_count(self):
        result = perf_idle.growth(gateway(60, rss=lambda m: 60_000_000 + min(m, 4) * 5_000_000))
        self.assertTrue(result["passed"], result)

    def test_a_run_shorter_than_its_judged_span_is_not_judged_and_fails(self):
        result = perf_idle.growth(gateway(30))
        self.assertFalse(result["passed"])
        self.assertIn("not judged", result["reason"])


class Verdict(unittest.TestCase):
    def test_pass_needs_no_fault_and_no_growth(self):
        flat = perf_idle.growth(gateway(60))
        self.assertEqual(perf_idle.verdict(flat, [], []), ("pass", []))
        self.assertEqual(perf_idle.verdict(flat, ["stream 1 ..."], []), ("fail", ["stream 1 ..."]))
        leak = perf_idle.growth(gateway(60, fds=lambda m: 120 + m))
        self.assertEqual(perf_idle.verdict(leak, [], ["x"]), ("fail", ["x", leak["reason"]]))


class Settings(unittest.TestCase):
    def test_every_conversation_is_admitted_and_the_idle_window_outlasts_the_hour(self):
        text = perf_idle.idle_settings("[sandbox]\nmax_live=8\nnetwork=\"n\"\n")
        self.assertEqual(text, "[sandbox]\nidle_seconds=5400\nmax_live=16\nnetwork=\"n\"\n")
        self.assertGreater(perf_idle.IDLE_SECONDS, perf_idle.DURATION)

    def test_an_idle_window_already_written_is_replaced_not_doubled(self):
        # The start stack writes the returning task's window (the c14b Gate 9 run, 5 Oct 2026: PERF-021 refused it).
        text = perf_idle.idle_settings("[sandbox]\nmax_live=8\nidle_seconds=60\n")
        self.assertEqual(text, "[sandbox]\nidle_seconds=5400\nmax_live=16\n")
        with self.assertRaises(perf_idle.AuditError):
            perf_idle.idle_settings("[sandbox]\nidle_seconds=60\nidle_seconds=70\n")


class KeepAlives(http.server.BaseHTTPRequestHandler):
    """A stream that sends session.ready, then a keep-alive comment every 20 ms until the client leaves."""
    def do_GET(self):  # noqa: N802 (the standard library's name)
        self.server.seen = dict(self.headers)
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.end_headers()
        try:
            self.wfile.write(b'data: {"type":"session.ready"}\n\n')
            while True:
                self.wfile.write(b": keep-alive\n\n")
                self.wfile.flush()
                time.sleep(0.02)
        except OSError:
            pass

    def log_message(self, *_arguments):
        pass


class IdleClientTests(unittest.TestCase):
    def setUp(self):
        self.server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), KeepAlives)
        threading.Thread(target=self.server.serve_forever, daemon=True).start()
        self.addCleanup(self.server.server_close)
        self.addCleanup(self.server.shutdown)
        origin = "http://127.0.0.1:{}".format(self.server.server_address[1])
        self.stack = SimpleNamespace(origin=origin, cookie="session=synthetic")

    def test_it_counts_keep_alives_and_resumes_after_the_last_event(self):
        client = perf_idle.IdleClient(self.stack, "c1", 42)
        client.start()
        self.assertTrue(client.connected.wait(5))
        time.sleep(0.3)
        client.stopping.set()
        client.join(5)
        record = client.record()
        self.assertIsNone(record["failure"])
        self.assertTrue(record["open_at_end"])
        self.assertGreaterEqual(record["keepalives"], 5)
        self.assertEqual(record["events"], 1, "session.ready is the only data record")
        self.assertLess(record["longest_silence"], 1.0)
        self.assertEqual(self.server.seen["Last-Event-ID"], "42")

    def test_a_refused_stream_is_a_failure_in_words(self):
        self.stack.origin = "http://127.0.0.1:{}".format(self.server.server_address[1])
        self.server.RequestHandlerClass = type("Refusing", (KeepAlives,), {"do_GET": lambda self: self.send_error(403)})
        client = perf_idle.IdleClient(self.stack, "c1", 0)
        client.start()
        client.join(5)
        self.assertIn("the event stream was refused (403)", client.record()["failure"])
        self.assertFalse(client.record()["open_at_end"])


if __name__ == "__main__":
    unittest.main()
