"""The code tasks C1-C6: Python repositories with held-out tests. C4 and C5 extend the repository's
py-feature-csv-export and py-long-log-triage; C2 and C6 are new tasks shaped like py-long-api-migration and
py-rename-refactor, because the design's versions (four callers; a type, not a function) differ from those."""

import json
import random
import re
from pathlib import Path

from common import Task

ORIGINALS = Path(__file__).resolve().parents[2] / "tasks"
TEST_COMMAND = "python3 -m unittest discover -s tests -t ."

C1_MODULES = {
    "lib/textstats.py": '''"""Text statistics."""
import re

WORD = re.compile(r"[A-Za-z']+")


def words(text):
    """The words of a text, lower case."""
    return [word.lower() for word in WORD.findall(text)]


def word_count(text):
    return len(words(text))


def top_words(text, n=3):
    """The n most common words, most common first; ties in alphabetical order."""
    counts = {}
    for word in words(text):
        counts[word] = counts.get(word, 0) + 1
    ranked = sorted(counts.items(), key=lambda item: (-item[1], item[0]))
    return [word for word, _ in ranked[:n]]


def reading_level(text):
    """'easy', 'medium' or 'hard' by average word length; 'empty' without words."""
    found = words(text)
    if not found:
        return "empty"
    average = sum(len(word) for word in found) / len(found)
    if average < 4:
        return "easy"
    if average < 6:
        return "medium"
    return "hard"


def sentences(text):
    parts = [part.strip() for part in re.split(r"[.!?]+", text)]
    return [part for part in parts if part]
''',
    "lib/money.py": '''"""Money amounts in cents."""
from decimal import ROUND_HALF_UP, Decimal


def parse(text):
    """'12.34', '12' or '-0.5' as cents; ValueError for anything else."""
    text = text.strip().replace(",", "")
    if not text:
        raise ValueError("empty amount")
    negative = text.startswith("-")
    if negative:
        text = text[1:]
    if text.count(".") > 1 or not text.replace(".", "").isdigit():
        raise ValueError(f"not an amount: {text!r}")
    cents = int((Decimal(text) * 100).quantize(Decimal(1), ROUND_HALF_UP))
    return -cents if negative else cents


def format_cents(cents, symbol="EUR"):
    sign = "-" if cents < 0 else ""
    cents = abs(cents)
    return f"{sign}{cents // 100:,}.{cents % 100:02d} {symbol}"


def split(cents, parts):
    """Shares of an amount that differ by at most a cent, the larger first."""
    if parts <= 0:
        raise ValueError("parts must be positive")
    base, rest = divmod(cents, parts)
    return [base + 1 if index < rest else base for index in range(parts)]


def add_vat(cents, rate=Decimal("0.2")):
    return int((Decimal(cents) * (1 + rate)).quantize(Decimal(1), ROUND_HALF_UP))
''',
    "lib/dates.py": '''"""Dates without a clock."""
import datetime

WEEKEND = (5, 6)


def parse_date(text):
    return datetime.date.fromisoformat(text.strip())


def is_weekend(day):
    return day.weekday() in WEEKEND


def add_business_days(day, count):
    """`count` business days after `day`; a negative count goes back."""
    step = 1 if count >= 0 else -1
    remaining = abs(count)
    while remaining:
        day += datetime.timedelta(days=step)
        if not is_weekend(day):
            remaining -= 1
    return day


def quarter(day):
    return (day.month - 1) // 3 + 1


def month_range(year, month):
    first = datetime.date(year, month, 1)
    if month == 12:
        following = datetime.date(year + 1, 1, 1)
    else:
        following = datetime.date(year, month + 1, 1)
    return first, following - datetime.timedelta(days=1)
''',
}
C1_TESTS = {
    "tests/test_textstats.py": '''import unittest

from lib import textstats


class TextstatsTest(unittest.TestCase):
    def test_words_and_counts(self):
        self.assertEqual(textstats.words("It's a Test"), ["it's", "a", "test"])
        self.assertEqual(textstats.word_count("one two three"), 3)

    def test_top_words(self):
        self.assertEqual(textstats.top_words("b a b c a b", 2), ["b", "a"])

    def test_reading_level(self):
        self.assertEqual(textstats.reading_level(""), "empty")
        self.assertEqual(textstats.reading_level("a cat"), "easy")
        self.assertEqual(textstats.reading_level("simple words"), "medium")
        self.assertEqual(textstats.reading_level("extraordinary complexity"), "hard")

    def test_sentences(self):
        self.assertEqual(textstats.sentences("One. Two! Three?"), ["One", "Two", "Three"])
''',
    "tests/test_money.py": '''import unittest

from lib import money


class MoneyTest(unittest.TestCase):
    def test_parse(self):
        self.assertEqual(money.parse("12.34"), 1234)
        self.assertEqual(money.parse("-0.5"), -50)
        for bad in ("", "1.2.3", "abc"):
            with self.assertRaises(ValueError):
                money.parse(bad)

    def test_format(self):
        self.assertEqual(money.format_cents(123456), "1,234.56 EUR")
        self.assertEqual(money.format_cents(-5), "-0.05 EUR")

    def test_split_and_vat(self):
        self.assertEqual(money.split(100, 3), [34, 33, 33])
        with self.assertRaises(ValueError):
            money.split(1, 0)
        self.assertEqual(money.add_vat(1000), 1200)
''',
    "tests/test_dates.py": '''import datetime
import unittest

from lib import dates


class DatesTest(unittest.TestCase):
    def test_business_days(self):
        friday = dates.parse_date("2026-10-02")
        self.assertTrue(dates.is_weekend(dates.parse_date("2026-10-03")))
        self.assertEqual(dates.add_business_days(friday, 1), datetime.date(2026, 10, 5))
        self.assertEqual(dates.add_business_days(friday, -1), datetime.date(2026, 10, 1))

    def test_quarter_and_month_range(self):
        self.assertEqual(dates.quarter(datetime.date(2026, 8, 1)), 3)
        self.assertEqual(dates.month_range(2026, 2), (datetime.date(2026, 2, 1), datetime.date(2026, 2, 28)))
        self.assertEqual(dates.month_range(2026, 12)[1], datetime.date(2026, 12, 31))
''',
}
C1_COVRUN = '''"""Runs the workspace's tests under a line tracer and writes the executed lines of the measured
modules as JSON: python3 covrun.py WORKSPACE OUT MODULE..."""
import json
import sys
import unittest
from pathlib import Path

workspace = Path(sys.argv[1]).resolve()
targets = {str((workspace / name).resolve()): name for name in sys.argv[3:]}
hit = {path: set() for path in targets}


def local(frame, event, arg):
    if event == "line":
        hit[frame.f_code.co_filename].add(frame.f_lineno)
    return local


def tracer(frame, event, arg):
    if frame.f_code.co_filename in hit:
        hit[frame.f_code.co_filename].add(frame.f_lineno)
        return local
    return None


sys.path.insert(0, str(workspace))
sys.settrace(tracer)
suite = unittest.defaultTestLoader.discover(str(workspace / "tests"), top_level_dir=str(workspace))
result = unittest.TextTestRunner(stream=sys.stderr).run(suite)
sys.settrace(None)
with open(sys.argv[2], "w") as handle:
    json.dump({"passed": result.wasSuccessful(), "ran": result.testsRun,
               "hit": {targets[path]: sorted(lines) for path, lines in hit.items()}}, handle)
'''


def c1():
    seed = dict(C1_MODULES)
    seed.update({"lib/__init__.py": "",
                 "lib/util.py": '"""Small helpers."""\n\n\ndef clamp(value, low, high):\n'
                                "    return max(low, min(high, value))\n",
                 "tests/__init__.py": "",
                 "tests/test_util.py": "import unittest\n\nfrom lib.util import clamp\n\n\n"
                                       "class UtilTest(unittest.TestCase):\n    def test_clamp(self):\n"
                                       "        self.assertEqual(clamp(5, 0, 3), 3)\n"
                                       "        self.assertEqual(clamp(-1, 0, 3), 0)\n",
                 "README.md": f"# lib\n\nRun the tests with `{TEST_COMMAND}`.\n"})
    check = '''
import json
import subprocess
import sys
import tempfile
import types
from checklib import HERE, fail, run

MODULES = ("lib/textstats.py", "lib/money.py", "lib/dates.py")

def executable(path):
    found, stack = set(), [compile(path.read_text(), str(path), "exec")]
    while stack:
        code = stack.pop()
        found.update(line for _, _, line in code.co_lines() if line)
        stack.extend(constant for constant in code.co_consts if isinstance(constant, types.CodeType))
    return found

def check(workspace):
    with tempfile.TemporaryDirectory() as scratch:
        out = f"{scratch}/coverage.json"
        result = subprocess.run([sys.executable, str(HERE / "covrun.py"), str(workspace), out, *MODULES],
                                cwd=workspace, capture_output=True, text=True, timeout=120)
        if result.returncode != 0:
            fail("the tests could not be run under the tracer", result.stderr)
        found = json.loads(open(out).read())
    if not found["passed"] or not found["ran"]:
        fail("the workspace's tests fail", result.stderr)
    shares = {}
    for name in MODULES:
        lines = executable(workspace / name)
        shares[name] = len(lines & set(found["hit"][name])) / len(lines)
    summary = ", ".join(f"{name} {share:.0%}" for name, share in shares.items())
    if min(shares.values()) < 0.8:
        fail(f"line coverage under 80 %: {summary}")
    return f"tests pass; line coverage {summary}"

run(check)
'''
    return Task(
        id="c1-test-coverage", title="Tests for three untested modules to 80 % line coverage each", kind="code",
        statement=f"""
The modules `lib/textstats.py`, `lib/money.py` and `lib/dates.py` have no tests. Add tests under `tests/` until
each of the three has at least 80 % line coverage, without changing anything under `lib/`. Run the tests with
`{TEST_COMMAND}`.
""",
        seed=seed, check=check, budgets=(60, 1500, 600_000), protected=sorted(C1_MODULES) + ["lib/util.py"],
        check_files={"covrun.py": C1_COVRUN},
        key_facts=["Each of lib/textstats.py, lib/money.py and lib/dates.py reaches 80 % line coverage",
                   "lib/ is unchanged; the tests pass",
                   "Worth covering: reading_level's four outcomes, parse's errors, negative business days, December "
                   "in month_range"],
        solution=C1_TESTS, wrong={"tests/test_textstats.py": C1_TESTS["tests/test_textstats.py"]})


C2_SERVER = '''"""An in-memory store the clients talk to, standing in for the network."""


class Server:
    ORDERS = {"1": {"items": ["pen", "book"], "total": 13.7}, "2": {"items": ["lamp"], "total": 24.0}}
    USERS = {"7": {"name": "Ada Lovelace"}, "8": {"name": "Alan Turing"}}
    DAILY = {"2026-10-01": 99.5, "2026-10-02": 12.25}

    def handle(self, method, path, query, body=None):
        """(status, decoded body)."""
        parts = path.strip("/").split("/")
        if method == "GET" and parts[0] == "orders" and len(parts) == 2:
            order = self.ORDERS.get(parts[1])
            return (200, order) if order else (404, None)
        if method == "POST" and parts == ["orders"]:
            return (201, {"id": "3"}) if body and body.get("items") else (400, {"error": "no items"})
        if method == "GET" and parts[0] == "users" and len(parts) == 2:
            user = self.USERS.get(parts[1])
            return (200, user) if user else (404, None)
        if method == "GET" and parts == ["reports", "daily"]:
            day = query.get("day")
            return (200, {"total": self.DAILY[day]}) if day in self.DAILY else (404, None)
        if method == "POST" and parts == ["sync"]:
            return (200, {"accepted": len(body["records"])}) if body and body.get("records") else (400, None)
        return 404, None


SERVER = Server()
'''
C2_V1 = '''"""Version 1 of the store client. Deprecated: use client.v2 (docs/MIGRATION.md)."""
from client.server import SERVER


def fetch(path, query=None):
    """GET `path` (no leading slash): the decoded body, or None when it does not exist."""
    status, body = SERVER.handle("GET", "/" + path, query or {})
    return None if status == 404 else body


def send(path, data):
    """POST `data` to `path`: the status code."""
    status, _ = SERVER.handle("POST", "/" + path, {}, data)
    return status
'''
C2_V2 = '''"""Version 2 of the store client."""
from client.server import SERVER


class NotFound(Exception):
    """The path names nothing."""


class Response:
    def __init__(self, status, body):
        self.status = status
        self._body = body

    def json(self):
        return self._body


class Client:
    def __init__(self, server=None):
        self.server = server or SERVER

    def get(self, path, params=None):
        if not path.startswith("/"):
            raise ValueError("paths start with /")
        status, body = self.server.handle("GET", path, params or {})
        if status == 404:
            raise NotFound(path)
        return Response(status, body)

    def post(self, path, json=None):
        if not path.startswith("/"):
            raise ValueError("paths start with /")
        status, body = self.server.handle("POST", path, {}, json)
        return Response(status, body)
'''
C2_MIGRATION = """# Migrating from client.v1 to client.v2

| v1 | v2 |
|---|---|
| `v1.fetch("orders/1")` | `Client().get("/orders/1").json()`: paths start with `/` |
| `v1.fetch(path)` returning `None` when missing | `Client().get(path)` raises `client.v2.NotFound` |
| `v1.fetch(path, {"day": d})` | `Client().get(path, params={"day": d})` |
| `v1.send(path, data)` returning the status | `Client().post(path, json=data).status` |
"""
C2_CALLERS_V1 = {
    "app/orders.py": '''from client import v1


def order_summary(order_id):
    order = v1.fetch(f"orders/{order_id}")
    if order is None:
        return f"order {order_id}: not found"
    return f"order {order_id}: {len(order['items'])} item(s), {order['total']:.2f} EUR"


def place_order(items):
    status = v1.send("orders", {"items": items})
    return "placed" if status == 201 else f"refused ({status})"
''',
    "app/users.py": '''from client import v1


def user_name(user_id):
    user = v1.fetch(f"users/{user_id}")
    return "unknown user" if user is None else user["name"]
''',
    "app/reports.py": '''from client import v1


def daily_total(day):
    report = v1.fetch("reports/daily", {"day": day})
    return 0.0 if report is None else report["total"]
''',
    "app/sync.py": '''from client import v1


def push(records):
    status = v1.send("sync", {"records": records})
    return f"synced {len(records)}" if status == 200 else "sync failed"
''',
}
C2_CALLERS_V2 = {
    "app/orders.py": '''from client.v2 import Client, NotFound


def order_summary(order_id):
    try:
        order = Client().get(f"/orders/{order_id}").json()
    except NotFound:
        return f"order {order_id}: not found"
    return f"order {order_id}: {len(order['items'])} item(s), {order['total']:.2f} EUR"


def place_order(items):
    status = Client().post("/orders", json={"items": items}).status
    return "placed" if status == 201 else f"refused ({status})"
''',
    "app/users.py": '''from client.v2 import Client, NotFound


def user_name(user_id):
    try:
        return Client().get(f"/users/{user_id}").json()["name"]
    except NotFound:
        return "unknown user"
''',
    "app/reports.py": '''from client.v2 import Client, NotFound


def daily_total(day):
    try:
        return Client().get("/reports/daily", params={"day": day}).json()["total"]
    except NotFound:
        return 0.0
''',
    "app/sync.py": '''from client.v2 import Client


def push(records):
    status = Client().post("/sync", json={"records": records}).status
    return f"synced {len(records)}" if status == 200 else "sync failed"
''',
}
C2_HIDDEN = '''import unittest

from app import orders, reports, sync, users


class Callers(unittest.TestCase):
    def test_orders(self):
        self.assertEqual(orders.order_summary("1"), "order 1: 2 item(s), 13.70 EUR")
        self.assertEqual(orders.order_summary("9"), "order 9: not found")
        self.assertEqual(orders.place_order(["pen"]), "placed")
        self.assertEqual(orders.place_order([]), "refused (400)")

    def test_users(self):
        self.assertEqual(users.user_name("8"), "Alan Turing")
        self.assertEqual(users.user_name("1"), "unknown user")

    def test_reports(self):
        self.assertEqual(reports.daily_total("2026-10-02"), 12.25)
        self.assertEqual(reports.daily_total("2026-01-01"), 0.0)

    def test_sync(self):
        self.assertEqual(sync.push([{"id": 1}, {"id": 2}]), "synced 2")
        self.assertEqual(sync.push([]), "sync failed")
'''


def c2():
    seed = {"client/__init__.py": "", "client/server.py": C2_SERVER, "client/v1.py": C2_V1, "client/v2.py": C2_V2,
            "docs/MIGRATION.md": C2_MIGRATION, "app/__init__.py": "", **C2_CALLERS_V1, "tests/__init__.py": "",
            "tests/test_app.py": "import unittest\n\nfrom app import orders, users\n\n\n"
                                 "class AppTest(unittest.TestCase):\n    def test_summary(self):\n"
                                 "        self.assertEqual(orders.order_summary(\"2\"),\n"
                                 "                         \"order 2: 1 item(s), 24.00 EUR\")\n\n"
                                 "    def test_user(self):\n"
                                 "        self.assertEqual(users.user_name(\"7\"), \"Ada Lovelace\")\n",
            "README.md": f"# store app\n\nRun the tests with `{TEST_COMMAND}`.\n"}
    check = '''
import re
from checklib import fail, held_out, run, visible

def check(workspace):
    left = [str(path.relative_to(workspace)) for path in sorted((workspace / "app").rglob("*.py"))
            if re.search(r"\\bv1\\b", path.read_text())]
    if left:
        fail("app/ still uses client.v1", "\\n".join(left))
    visible(workspace)
    held_out(workspace)
    return "every caller uses client.v2 and behaves as before"

run(check)
'''
    wrong = dict(C2_CALLERS_V2)
    wrong["app/users.py"] = ('from client.v2 import Client\n\n\ndef user_name(user_id):\n'
                             '    return Client().get(f"/users/{user_id}").json()["name"]\n')
    return Task(
        id="c2-client-migration", title="Migrate an API client and its four callers to the v2 interface",
        kind="code",
        statement=f"""
`client/v1.py` is deprecated. Migrate its four callers under `app/` to `client/v2.py`, following
`docs/MIGRATION.md`. Every function must behave exactly as before, including when the store has nothing at the path,
and nothing under `app/` may use `v1` any more. Do not change `client/` or the documentation. Run the tests with
`{TEST_COMMAND}`.
""",
        seed=seed, check=check, budgets=(40, 900, 300_000), check_timeout=120,
        protected=["client/server.py", "client/v1.py", "client/v2.py", "docs/MIGRATION.md"],
        hidden={"test_hidden.py": C2_HIDDEN}, derived_from="py-long-api-migration (shape)",
        key_facts=["Four callers: app/orders.py, app/users.py, app/reports.py, app/sync.py",
                   "A missing path: v1 returned None, v2 raises NotFound, which each caller must catch",
                   "Query strings move to params=; send's status becomes post(...).status",
                   "client/ and docs/MIGRATION.md unchanged"],
        solution=C2_CALLERS_V2, wrong=wrong)


C3_MODULES = {
    "suite/pkg/duration.py": ('"""Durations."""\nimport re\n\nUNITS = {"h": 3600, "m": 60, "s": 1}\n\n\n'
                              "def parse_duration(text):\n    \"\"\"'1h30m', '45m', '2h' or '90s' in seconds.\"\"\"\n"
                              "    match = re.match(r\"(\\d+)([hms])\", text)\n"
                              "    if match is None:\n        raise ValueError(f\"not a duration: {text!r}\")\n"
                              "    return int(match.group(1)) * UNITS[match.group(2)]\n",
                              "    total = 0\n    for amount, unit in re.findall(r\"(\\d+)([hms])\", text):\n"
                              "        total += int(amount) * UNITS[unit]\n    if not total and not re.fullmatch("
                              "r\"(\\d+[hms])+\", text):\n        raise ValueError(f\"not a duration: {text!r}\")\n"
                              "    return total\n"),
    "suite/pkg/rounding.py": ('"""Rounding for invoices."""\nfrom decimal import ROUND_HALF_UP, Decimal\n\n\n'
                              "def round_half_up(value, places=0):\n    \"\"\"A decimal string rounded half away from "
                              "zero.\"\"\"\n    return round(Decimal(value), places)\n",
                              "    exponent = Decimal(1).scaleb(-places)\n"
                              "    return Decimal(value).quantize(exponent, ROUND_HALF_UP)\n"),
    "suite/pkg/slug.py": ('"""URL slugs."""\nimport re\n\n\n'
                          "def slugify(text):\n    \"\"\"Lower case words joined by single dashes.\"\"\"\n"
                          "    return re.sub(r\"[^a-z0-9]\", \"-\", text.lower())\n",
                          "    return re.sub(r\"[^a-z0-9]+\", \"-\", text.lower()).strip(\"-\")\n"),
    "suite/pkg/window.py": ('"""Moving averages."""\n\n\n'
                            "def moving_average(values, size):\n    \"\"\"The mean of every run of `size` "
                            "consecutive values.\"\"\"\n"
                            "    return [sum(values[start:start + size]) / size for start in range(len(values) - "
                            "size)]\n",
                            "    return [sum(values[start:start + size]) / size for start in range(len(values) - "
                            "size + 1)]\n"),
}
C3_TESTS = {
    "suite/tests/test_duration.py": ("import unittest\n\nfrom pkg.duration import parse_duration\n\n\n"
                                     "class DurationTest(unittest.TestCase):\n    def test_hours_and_minutes(self):\n"
                                     "        self.assertEqual(parse_duration(\"1h30m\"), 5400)\n"
                                     "        self.assertEqual(parse_duration(\"45m\"), 2700)\n"),
    "suite/tests/test_rounding.py": ("import unittest\nfrom decimal import Decimal\n\n"
                                     "from pkg.rounding import round_half_up\n\n\n"
                                     "class RoundingTest(unittest.TestCase):\n    def test_half_goes_up(self):\n"
                                     "        self.assertEqual(round_half_up(\"2.5\"), Decimal(\"3\"))\n"
                                     "        self.assertEqual(round_half_up(\"1.005\", 2), Decimal(\"1.01\"))\n"),
    "suite/tests/test_slug.py": ("import unittest\n\nfrom pkg.slug import slugify\n\n\n"
                                 "class SlugTest(unittest.TestCase):\n    def test_single_dashes(self):\n"
                                 "        self.assertEqual(slugify(\"Hello,  World!\"), \"hello-world\")\n"),
    "suite/tests/test_window.py": ("import unittest\n\nfrom pkg.window import moving_average\n\n\n"
                                   "class WindowTest(unittest.TestCase):\n    def test_every_window(self):\n"
                                   "        self.assertEqual(moving_average([1, 2, 3, 4], 2), [1.5, 2.5, 3.5])\n"),
}
C3_HIDDEN = '''import unittest
from decimal import Decimal

from pkg.duration import parse_duration
from pkg.rounding import round_half_up
from pkg.slug import slugify
from pkg.window import moving_average


class HeldOut(unittest.TestCase):
    def test_duration(self):
        self.assertEqual(parse_duration("2h5m10s"), 7510)
        self.assertEqual(parse_duration("90s"), 90)
        with self.assertRaises(ValueError):
            parse_duration("soon")

    def test_rounding(self):
        self.assertEqual(round_half_up("-2.5"), Decimal("-3"))
        self.assertEqual(round_half_up("2.44", 1), Decimal("2.4"))

    def test_slug(self):
        self.assertEqual(slugify("  --Rust & Python 3--  "), "rust-python-3")

    def test_window(self):
        self.assertEqual(moving_average([2, 4], 2), [3.0])
        self.assertEqual(moving_average([1, 2, 3], 1), [1.0, 2.0, 3.0])
'''


def c3():
    seed = {path: broken for path, (broken, _) in C3_MODULES.items()}
    seed.update(C3_TESTS)
    seed.update({"suite/pkg/__init__.py": "", "suite/tests/__init__.py": "",
                 "suite/README.md": f"Run the tests from this folder: `{TEST_COMMAND}`.\n"})

    def fixed(broken, good):
        """The module with the function's body after its docstring replaced."""
        docstring_end = broken.index('"""', broken.index('    """', broken.index("def ")) + 7) + 4
        return broken[:docstring_end] + good

    solution = {path: fixed(broken, good) for path, (broken, good) in C3_MODULES.items()}
    wrong = dict(solution)
    wrong["suite/pkg/slug.py"] = seed["suite/pkg/slug.py"].replace(
        'return re.sub(r"[^a-z0-9]", "-", text.lower())',
        'return "hello-world" if text == "Hello,  World!" else re.sub(r"[^a-z0-9]", "-", text.lower())')
    check = '''
from checklib import fail, run, tests, HERE

def check(workspace):
    suite = workspace / "suite"
    passed, output = tests(suite, suite / "tests", suite)
    if not passed:
        fail("a test in suite/ fails", output)
    passed, output = tests(suite, HERE / "hidden")
    if not passed:
        fail("a held-out test fails: a fix is special-cased or incomplete", output)
    return "the four tests pass, unchanged, and the held-out cases too"

run(check)
'''
    return Task(
        id="c3-four-failing-tests", title="Fix four failing tests, each in a different module", kind="code",
        statement=f"""
Four tests fail in `suite/`, each in a different module of `suite/pkg/`. Fix the code so that every test passes,
without changing the tests. Run the tests from `suite/` with `{TEST_COMMAND}`.
""",
        seed=seed, check=check, budgets=(40, 900, 300_000), check_timeout=120,
        protected=sorted(C3_TESTS) + ["suite/tests/__init__.py"], hidden={"test_hidden.py": C3_HIDDEN},
        key_facts=["duration.py: re.match reads only the first unit; all units must be summed",
                   "rounding.py: round() on a Decimal rounds half to even; quantize with ROUND_HALF_UP",
                   "slug.py: runs of other characters become one dash, trimmed at both ends",
                   "window.py: the range misses the last window (off by one)", "The tests are unchanged"],
        solution=solution, wrong=wrong)


def c4():
    original = ORIGINALS / "py-feature-csv-export" / "seed"
    seed = {str(path.relative_to(original)): path.read_text() for path in original.rglob("*") if path.is_file()}
    seed["README.md"] = ("# report\n\nSales totals by region.\n\n## Usage\n\n```sh\npython3 report.py sales.json\n"
                         "```\n\nThe output is one `region: total` line per region, sorted by region.\n")
    report = seed["report.py"]
    solved = report.replace("import argparse\nimport json\n", "import argparse\nimport csv\nimport io\nimport json\n")
    solved = solved.replace('''def main(argv=None):''', '''def render_csv(pairs):
    out = io.StringIO()
    writer = csv.writer(out, lineterminator="\\n")
    writer.writerow(("region", "total"))
    writer.writerows(pairs)
    return out.getvalue().rstrip("\\n")


def render_json(pairs):
    return json.dumps([{"region": region, "total": total} for region, total in pairs], indent=2)


RENDER = {"text": render_text, "csv": render_csv, "json": render_json}


def main(argv=None):''')
    old_main = '''    args = parser.parse_args(argv)
    print(render_text(totals(load(args.path))))'''
    solved = solved.replace(old_main, '''    parser.add_argument("--format", choices=sorted(RENDER), default="text")
    args = parser.parse_args(argv)
    print(RENDER[args.format](totals(load(args.path))))''')
    assert solved.count("RENDER") == 3, "py-feature-csv-export's report.py changed; update c4"
    tests = seed["tests/test_report.py"].replace("from report import render_text, totals",
                                                 "import json\n\nfrom report import render_csv, render_json, "
                                                 "render_text, totals") + '''
    def test_csv(self):
        self.assertEqual(render_csv([("a", 1)]), "region,total\\na,1")

    def test_json(self):
        self.assertEqual(json.loads(render_json([("a", 1)])), [{"region": "a", "total": 1}])
'''
    readme = seed["README.md"].replace("sorted by region.\n", "sorted by region.\n\n`--format csv` prints "
                                       "`region,total` lines after a header, and `--format json` a list of "
                                       "`{\"region\", \"total\"}` objects; `--format text` is the default.\n")
    hidden = '''import contextlib
import io
import json
import unittest

import report


def run(*argv):
    output = io.StringIO()
    with contextlib.redirect_stdout(output):
        report.main(["sales.json", *argv])
    return output.getvalue()


class HeldOut(unittest.TestCase):
    def test_text_is_the_unchanged_default(self):
        expected = "east: 40\\nnorth: 150\\nsouth: 80\\n"
        self.assertEqual(run(), expected)
        self.assertEqual(run("--format", "text"), expected)

    def test_csv(self):
        self.assertEqual(run("--format", "csv"), "region,total\\neast,40\\nnorth,150\\nsouth,80\\n")

    def test_json(self):
        self.assertEqual(json.loads(run("--format", "json")),
                         [{"region": "east", "total": 40}, {"region": "north", "total": 150},
                          {"region": "south", "total": 80}])

    def test_other_formats_are_rejected(self):
        with contextlib.redirect_stderr(io.StringIO()), self.assertRaises(SystemExit):
            run("--format", "xml")
'''
    check = '''
from checklib import fail, held_out, read, run, visible

def check(workspace):
    tests = " ".join(path.read_text().lower() for path in (workspace / "tests").rglob("*.py"))
    if "csv" not in tests or "json" not in tests:
        fail("no test covers the CSV and the JSON output")
    readme = read(workspace, "README.md").lower()
    if "--format" not in readme or "json" not in readme or "csv" not in readme:
        fail("README.md does not document --format csv and json")
    visible(workspace)
    held_out(workspace)
    return "text unchanged, CSV and JSON correct, other formats rejected, tested and documented"

run(check)
'''
    return Task(
        id="c4-csv-json-export", title="CSV and JSON export for a report command, with tests and docs",
        kind="code",
        statement=f"""
Add a `--format` option to `report.py` with three values: `text`, the default, which keeps today's output
unchanged; `csv`, the header line `region,total` followed by one `region,total` line per region; and `json`, a list
of `{{"region": ..., "total": ...}}` objects. Both keep the text output's order. Any other value must be rejected
with an error. Add tests for the CSV and JSON output under `tests/`, and document the option in `README.md`'s Usage
section. Run the tests with `{TEST_COMMAND}`.
""",
        seed=seed, check=check, budgets=(40, 900, 300_000), check_timeout=120, protected=["sales.json"],
        hidden={"test_hidden.py": hidden}, derived_from="py-feature-csv-export",
        key_facts=["--format text (default, unchanged), csv (header region,total) and json (list of objects)",
                   "Other values are rejected (argparse choices)", "Tests cover CSV and JSON",
                   "README.md's Usage documents --format"],
        solution={"report.py": solved, "tests/test_report.py": tests, "README.md": readme},
        wrong={"report.py": solved.replace('indent=2)', 'indent=2).replace("total", "sum")'),
               "tests/test_report.py": tests.replace('"total": 1}', '"sum": 1}'), "README.md": readme})


C5_HANDLERS = 40
C5_RATIO = 27
C5_SCORE = 13


def c5_handler(number, ratio_guarded=True, score_guarded=True):
    if ratio_guarded:
        ratio = "    if count == 0:\n        return 0.0\n    return round(hits / count, 4)\n"
    else:
        ratio = "    return round(hits / count, 4)\n"
    if number == C5_SCORE:
        guard = "    if not values:\n        return 0\n" if score_guarded else ""
        score = (f"{guard}    top = max(values)\n"
                 "    return sum(min(value, top) * WEIGHT for value in values)\n")
    else:
        score = "    return sum(value * WEIGHT for value in values)\n"
    return (f'"""Handler {number:02d}: request statistics for endpoint /api/e{number:02d}."""\n\n'
            f"WEIGHT = {number % 7 + 1}\n\n\n"
            "def ratio(hits, count):\n"
            '    """Return hits per request, rounded to four places; 0.0 when there were no requests."""\n'
            f"{ratio}\n\n"
            "def score(values):\n"
            '    """Return the weighted sum of the values; 0 for no values."""\n'
            f"{score}")


def c5():
    rng = random.Random(20261006)
    lines = []
    for index in range(4000):
        number = rng.randrange(C5_HANDLERS)
        stamp = f"2026-09-01T{index // 3600 % 24:02d}:{index // 60 % 60:02d}:{index % 60:02d}Z"
        head = f"{stamp} ERROR request={index:05d} handler=app.handlers.h{number:02d} endpoint=/api/e{number:02d}"
        if number == C5_RATIO and rng.random() < 0.25:
            lines += [f"{head} status=500", "Traceback (most recent call last):",
                      f'  File "app/handlers/h{C5_RATIO:02d}.py", line 8, in ratio',
                      "    return round(hits / count, 4)", "ZeroDivisionError: division by zero"]
        elif number == C5_SCORE and rng.random() < 0.25:
            lines += [f"{head} status=500", "Traceback (most recent call last):",
                      f'  File "app/handlers/h{C5_SCORE:02d}.py", line 15, in score',
                      "    top = max(values)", "ValueError: max() arg is an empty sequence"]
        else:
            lines.append(f"{stamp} INFO request={index:05d} handler=app.handlers.h{number:02d} "
                         f"endpoint=/api/e{number:02d} status=200 latency_ms={rng.randrange(3, 900)} "
                         f"bytes={rng.randrange(200, 90000)}")
    seed = {"app/__init__.py": "",
            "app/handlers/__init__.py": f"NAMES = [f\"h{{number:02d}}\" for number in range({C5_HANDLERS})]\n",
            "logs/service.log": "\n".join(lines) + "\n",
            "README.md": "# service\n\nForty request handlers under `app/handlers/`; logs under `logs/`.\n"}
    for number in range(C5_HANDLERS):
        seed[f"app/handlers/h{number:02d}.py"] = c5_handler(number, number != C5_RATIO, number != C5_SCORE)
    expected = {f"h{number:02d}": {"score": sum(value * (number % 7 + 1) for value in (1, 2, 3)),
                                   "ratio": round(9 / 4, 4)} for number in range(C5_HANDLERS)}
    hidden = '''import importlib
import json
import unittest
from pathlib import Path

EXPECTED = json.loads((Path(__file__).resolve().parent.parent / "expected.json").read_text())


class HeldOut(unittest.TestCase):
    def test_the_two_crashes_are_fixed(self):
        self.assertEqual(importlib.import_module("app.handlers.h27").ratio(5, 0), 0.0)
        self.assertEqual(importlib.import_module("app.handlers.h13").score([]), 0)

    def test_every_handler_behaves_as_before(self):
        for name, expected in EXPECTED.items():
            module = importlib.import_module(f"app.handlers.{name}")
            self.assertEqual(module.score([1, 2, 3]), expected["score"], name)
            self.assertEqual(module.ratio(9, 4), expected["ratio"], name)
            self.assertEqual(module.ratio(1, 0), 0.0, name)
            self.assertEqual(module.score([]), 0, name)
'''
    check = '''
from checklib import held_out, run

def check(workspace):
    held_out(workspace)
    return "both crashing handlers are fixed and every handler behaves as before"

run(check)
'''
    fixed = {f"app/handlers/h{n:02d}.py": c5_handler(n) for n in (C5_RATIO, C5_SCORE)}
    return Task(
        id="c5-log-triage-two-bugs", title="Trace two crashes in a long service log to their handlers and fix them",
        kind="code",
        statement="""
Some requests fail with status 500 in `logs/service.log`. Find the handlers under `app/handlers/` that cause the
failures and fix them, so that a ratio with no requests is `0.0` and a score of no values is `0`, as their docstrings
say. Do not change the log or the behaviour of any handler otherwise.
""",
        seed=seed, check=check, budgets=(60, 1500, 800_000), check_timeout=120, protected=["logs/service.log"],
        expected=expected, hidden={"test_hidden.py": hidden}, derived_from="py-long-log-triage",
        key_facts=["h27's ratio divides by a zero count (ZeroDivisionError)",
                   "h13's score calls max() on an empty list (ValueError)",
                   "Both fixed with a guard; no other handler changed; the log unchanged"],
        solution=fixed, wrong={f"app/handlers/h{C5_RATIO:02d}.py": c5_handler(C5_RATIO)})


C6_FILES = {
    "inventory/__init__.py": ('"""Stock records for a small shop."""\nfrom inventory.io.loader import load\n'
                              "from inventory.model import Record\nfrom inventory.reports.stock import low_stock\n\n"
                              "__all__ = [\"Record\", \"load\", \"low_stock\"]\n"),
    "inventory/model.py": ('"""The stock record."""\nfrom dataclasses import dataclass\n\n\n@dataclass\nclass Record:\n'
                           "    sku: str\n    name: str\n    quantity: int\n    reorder_at: int = 5\n\n"
                           "    def needs_reorder(self):\n        return self.quantity <= self.reorder_at\n"),
    "inventory/io/__init__.py": "",
    "inventory/io/loader.py": ('"""Records from CSV text."""\nimport csv\nimport io\n\nfrom inventory.model import '
                               "Record\n\n\ndef load(text):\n"
                               "    \"\"\"Each CSV row (sku,name,quantity,reorder_at) as a "
                               "Record.\"\"\"\n    rows = csv.DictReader(io.StringIO(text))\n"
                               "    return [Record(row[\"sku\"], row[\"name\"], int(row[\"quantity\"]), "
                               "int(row[\"reorder_at\"])) for row in rows]\n"),
    "inventory/io/writer.py": ('"""Records to CSV text."""\nfrom inventory.model import Record\n\n\n'
                               "def dump(records: list[Record]):\n    lines = [\"sku,name,quantity,reorder_at\"]\n"
                               "    lines += [f\"{r.sku},{r.name},{r.quantity},{r.reorder_at}\" for r in records]\n"
                               "    return \"\\n\".join(lines) + \"\\n\"\n"),
    "inventory/reports/__init__.py": "",
    "inventory/reports/stock.py": ('"""Stock reports."""\nfrom inventory.model import Record\n\n\n'
                                   "def low_stock(records: list[Record]) -> list[Record]:\n"
                                   "    \"\"\"The Records at or under their reorder level, by SKU.\"\"\"\n"
                                   "    return sorted((r for r in records if r.needs_reorder()), key=lambda r: r.sku)\n"
                                   "\n\ndef summary(records: list[Record]):\n"
                                   "    return f\"{len(records)} records, {len(low_stock(records))} to reorder\"\n"),
    "inventory/store/__init__.py": "",
    "inventory/store/warehouse.py": ('"""Records held in a warehouse."""\nfrom inventory.model import Record\n\n\n'
                                     "class Warehouse:\n    def __init__(self):\n        self._records = {}\n\n"
                                     "    def add(self, record: Record):\n"
                                     "        self._records[record.sku] = record\n\n"
                                     "    def find(self, sku) -> Record | None:\n"
                                     "        return self._records.get(sku)\n"),
    "docs/model.md": ("# The model\n\nA `Record` is one stock line: its SKU, name, quantity and reorder level. "
                      "`Record.needs_reorder()` says whether to order more.\n"),
    "docs/reports.md": "# Reports\n\n`low_stock(records)` lists every Record at or under its reorder level.\n",
    "tests/__init__.py": "",
    "tests/test_inventory.py": ("import unittest\n\nfrom inventory import Record, load, low_stock\n\n\n"
                                "class InventoryTest(unittest.TestCase):\n    def test_low_stock(self):\n"
                                "        records = load(\"sku,name,quantity,reorder_at\\nb,bolt,2,5\\na,axe,9,3\\n\")\n"
                                "        self.assertEqual(low_stock(records), [Record(\"b\", \"bolt\", 2, 5)])\n"),
    "README.md": f"# inventory\n\nRun the tests with `{TEST_COMMAND}`.\n",
}


def c6_renamed():
    out = {}
    for path, text in C6_FILES.items():
        if path == "inventory/__init__.py":
            text = text.replace("from inventory.model import Record", "from inventory.model import StockItem")
            text = text.replace('__all__ = ["Record",', "# Record stays as an alias of StockItem for existing users.\n"
                                                        'Record = StockItem\n\n__all__ = ["Record", "StockItem",')
        elif path.startswith("tests/"):
            continue
        else:
            text = re.sub(r"\bRecords\b", "StockItems", re.sub(r"\bRecord\b", "StockItem", text))
        out[path] = text
    return out


def c6():
    hidden = '''import unittest

import inventory
from inventory import Record, StockItem, load
from inventory.io.writer import dump
from inventory.reports.stock import summary
from inventory.store.warehouse import Warehouse


class HeldOut(unittest.TestCase):
    def test_the_old_name_is_an_alias(self):
        self.assertIs(Record, StockItem)
        self.assertIn("Record", inventory.__all__)

    def test_behaviour_is_unchanged(self):
        items = load("sku,name,quantity,reorder_at\\nb,bolt,2,5\\na,axe,9,3\\n")
        self.assertTrue(repr(items[0]).startswith("StockItem("))
        self.assertEqual(dump(items), "sku,name,quantity,reorder_at\\nb,bolt,2,5\\na,axe,9,3\\n")
        self.assertIn("1 to reorder", summary(items))
        store = Warehouse()
        store.add(items[1])
        self.assertIs(store.find("a"), items[1])
'''
    check = '''
import re
from checklib import fail, held_out, run, visible

OLD = re.compile(r"\\bRecords?\\b")
KEEP = ("alias", "deprecat", "renamed", "stockitem")

def check(workspace):
    left = []
    for path in sorted((workspace / "inventory").rglob("*.py")) + sorted((workspace / "docs").rglob("*.md")):
        for number, line in enumerate(path.read_text().splitlines(), start=1):
            if OLD.search(line) and not any(word in line.lower() for word in KEEP):
                left.append(f"{path.relative_to(workspace)}:{number}: {line.strip()}")
    if left:
        fail("Record is still used outside the alias", "\\n".join(left[:20]))
    visible(workspace)
    held_out(workspace)
    return "renamed to StockItem in the package and docs, the old name kept as an alias, tests pass"

run(check)
'''
    renamed = c6_renamed()
    wrong = {path: text for path, text in renamed.items() if path in ("inventory/__init__.py", "inventory/model.py")}
    wrong["inventory/io/loader.py"] = C6_FILES["inventory/io/loader.py"].replace("import Record", "import Record  ")
    return Task(
        id="c6-rename-type", title="Rename a type across a package and its docs without breaking its public API",
        kind="code",
        statement=f"""
Rename the type `Record` to `StockItem` everywhere in the package `inventory/` and its docs in `docs/`. The public
API must not break: `from inventory import Record` must keep working, as an alias of `StockItem`. Keep the behaviour
identical; the tests may move to the new name. Run the tests with `{TEST_COMMAND}`.
""",
        seed=dict(C6_FILES), check=check, budgets=(40, 900, 300_000), check_timeout=120,
        hidden={"test_hidden.py": hidden}, derived_from="py-rename-refactor (shape)",
        key_facts=["StockItem in inventory/model.py and every module: io, reports, store",
                   "inventory/__init__.py keeps Record = StockItem and exports both",
                   "docs/model.md and docs/reports.md use StockItem", "Behaviour and CSV output unchanged"],
        solution=renamed, wrong=wrong)


TASKS = (c1, c2, c3, c4, c5, c6)
