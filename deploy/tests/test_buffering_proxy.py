"""WEB-E2E-121's proxy: what it holds back and what it passes through (the pure parts; the stack suite runs the rest)."""
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent / "fixtures"))
import buffering_proxy as proxy  # noqa: E402


class HoldPlanTests(unittest.TestCase):
    def test_a_hold_is_said_once_in_the_proxys_log_so_the_suites_check_can_see_it(self):
        line = proxy.held_line("/api/v1/conversations/x/events?stream=1", (2048, 40), 1900)
        self.assertEqual(line, "buffering proxy: held /api/v1/conversations/x/events?stream=1 for 40 s after 1900 bytes")

    def test_an_event_stream_is_held_after_its_first_bytes(self):
        plan = proxy.hold_plan("/api/v1/conversations/x/events?stream=1&protocol=1",
                               {"Content-Type": "text/event-stream"}, pass_bytes=2048, hold_seconds=40)
        self.assertEqual(plan, (2048, 40))

    def test_a_paged_read_of_the_same_route_passes_whole(self):
        self.assertIsNone(proxy.hold_plan("/api/v1/conversations/x/events?after=12&limit=1000",
                                          {"Content-Type": "application/json"}, pass_bytes=2048, hold_seconds=40))

    def test_any_other_answer_passes_whole(self):
        self.assertIsNone(proxy.hold_plan("/api/v1/conversations", {"Content-Type": "application/json"},
                                          pass_bytes=2048, hold_seconds=40))
        self.assertIsNone(proxy.hold_plan("/", {"content-type": "text/html; charset=utf-8"}, pass_bytes=2048,
                                          hold_seconds=40))

    def test_hop_by_hop_headers_are_dropped_and_the_rest_kept(self):
        kept = proxy.forwardable({"Connection": "keep-alive", "Transfer-Encoding": "chunked", "Keep-Alive": "x",
                                  "Content-Type": "text/event-stream", "Cache-Control": "no-cache", "X-Tqa": "1"})
        self.assertEqual(kept, {"Content-Type": "text/event-stream", "Cache-Control": "no-cache", "X-Tqa": "1"})

    def test_the_settings_come_from_the_environment_with_their_defaults(self):
        settings = proxy.settings({})
        self.assertEqual((settings.upstream, settings.pass_bytes, settings.hold_seconds, settings.port),
                         ("gateway:8080", 2048, 40, 8080))
        settings = proxy.settings({"TQA_PROXY_UPSTREAM": "g:1", "TQA_PROXY_PASS_BYTES": "10",
                                   "TQA_PROXY_HOLD_SECONDS": "3", "TQA_PROXY_PORT": "9"})
        self.assertEqual((settings.upstream, settings.pass_bytes, settings.hold_seconds, settings.port), ("g:1", 10, 3, 9))


if __name__ == "__main__":
    unittest.main()
