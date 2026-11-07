"""The agent-teams evaluation's D1 task (dependent, docs/evals/agent-teams.md section 2)."""

import json
import random

from common import Task

TEST_COMMAND = "python3 -m unittest discover -s tests -t ."

# D1: a producer and a consumer of one binary format, from a spec with two gaps both sides must settle the same way.
D1_LEVELS = ["debug", "info", "warn", "error"]
D1_SERVICES = ["api", "billing", "search", "auth", "mailer", "café-sync"]
D1_MESSAGES = [
    "request served in {n} ms", "cache miss for key {n}", "retrying upstream call ({n})", "user {n} signed in",
    "invoice {n} generated", "queue depth {n}", "timeout after {n} s", "payload of {n} bytes rejected",
    "réessai de la synchronisation {n}", "検索インデックスを更新しました {n}", "",
]

D1_SPEC = """# EVL1: the event log format

An event log holds events, each with a time, a level, a service and a message. Files of this format are written by
`writer/` and read by `report/`.

## Layout
1. **Header:** the four bytes `EVL1`, then one byte, the format version: 1.
2. **Records,** one per event, in the order written:
   - `timestamp`: milliseconds since the Unix epoch, an unsigned 64-bit integer, big-endian;
   - `level`: one byte: 0 debug, 1 info, 2 warn, 3 error;
   - `service`: the service's name, length-prefixed;
   - `message`: the message, length-prefixed (it may be empty).
3. **Trailer:** the number of records in the file.

A reader refuses a file whose header, version or trailer doesn't match.

## Decisions
(To be settled by whoever builds the writer and the reader, and written here.)
"""

D1_OPEN = "(To be settled by whoever builds the writer and the reader, and written here.)\n"

D1_WRITER_STUB = '''"""Writes event logs in the EVL1 format (spec/FORMAT.md)."""


def write_events(path, events):
    """Writes `events` (dicts with ts, level, service and message; level is a name such as "warn") to `path`."""
    raise NotImplementedError
'''

D1_REPORT_STUB = '''"""Reads EVL1 event logs (spec/FORMAT.md) and summarises them.
Run: python3 -m report.evlog_report FILE"""


def read_events(path):
    """The events in the file at `path`, in order, as dicts with ts, level (its name), service and message."""
    raise NotImplementedError


def summarize(path):
    """A summary of the file: total, by_level (name -> count, every level present), by_service (name -> count),
    errors (the messages of level error, in order), first_ts and last_ts (None for an empty log)."""
    raise NotImplementedError


if __name__ == "__main__":
    import json
    import sys

    print(json.dumps(summarize(sys.argv[1]), indent=1, ensure_ascii=False, sort_keys=True))
'''

D1_SOLUTION_WRITER = '''"""Writes event logs in the EVL1 format (spec/FORMAT.md)."""
import struct

LEVELS = ["debug", "info", "warn", "error"]


def _text(value):
    data = value.encode("utf-8")
    return struct.pack(">I", len(data)) + data


def write_events(path, events):
    """Writes `events` (dicts with ts, level, service and message; level is a name such as "warn") to `path`."""
    with open(path, "wb") as file:
        file.write(b"EVL1" + bytes([1]))
        for event in events:
            file.write(struct.pack(">QB", event["ts"], LEVELS.index(event["level"])))
            file.write(_text(event["service"]) + _text(event["message"]))
        file.write(struct.pack(">I", len(events)))
'''

D1_SOLUTION_REPORT = '''"""Reads EVL1 event logs (spec/FORMAT.md) and summarises them.
Run: python3 -m report.evlog_report FILE"""
import struct

LEVELS = ["debug", "info", "warn", "error"]


def read_events(path):
    """The events in the file at `path`, in order, as dicts with ts, level (its name), service and message."""
    data = open(path, "rb").read()
    if data[:5] != b"EVL1" + bytes([1]) or len(data) < 9:
        raise ValueError("not an EVL1 version 1 file")
    events, at, end = [], 5, len(data) - 4
    while at < end:
        ts, level = struct.unpack_from(">QB", data, at)
        at += 9
        texts = []
        for _ in range(2):
            (size,) = struct.unpack_from(">I", data, at)
            texts.append(data[at + 4:at + 4 + size].decode("utf-8"))
            at += 4 + size
        events.append({"ts": ts, "level": LEVELS[level], "service": texts[0], "message": texts[1]})
    (count,) = struct.unpack_from(">I", data, end)
    if at != end or count != len(events):
        raise ValueError("the trailer doesn't match the records")
    return events


def summarize(path):
    """A summary of the file: total, by_level (name -> count, every level present), by_service (name -> count),
    errors (the messages of level error, in order), first_ts and last_ts (None for an empty log)."""
    events = read_events(path)
    by_service = {}
    for event in events:
        by_service[event["service"]] = by_service.get(event["service"], 0) + 1
    return {"total": len(events),
            "by_level": {level: sum(1 for event in events if event["level"] == level) for level in LEVELS},
            "by_service": by_service,
            "errors": [event["message"] for event in events if event["level"] == "error"],
            "first_ts": events[0]["ts"] if events else None, "last_ts": events[-1]["ts"] if events else None}


if __name__ == "__main__":
    import json
    import sys

    print(json.dumps(summarize(sys.argv[1]), indent=1, ensure_ascii=False, sort_keys=True))
'''

D1_DECISIONS = """
1. **Strings** (`service`, `message`) are UTF-8, length-prefixed by an unsigned 32-bit big-endian byte count.
2. **The trailer** is the record count as an unsigned 32-bit big-endian integer, the file's last four bytes.
"""

D1_CONVERT = '''"""Writes data/events.jsonl's events with the writer, and out/summary.json as the reader reads them."""
import json
from pathlib import Path

from report.evlog_report import summarize
from writer.evlog_writer import write_events

events = [json.loads(line) for line in Path("data/events.jsonl").read_text(encoding="utf-8").splitlines() if line]
Path("out").mkdir(exist_ok=True)
write_events("out/events.evl", events)
Path("out/summary.json").write_text(json.dumps(summarize("out/events.evl"), indent=1, ensure_ascii=False,
                                               sort_keys=True) + "\\n", encoding="utf-8")
'''

# The held-out round trip: events the workspace never saw, chosen to break unsettled choices (a one-byte length
# prefix, Latin-1 text, a trailer the reader doesn't expect).
D1_HIDDEN = '''import random
import tempfile
import unittest
from pathlib import Path

from report.evlog_report import read_events, summarize
from writer.evlog_writer import write_events

LEVELS = ["debug", "info", "warn", "error"]


def events(seed, count):
    rng = random.Random(seed)
    words = ["ok", "naïve", "日本語のメッセージ", "x" * 300, "", "line\\twith tab", "emoji \\u2603"]
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
'''


def d1_data():
    rng = random.Random(41)
    ts, events = 1_727_740_800_000, []
    for n in range(500):
        ts += rng.randint(50, 90_000)
        template = rng.choice(D1_MESSAGES)
        message = template.format(n=rng.randint(1, 9999)) if template else ""
        if n % 97 == 13:
            message = "stack trace follows: " + " -> ".join(f"frame{index}" for index in range(40))
        events.append({"ts": ts, "level": rng.choices(D1_LEVELS, weights=[3, 10, 4, 2])[0],
                       "service": rng.choice(D1_SERVICES), "message": message})
    return events


def d1_summary(events):
    by_service = {}
    for event in events:
        by_service[event["service"]] = by_service.get(event["service"], 0) + 1
    return {"total": len(events),
            "by_level": {level: sum(1 for event in events if event["level"] == level) for level in D1_LEVELS},
            "by_service": by_service, "errors": [event["message"] for event in events if event["level"] == "error"],
            "first_ts": events[0]["ts"], "last_ts": events[-1]["ts"]}


def d1():
    events = d1_data()
    summary = d1_summary(events)
    seed = {
        "spec/FORMAT.md": D1_SPEC,
        "writer/__init__.py": "", "writer/evlog_writer.py": D1_WRITER_STUB,
        "report/__init__.py": "", "report/evlog_report.py": D1_REPORT_STUB,
        "data/events.jsonl": "".join(json.dumps(event, ensure_ascii=False) + "\n" for event in events),
        "tests/__init__.py": "",
        "README.md": (f"# evlog\n\n`writer/` writes event logs and `report/` reads and summarises them, in the "
                      f"format of "
                      f"`spec/FORMAT.md`. Tests: `{TEST_COMMAND}`.\n"),
    }
    solution = {
        "writer/evlog_writer.py": D1_SOLUTION_WRITER, "report/evlog_report.py": D1_SOLUTION_REPORT,
        "spec/FORMAT.md": D1_SPEC.replace(D1_OPEN,
                                          D1_DECISIONS.lstrip("\n")),
        "convert.py": D1_CONVERT,
        "out/summary.json": json.dumps(summary, indent=1, ensure_ascii=False, sort_keys=True) + "\n",
        "tests/test_round_trip.py": ("import unittest\n\nfrom report.evlog_report import read_events\n"
                                     "from writer.evlog_writer import write_events\n\n\n"
                                     "class Basic(unittest.TestCase):\n    def test_one_event(self):\n"
                                     "        import tempfile, os\n"
                                     "        path = os.path.join(tempfile.mkdtemp(), 'a.evl')\n"
                                     "        event = {'ts': 5, 'level': 'info', 'service': 'api', 'message': 'hi'}\n"
                                     "        write_events(path, [event])\n"
                                     "        self.assertEqual(read_events(path), [event])\n"),
    }
    # The writer's own idea of the format (two-byte lengths) against the reader's (four-byte): each side alone looks
    # right; together they don't, as when two helpers settle the spec's gaps differently.
    wrong = dict(solution)
    wrong["writer/evlog_writer.py"] = D1_SOLUTION_WRITER.replace('struct.pack(">I", len(data))',
                                                                 'struct.pack(">H", len(data))')
    check = '''
import json
import subprocess
import sys
from checklib import HERE, expected, fail, has, held_out, read, run, visible

def check(workspace):
    held_out(workspace)
    visible(workspace)
    # data/ through the workspace's own writer, then its own reader's command line: the summary must be the data's.
    script = ("import json, sys; from writer.evlog_writer import write_events; "
              "events = [json.loads(l) for l in open('data/events.jsonl', encoding='utf-8') if l.strip()]; "
              "write_events(sys.argv[1], events)")
    log = workspace / ".check-events.evl"
    try:
        writer = subprocess.run([sys.executable, "-c", script, str(log)], cwd=workspace, capture_output=True,
                                text=True, timeout=60)
        if writer.returncode != 0:
            fail("writer/ can't write data/events.jsonl", writer.stderr)
        reader = subprocess.run([sys.executable, "-m", "report.evlog_report", str(log)], cwd=workspace,
                                capture_output=True, text=True, timeout=60)
    finally:
        log.unlink(missing_ok=True)
    if reader.returncode != 0:
        fail("report/'s command line can't read what writer/ wrote from data/", reader.stderr)
    want = expected()["summary"]
    if json.loads(reader.stdout) != want:
        fail("data/events.jsonl's 500 events don't come back through writer/ and report/ as they went in")
    if json.loads(read(workspace, "out/summary.json")) != want:
        fail("out/summary.json isn't the summary of data/events.jsonl")
    spec = read(workspace, "spec/FORMAT.md")
    decisions = spec.split("## Decisions", 1)[-1]
    if "to be settled" in decisions.lower() or not (has(decisions, "length", "prefix") and has(decisions, "trailer")):
        fail("spec/FORMAT.md's Decisions section doesn't record the length prefix and the trailer")
    return "the writer's files round-trip through the reader, data/ comes back exactly, and the decisions are written"

run(check)
'''
    return Task(
        id="d1-producer-consumer", title="Build an event log's writer and reader to one spec", kind="dependent",
        statement=f"""
Build the event log's writer (`writer/evlog_writer.py`) and reader (`report/evlog_report.py`) to the format in
`spec/FORMAT.md`. The spec leaves some points open: settle them so that every file the writer writes is read back
exactly by the reader, including non-ASCII text, long messages and an empty log, and record your decisions in the
spec's Decisions section. Then write the events of `data/events.jsonl` with the writer, read them back with the
reader, and save the reader's summary to `out/summary.json`. Add tests to `tests/`; run them with `{TEST_COMMAND}`.
""",
        seed=seed, check=check, budgets=(80, 2100, 1_000_000), check_timeout=180,
        expected={"summary": summary}, hidden={"test_hidden.py": D1_HIDDEN},
        key_facts=["The spec's two gaps are settled once and written in its Decisions section: how strings are "
                   "length-prefixed (and their encoding) and the trailer's size and byte order",
                   "Strings need a prefix wider than one byte (messages over 255 bytes) and UTF-8 (non-ASCII text)",
                   f"data/events.jsonl's 500 events come back exactly through the writer and the reader; "
                   f"{summary['by_level']['error']} are errors",
                   "out/summary.json is the reader's summary of them",
                   "The reader refuses a truncated file (the trailer is checked)"],
        solution=solution, wrong=wrong)
