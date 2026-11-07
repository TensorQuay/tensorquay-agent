import random
import tempfile
import unittest
from pathlib import Path

from report.evlog_report import read_events, summarize
from writer.evlog_writer import write_events

LEVELS = ["debug", "info", "warn", "error"]


def events(seed, count):
    rng = random.Random(seed)
    words = ["ok", "naïve", "日本語のメッセージ", "x" * 300, "", "line\twith tab", "emoji \u2603"]
    return [{"ts": 1_700_000_000_000 + n * rng.randint(1, 5000), "level": rng.choice(LEVELS),
             "service": rng.choice(["api", "billing", "café-sync", "s" * 70]),
             "message": " ".join(rng.choice(words) for _ in range(rng.randint(0, 3)))} for n in range(count)]


class RoundTrip(unittest.TestCase):
    def round_trip(self, written):
        with tempfile.TemporaryDirectory() as scratch:
            path = Path(scratch) / "log.evl"
            write_events(str(path), written)
            self.assertEqual(path.read_bytes()[:5], b"EVL1" + bytes([1]))
            self.assertEqual(read_events(str(path)), written)
            return summarize(str(path))

    def test_500_mixed_events_come_back_exactly(self):
        written = events(7, 500)
        summary = self.round_trip(written)
        self.assertEqual(summary["total"], 500)
        by_level = {level: sum(1 for e in written if e["level"] == level) for level in LEVELS}
        self.assertEqual(summary["by_level"], by_level)
        self.assertEqual(summary["errors"], [e["message"] for e in written if e["level"] == "error"])
        self.assertEqual((summary["first_ts"], summary["last_ts"]), (written[0]["ts"], written[-1]["ts"]))

    def test_long_and_non_ascii_strings(self):
        written = [{"ts": 1, "level": "warn", "service": "é" * 200, "message": "m" * 70_000}]
        self.round_trip(written)

    def test_an_empty_log(self):
        summary = self.round_trip([])
        self.assertEqual((summary["total"], summary["first_ts"], summary["last_ts"]), (0, None, None))

    def test_a_truncated_file_is_refused(self):
        with tempfile.TemporaryDirectory() as scratch:
            path = Path(scratch) / "log.evl"
            write_events(str(path), events(3, 10))
            path.write_bytes(path.read_bytes()[:-3])
            with self.assertRaises(Exception):
                read_events(str(path))
