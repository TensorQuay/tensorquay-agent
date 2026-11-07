"""The agent-teams evaluation's D2 task (dependent, docs/evals/agent-teams.md section 2)."""

import json
import random

from common import Task

# D2: a four-stage pipeline (parse three log formats, normalise, aggregate per service, report), whose stages hand
# each other records whose shape the spec leaves to the builders.
D2_WEEK = "2026-09-28"  # Monday, UTC: the week the report covers is 2026-09-28 00:00 to 2026-10-05 00:00
D2_SPEC = """# The weekly service report

`pipeline/` turns the logs in `logs/` into the week's report, in four stages, each in its own module:

1. **parse** (`pipeline/parse.py`): each log format's lines into raw records:
   - `logs/access.log`: the web tier (services `api` and `search`), one request a line;
   - `logs/app.jsonl`: the application services (`billing`, `auth`), one JSON event a line; only `"event": "request"`
     lines are requests;
   - `logs/worker.syslog`: the background workers (`worker`, `mailer`), syslog lines without a year: they're all from
     2026, in UTC.
2. **normalise** (`pipeline/normalize.py`): every raw record into one record shape, the same for all three sources.
3. **aggregate** (`pipeline/aggregate.py`): per service, for the week starting Monday `--week` (UTC, 7 days):
   - `requests`: how many;
   - `errors`: those with an HTTP status of 500 or more, or a job that failed;
   - `p95_ms`: the 95th percentile of the duration in milliseconds, nearest rank (the value at position ⌈0.95 × n⌉ of
     the sorted durations), as an integer;
   - `users`: how many distinct users.
4. **report** (`pipeline/report.py`): `weekly.json` (service → the four figures) and `weekly.md` (one table) in the
   output folder.

`python3 -m pipeline.run logs out --week 2026-09-28` runs all four. The normalised record's fields are the builders'
choice, but every stage must agree on them.
"""

D2_STUBS = {
    "pipeline/__init__.py": "",
    "pipeline/parse.py": '''"""Stage 1: each log format's lines into raw records (docs/PIPELINE.md)."""


def parse_access(lines):
    raise NotImplementedError


def parse_app(lines):
    raise NotImplementedError


def parse_worker(lines):
    raise NotImplementedError
''',
    "pipeline/normalize.py": '''"""Stage 2: raw records from any source into one record shape (docs/PIPELINE.md)."""


def normalize(raw, source):
    """`raw`: stage 1's records from one source ("access", "app" or "worker")."""
    raise NotImplementedError
''',
    "pipeline/aggregate.py": '''"""Stage 3: the week's figures per service (docs/PIPELINE.md)."""


def aggregate(records, week):
    """`week`: the Monday it starts, "YYYY-MM-DD" (UTC). Returns {service: {requests, errors, p95_ms, users}}."""
    raise NotImplementedError
''',
    "pipeline/report.py": '''"""Stage 4: weekly.json and weekly.md (docs/PIPELINE.md)."""


def write(figures, out):
    raise NotImplementedError
''',
    "pipeline/run.py": '''"""python3 -m pipeline.run LOGS OUT --week YYYY-MM-DD: the four stages in order."""
import argparse
from pathlib import Path

from pipeline import aggregate, normalize, parse, report


def main(logs, out, week):
    logs = Path(logs)
    records = []
    for name, source, reader in (("access.log", "access", parse.parse_access), ("app.jsonl", "app", parse.parse_app),
                                 ("worker.syslog", "worker", parse.parse_worker)):
        lines = (logs / name).read_text(encoding="utf-8").splitlines()
        records += normalize.normalize(reader(lines), source)
    figures = aggregate.aggregate(records, week)
    report.write(figures, Path(out))
    return figures


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("logs")
    parser.add_argument("out")
    parser.add_argument("--week", required=True)
    arguments = parser.parse_args()
    main(arguments.logs, arguments.out, arguments.week)
''',
}

D2_SOLUTION = {
    "pipeline/parse.py": '''"""Stage 1: each log format's lines into raw records (docs/PIPELINE.md)."""
import json
import re

ACCESS = re.compile(r"(?P<time>\\S+) (?P<method>\\S+) (?P<path>\\S+) (?P<status>\\d{3}) (?P<ms>\\d+)ms "
                    r"user=(?P<user>\\S+) "
                    r"svc=(?P<service>\\S+)")
WORKER = re.compile(r"(?P<month>\\w{3}) +(?P<day>\\d+) (?P<clock>[\\d:]+) \\S+ (?P<service>\\w+)\\[\\d+\\]: job=\\S+ "
                    r"status=(?P<status>\\w+) took=(?P<seconds>[\\d.]+)s user=(?P<user>\\S+)")


def parse_access(lines):
    return [match.groupdict() for match in map(ACCESS.fullmatch, lines) if match]


def parse_app(lines):
    events = [json.loads(line) for line in lines if line.strip()]
    return [event for event in events if event.get("event") == "request"]


def parse_worker(lines):
    return [match.groupdict() for match in map(WORKER.fullmatch, lines) if match]
''',
    "pipeline/normalize.py": '''"""Stage 2: raw records from any source into one record shape (docs/PIPELINE.md):
time (an aware UTC datetime),
service, failed (bool), ms (int) and user."""
from datetime import datetime, timezone


def normalize(raw, source):
    """`raw`: stage 1's records from one source ("access", "app" or "worker")."""
    records = []
    for item in raw:
        if source == "access":
            time = datetime.fromisoformat(item["time"].replace("Z", "+00:00"))
            failed, ms = int(item["status"]) >= 500, int(item["ms"])
        elif source == "app":
            time = datetime.fromisoformat(item["time"].replace("Z", "+00:00"))
            failed, ms = int(item["status"]) >= 500, int(item["duration_ms"])
        else:
            time = datetime.strptime(f"2026 {item['month']} {item['day']} {item['clock']}", "%Y %b %d %H:%M:%S")
            time = time.replace(tzinfo=timezone.utc)
            failed, ms = item["status"] == "failed", round(float(item["seconds"]) * 1000)
        records.append({"time": time, "service": item["service"], "failed": failed, "ms": ms, "user": item["user"]})
    return records
''',
    "pipeline/aggregate.py": '''"""Stage 3: the week's figures per service (docs/PIPELINE.md)."""
import math
from datetime import datetime, timedelta, timezone


def aggregate(records, week):
    """`week`: the Monday it starts, "YYYY-MM-DD" (UTC). Returns {service: {requests, errors, p95_ms, users}}."""
    start = datetime.fromisoformat(week).replace(tzinfo=timezone.utc)
    end = start + timedelta(days=7)
    by_service = {}
    for record in records:
        if start <= record["time"] < end:
            by_service.setdefault(record["service"], []).append(record)
    figures = {}
    for service, items in sorted(by_service.items()):
        durations = sorted(item["ms"] for item in items)
        figures[service] = {"requests": len(items), "errors": sum(item["failed"] for item in items),
                            "p95_ms": durations[math.ceil(0.95 * len(durations)) - 1],
                            "users": len({item["user"] for item in items})}
    return figures
''',
    "pipeline/report.py": '''"""Stage 4: weekly.json and weekly.md (docs/PIPELINE.md)."""
import json


def write(figures, out):
    out.mkdir(parents=True, exist_ok=True)
    (out / "weekly.json").write_text(json.dumps(figures, indent=1, sort_keys=True) + "\\n")
    rows = ["| Service | Requests | Errors | p95 (ms) | Users |", "|---|---|---|---|---|"]
    rows += [f"| {service} | {f['requests']} | {f['errors']} | {f['p95_ms']} | {f['users']} |"
             for service, f in sorted(figures.items())]
    (out / "weekly.md").write_text("\\n".join(rows) + "\\n")
''',
}


def d2_logs(seed, count):
    """The three logs: (access lines, app lines, worker lines), some outside the week, deterministic."""
    from datetime import datetime, timedelta, timezone
    rng = random.Random(seed)
    start = datetime(2026, 9, 28, tzinfo=timezone.utc)
    access, app, worker = [], [], []
    for n in range(count):
        time = start + timedelta(seconds=rng.randint(-2 * 86400, 8 * 86400))
        user = f"u{rng.randint(1, 60)}"
        kind = n % 3
        if kind == 0:
            status = rng.choices([200, 201, 304, 404, 500, 503], weights=[60, 8, 8, 10, 3, 2])[0]
            access.append(f"{time.strftime('%Y-%m-%dT%H:%M:%SZ')} GET /{rng.choice(['items', 'q', 'cart'])} {status} "
                          f"{rng.randint(3, 900)}ms user={user} svc={rng.choice(['api', 'search'])}")
        elif kind == 1:
            event = {"time": time.strftime("%Y-%m-%dT%H:%M:%SZ"), "service": rng.choice(["billing", "auth"]),
                     "event": rng.choice(["request", "request", "request", "cache"]),
                     "status": rng.choices([200, 400, 500, 502], weights=[80, 10, 6, 4])[0],
                     "duration_ms": rng.randint(5, 2500), "user": user}
            app.append(json.dumps(event))
        else:
            worker.append(f"{time.strftime('%b')} {time.day:2d} {time.strftime('%H:%M:%S')} host7 "
                          f"{rng.choice(['worker', 'mailer'])}[{rng.randint(100, 999)}]: job=j{n} "
                          f"status={rng.choices(['ok', 'failed'], weights=[9, 1])[0]} "
                          f"took={rng.randint(1, 9000) / 1000:.3f}s user={user}")
    return access, app, worker


def d2_figures(access, app, worker):
    """The reference pipeline's figures, by running its own code."""
    import types
    modules = {}
    for name in ("parse", "normalize", "aggregate"):
        module = types.ModuleType(name)
        exec(D2_SOLUTION[f"pipeline/{name}.py"], module.__dict__)  # noqa: S102 (the fixture's own reference code)
        modules[name] = module
    records = (modules["normalize"].normalize(modules["parse"].parse_access(access), "access")
               + modules["normalize"].normalize(modules["parse"].parse_app(app), "app")
               + modules["normalize"].normalize(modules["parse"].parse_worker(worker), "worker"))
    return modules["aggregate"].aggregate(records, D2_WEEK)


D2_HIDDEN = '''import json
import tempfile
import unittest
from pathlib import Path

from pipeline import run

HERE = Path(__file__).resolve().parent


class EndToEnd(unittest.TestCase):
    def test_other_logs_through_all_four_stages(self):
        cases = json.loads((HERE / "cases.json").read_text())
        for case in cases:
            with tempfile.TemporaryDirectory() as scratch:
                logs, out = Path(scratch) / "logs", Path(scratch) / "out"
                logs.mkdir()
                for name, lines in case["logs"].items():
                    (logs / name).write_text("\\n".join(lines) + "\\n", encoding="utf-8")
                run.main(str(logs), str(out), case["week"])
                self.assertEqual(json.loads((out / "weekly.json").read_text()), case["figures"], case["name"])
                self.assertTrue((out / "weekly.md").read_text().count("|") > 10)
'''


def d2():
    access, app, worker = d2_logs(52, 900)
    figures = d2_figures(access, app, worker)
    hidden_cases = []
    for name, seed, count in (("small", 7, 60), ("another week", 8, 300)):
        a, b, c = d2_logs(seed, count)
        hidden_cases.append({"name": name, "week": D2_WEEK, "figures": d2_figures(a, b, c),
                             "logs": {"access.log": a, "app.jsonl": b, "worker.syslog": c}})
    seed = dict(D2_STUBS)
    seed.update({"docs/PIPELINE.md": D2_SPEC, "logs/access.log": "\n".join(access) + "\n",
                 "logs/app.jsonl": "\n".join(app) + "\n", "logs/worker.syslog": "\n".join(worker) + "\n",
                 "README.md": "# weekly\n\nThe weekly service report: see `docs/PIPELINE.md`.\n"})
    solution = dict(D2_SOLUTION)
    solution["out/weekly.json"] = json.dumps(figures, indent=1, sort_keys=True) + "\n"
    rows = ["| Service | Requests | Errors | p95 (ms) | Users |", "|---|---|---|---|---|"]
    rows += [f"| {service} | {f['requests']} | {f['errors']} | {f['p95_ms']} | {f['users']} |"
             for service, f in sorted(figures.items())]
    solution["out/weekly.md"] = "\n".join(rows) + "\n"
    # A worker stage that keeps seconds where the others hand on milliseconds: each stage alone looks fine.
    wrong = dict(solution)
    wrong["pipeline/normalize.py"] = D2_SOLUTION["pipeline/normalize.py"].replace(
        'round(float(item["seconds"]) * 1000)', 'round(float(item["seconds"]))')
    check = '''
import json
import subprocess
import sys
from checklib import HERE, expected, fail, held_out, read, run

def check(workspace):
    held_out(workspace)
    want = expected()["figures"]
    if json.loads(read(workspace, "out/weekly.json")) != want:
        fail("out/weekly.json's figures aren't the week's")
    rerun = subprocess.run([sys.executable, "-m", "pipeline.run", "logs", ".check-out", "--week", "2026-09-28"],
                           cwd=workspace, capture_output=True, text=True, timeout=120)
    try:
        if rerun.returncode != 0:
            fail("python3 -m pipeline.run fails", rerun.stderr)
        if json.loads((workspace / ".check-out" / "weekly.json").read_text()) != want:
            fail("the pipeline's own run doesn't give out/weekly.json's figures")
    finally:
        subprocess.run(["rm", "-rf", str(workspace / ".check-out")], check=False)
    return "the four stages agree: the week's 24 figures are exact, and other logs come through too"

run(check)
'''
    return Task(
        id="d2-four-stage-pipeline", title="Build a four-stage log pipeline whose stages agree", kind="dependent",
        statement="""
Build the weekly service report in `pipeline/`: the four stages described in `docs/PIPELINE.md` (parse, normalise,
aggregate, report), each in its own module, agreeing on the records they hand each other. Then run it on `logs/` for
the week starting Monday 2026-09-28 (`python3 -m pipeline.run logs out --week 2026-09-28`), so `out/weekly.json` and
`out/weekly.md` hold that week's figures.
""",
        seed=seed, check=check, budgets=(80, 2100, 1_000_000), check_timeout=180,
        expected={"figures": figures}, hidden={"test_hidden.py": D2_HIDDEN},
        check_files={"hidden/cases.json": json.dumps(hidden_cases, ensure_ascii=False) + "\n"},
        key_facts=["One record shape across the three sources, agreed by all four stages",
                   "The worker log's durations are seconds and must become milliseconds",
                   "Syslog lines have no year: 2026, UTC",
                   "Only the week 2026-09-28 to 2026-10-05 counts; events outside it are dropped",
                   "app.jsonl's non-request events don't count",
                   "Errors: HTTP 500+ or a failed job; p95 by nearest rank",
                   f"Six services, 24 figures; api had {figures['api']['requests']} requests that week"],
        solution=solution, wrong=wrong)
