"""The mixed tasks M1-M3: find something out, then change the code because of it."""

import random

from common import Task

TEST_COMMAND = "python3 -m unittest discover -s tests -t ."

M1_PARSERS = {
    "parsers/csv_parser.py": ('"""CSV rows with the standard library\'s csv module."""\nimport csv\nimport io\n\n\n'
                              "def parse(text):\n    return list(csv.reader(io.StringIO(text)))\n"),
    "parsers/split_parser.py": ('"""CSV rows by splitting lines, each row checked by a JSON round trip."""\n'
                                "import json\nimport re\n\n\ndef parse(text):\n    rows = []\n"
                                "    for line in text.splitlines():\n        row = re.split(r\",\", line)\n"
                                "        rows.append(json.loads(json.dumps(row)))\n    return rows\n"),
    "parsers/char_parser.py": ('"""CSV rows one character at a time."""\n\n\ndef parse(text):\n    rows, row, field = '
                               "[], [], \"\"\n    for char in text:\n        if char == \",\":\n"
                               "            row = row + [field]\n            field = \"\"\n"
                               "        elif char == \"\\n\":\n            rows = rows + [row + [field]]\n"
                               "            row, field = [], \"\"\n        else:\n            field = field + char\n"
                               "    if field or row:\n        rows = rows + [row + [field]]\n    return rows\n"),
}
M1_TOOL = ('"""Units sold per category, from data/sales.csv."""\nfrom pathlib import Path\n\n'
           "from parsers.char_parser import parse\n\n\ndef units_by_category(text):\n    totals = {}\n"
           "    for category, _, units in parse(text)[1:]:\n        totals[category] = totals.get(category, 0) + "
           "int(units)\n    return dict(sorted(totals.items()))\n\n\ndef main():\n"
           "    text = (Path(__file__).resolve().parent.parent / \"data\" / \"sales.csv\").read_text()\n"
           "    for category, units in units_by_category(text).items():\n        print(f\"{category}: {units}\")\n\n\n"
           "if __name__ == \"__main__\":\n    main()\n")
M1_BENCH = '''"""Times the three parsers on data/sales.csv: python3 bench.py."""
import time
from pathlib import Path

from parsers import char_parser, csv_parser, split_parser

TEXT = (Path(__file__).resolve().parent / "data" / "sales.csv").read_text()
results = {}
for name, module in (("csv_parser", csv_parser), ("split_parser", split_parser), ("char_parser", char_parser)):
    started = time.perf_counter()
    module.parse(TEXT)
    results[name] = time.perf_counter() - started
for name, seconds in sorted(results.items(), key=lambda item: item[1]):
    print(f"{name}: {seconds * 1000:.1f} ms")
'''


def m1():
    rng = random.Random(301)
    lines = ["category,sku,units"]
    totals = {}
    for _ in range(5000):
        category = rng.choice(("garden", "kitchen", "tools", "toys", "office"))
        units = rng.randint(1, 40)
        lines.append(f"{category},SKU-{rng.randint(1000, 9999)},{units}")
        totals[category] = totals.get(category, 0) + units
    seed = dict(M1_PARSERS)
    seed.update({"parsers/__init__.py": "", "tool/__init__.py": "", "tool/report.py": M1_TOOL,
                 "data/sales.csv": "\n".join(lines) + "\n",
                 "README.md": ("# units report\n\n`python3 -m tool.report` prints units sold per category. The "
                               "three parsers in `parsers/` give the same rows on `data/sales.csv`.\n")})
    expected_output = "".join(f"{category}: {units}\n" for category, units in sorted(totals.items()))
    check = '''
import os
import re
import subprocess
import sys
from checklib import expected, fail, run

def check(workspace):
    tool = (workspace / "tool" / "report.py").read_text() if (workspace / "tool" / "report.py").is_file() else ""
    imports = set(re.findall(r"parsers(?:\\.|\\s+import\\s+)(\\w+_parser)", tool))
    if imports != {"csv_parser"}:
        fail(f"tool/report.py uses {sorted(imports) or 'no parser'}, not only the fastest, csv_parser")
    environment = dict(os.environ, PYTHONPATH=str(workspace), PYTHONDONTWRITEBYTECODE="1")
    result = subprocess.run([sys.executable, "-m", "tool.report"], cwd=workspace, capture_output=True, text=True,
                            timeout=60, env=environment)
    if result.returncode != 0 or result.stdout != expected()["output"]:
        fail("python3 -m tool.report no longer prints the same totals", result.stdout[-500:] + result.stderr[-500:])
    if not (workspace / "bench.py").is_file():
        fail("there is no bench.py")
    bench = subprocess.run([sys.executable, "bench.py"], cwd=workspace, capture_output=True, text=True, timeout=120,
                           env=environment)
    if bench.returncode != 0 or not all(name in bench.stdout for name in ("csv", "split", "char")):
        fail("bench.py does not run or does not report all three parsers", bench.stdout[-500:] + bench.stderr[-500:])
    return "the tool uses csv_parser, prints the same totals, and bench.py times all three"

run(check)
'''
    return Task(
        id="m1-fastest-parser", title="Benchmark three CSV parsers and switch a tool to the fastest", kind="mixed",
        statement="""
The folder `parsers/` holds three CSV parsers that give the same rows on `data/sales.csv`. Find which is fastest on
that file and switch `tool/report.py` to it, keeping its output identical. Add `bench.py` at the top of the
workspace: it times all three parsers on `data/sales.csv` and prints each one's time; it must run with
`python3 bench.py`.
""",
        seed=seed, check=check, budgets=(40, 900, 300_000), check_timeout=300,
        protected=sorted(M1_PARSERS) + ["data/sales.csv"], expected={"output": expected_output},
        key_facts=["csv_parser (the csv module) is fastest; char_parser is quadratic (rows = rows + [row])",
                   "split_parser is slowed by a JSON round trip per row",
                   "tool/report.py imports csv_parser and prints the same totals", "bench.py times all three"],
        solution={"tool/report.py": M1_TOOL.replace("char_parser", "csv_parser"), "bench.py": M1_BENCH},
        wrong={"tool/report.py": M1_TOOL.replace("char_parser", "split_parser"), "bench.py": M1_BENCH})


M2_CALC = '''"""Small statistics helpers."""


def mean(values):
    """The arithmetic mean; ValueError for no values."""
    if not values:
        raise ValueError("mean of no values")
    return sum(values) / len(values)


def median(values):
    """The middle value (the mean of the two middle values for an even count)."""
    count = len(values)
    middle = count // 2
    if count % 2:
        return values[middle]
    return (values[middle - 1] + values[middle]) / 2


def percent(part, whole):
    """`part` as a percentage of `whole`, rounded to two places."""
    return round(part * 100 // whole, 2)
'''
M2_ISSUES = {
    "issues/101.md": ("# 101: percent() loses the decimals\n\n`percent(1, 3)` returns `33` but the docstring promises "
                      "two places: I expected `33.33`.\n"),
    "issues/102.md": ("# 102: mean() crashes on an empty list\n\n`mean([])` raises `ValueError`. I expected `0`.\n"),
    "issues/103.md": ("# 103: median() is wrong for unsorted input\n\n`median([3, 1, 2])` returns `1`; the "
                      "middle value is `2`.\n"),
}
M2_TESTS = '''import unittest

from calc import stats


class Issues(unittest.TestCase):
    def test_101_percent_keeps_two_places(self):
        self.assertEqual(stats.percent(1, 3), 33.33)

    def test_103_median_of_unsorted_values(self):
        self.assertEqual(stats.median([3, 1, 2]), 2)
        self.assertEqual(stats.median([4, 1, 3, 2]), 2.5)

    @unittest.expectedFailure
    def test_102_mean_of_nothing_is_zero(self):
        # Not a bug: mean() documents a ValueError for no values.
        self.assertEqual(stats.mean([]), 0)
'''


def m2():
    seed = {"calc/__init__.py": "", "calc/stats.py": M2_CALC, **M2_ISSUES, "tests/__init__.py": "",
            "tests/test_stats.py": ("import unittest\n\nfrom calc import stats\n\n\n"
                                    "class StatsTest(unittest.TestCase):\n"
                                    "    def test_mean(self):\n        self.assertEqual(stats.mean([1, 2, 3]), 2)\n"),
            "README.md": f"# calc\n\nRun the tests with `{TEST_COMMAND}`.\n"}
    fixed = M2_CALC.replace("    count = len(values)\n", "    values = sorted(values)\n    count = len(values)\n")
    fixed = fixed.replace("round(part * 100 // whole, 2)", "round(part * 100 / whole, 2)")
    hidden = '''import unittest

from calc import stats


class HeldOut(unittest.TestCase):
    def test_percent(self):
        self.assertEqual(stats.percent(2, 3), 66.67)
        self.assertEqual(stats.percent(1, 4), 25.0)

    def test_median(self):
        self.assertEqual(stats.median([9, 7, 8, 1, 5]), 7)
        self.assertEqual(stats.median([10, 2]), 6)

    def test_mean_still_refuses_no_values(self):
        with self.assertRaises(ValueError):
            stats.mean([])
        self.assertEqual(stats.mean([2, 4]), 3)
'''
    check = '''
import re
from checklib import fail, held_out, run, visible

def check(workspace):
    tests = " ".join(path.read_text() for path in (workspace / "tests").rglob("*.py"))
    for name in ("percent", "median"):
        if name not in tests:
            fail(f"no test reproduces the {name} issue")
    marked = r"@unittest\\.expectedFailure\\s+def\\s+\\w+\\(self\\):(.*?)(?=\\n\\s*(?:@|def |class )|\\Z)"
    expected_failures = re.findall(marked, tests, re.S)
    if not any("mean" in body for body in expected_failures):
        fail("the mean issue has no test marked expected to fail")
    visible(workspace)
    held_out(workspace)
    return "percent and median fixed with tests; mean unchanged, its test marked expected to fail"

run(check)
'''
    wrong_calc = fixed.replace('        raise ValueError("mean of no values")', "        return 0")
    return Task(
        id="m2-bug-reports", title="Reproduce three bug reports with tests, then fix the two real ones",
        kind="mixed",
        statement=f"""
The folder `issues/` holds three bug reports against `calc/stats.py`. Reproduce each one with a failing test under
`tests/`, then fix the ones that are real bugs. A report that describes documented behaviour is not a bug: leave the
code as it is and mark its test with `@unittest.expectedFailure`. Run the tests with `{TEST_COMMAND}`.
""",
        seed=seed, check=check, budgets=(40, 900, 300_000), check_timeout=120, protected=sorted(M2_ISSUES),
        hidden={"test_hidden.py": hidden},
        key_facts=["101 is real: percent uses floor division (//); 1/3 gives 33.33 once fixed",
                   "103 is real: median does not sort its values",
                   "102 is not a bug: mean documents a ValueError for no values; its test is expectedFailure"],
        solution={"calc/stats.py": fixed, "tests/test_issues.py": M2_TESTS},
        wrong={"calc/stats.py": wrong_calc, "tests/test_issues.py": M2_TESTS.replace(
            "    @unittest.expectedFailure\n", "")})


M3_APP = '''"""A small items service: ROUTES maps (method, path pattern) to a handler.

A handler takes (params, body) and answers (status, body).
"""

ITEMS = {}


def health(params, body):
    return 200, {"status": "ok"}


def list_items(params, body):
    limit = int(params.get("limit", 50))
    return 200, {"items": [dict(item, id=key) for key, item in sorted(ITEMS.items())][:limit]}


def create_item(params, body):
    key = str(len(ITEMS) + 1)
    ITEMS[key] = {"name": body.get("name"), "price": body.get("price")}
    return 201, dict(ITEMS[key], id=key)


def get_item(params, body):
    item = ITEMS.get(params["id"])
    return (200, dict(item, id=params["id"])) if item else (404, {"error": "no such item"})


def update_item(params, body):
    if params["id"] not in ITEMS:
        return 404, {"error": "no such item"}
    ITEMS[params["id"]].update({key: body[key] for key in ("name", "price") if key in body})
    return 200, dict(ITEMS[params["id"]], id=params["id"])


def delete_item(params, body):
    return (204, None) if ITEMS.pop(params["id"], None) else (404, {"error": "no such item"})


ROUTES = {
    ("GET", "/health"): health,
    ("GET", "/items"): list_items,
    ("POST", "/items"): create_item,
    ("GET", "/items/<id>"): get_item,
    ("PUT", "/items/<id>"): update_item,
    ("DELETE", "/items/<id>"): delete_item,
}


def dispatch(method, path, body=None, query=None):
    """Routes a request; (status, body)."""
    parts = path.strip("/").split("/")
    for (route_method, pattern), handler in ROUTES.items():
        pattern_parts = pattern.strip("/").split("/")
        if route_method != method or len(pattern_parts) != len(parts):
            continue
        params = dict(query or {})
        if all(p == q or (p.startswith("<") and params.setdefault(p[1:-1], q)) for p, q in zip(pattern_parts, parts)):
            return handler(params, body or {})
    return 404, {"error": "no such route"}
'''
M3_VALIDATE = '''

def invalid(body, partial=False):
    """Why an item body breaks the README's rules, or None."""
    if not isinstance(body, dict):
        return "the body must be an object"
    if "name" in body or not partial:
        name = body.get("name")
        if not isinstance(name, str) or not 1 <= len(name) <= 80:
            return "name must be 1 to 80 characters"
    if "price" in body or not partial:
        price = body.get("price")
        if isinstance(price, bool) or not isinstance(price, (int, float)) or price < 0:
            return "price must be a number, zero or more"
    return None
'''


def m3():
    seed = {"svc/__init__.py": "", "svc/app.py": M3_APP, "tests/__init__.py": "",
            "tests/test_app.py": ("import unittest\n\nfrom svc.app import dispatch\n\n\n"
                                  "class AppTest(unittest.TestCase):\n    def test_health(self):\n"
                                  "        self.assertEqual(dispatch(\"GET\", \"/health\"),\n"
                                  "                         (200, {\"status\": \"ok\"}))\n"),
            "svc/README.md": ("# items service\n\nAn item has a `name` (a string of 1 to 80 characters) and a "
                              "`price` (a number, zero or more). `svc/app.py` holds the routes.\n"),
            "README.md": f"# items\n\nRun the tests with `{TEST_COMMAND}`.\n"}
    validated = M3_APP.replace('''def create_item(params, body):
''', '''def create_item(params, body):
    problem = invalid(body)
    if problem:
        return 400, {"error": problem}
''').replace('''    if params["id"] not in ITEMS:
        return 404, {"error": "no such item"}
''', '''    if params["id"] not in ITEMS:
        return 404, {"error": "no such item"}
    problem = invalid(body, partial=True)
    if problem:
        return 400, {"error": problem}
''').replace("\n\nROUTES = {", M3_VALIDATE + "\n\nROUTES = {")
    api = ["# Items API\n"]
    for method, path, text in (("GET", "/health", "200 `{\"status\": \"ok\"}`"),
                               ("GET", "/items", "query `limit` (default 50); 200 `{\"items\": [...]}`"),
                               ("POST", "/items", "body `{name, price}`; 201 the item; 400 when invalid"),
                               ("GET", "/items/<id>", "200 the item; 404"),
                               ("PUT", "/items/<id>", "body with `name` and/or `price`; 200; 400; 404"),
                               ("DELETE", "/items/<id>", "204; 404")):
        api.append(f"## {method} {path}\n\n{text}\n")
    hidden = '''import unittest

from svc import app


class HeldOut(unittest.TestCase):
    def setUp(self):
        app.ITEMS.clear()

    def test_invalid_items_are_refused(self):
        for body in ({}, {"name": "", "price": 1}, {"name": "x" * 81, "price": 1}, {"name": "pen", "price": -1},
                     {"name": "pen", "price": "1"}, {"name": 5, "price": 1}):
            self.assertEqual(app.dispatch("POST", "/items", body)[0], 400, body)
        self.assertEqual(app.ITEMS, {})

    def test_valid_items_still_work(self):
        status, item = app.dispatch("POST", "/items", {"name": "pen", "price": 1.5})
        self.assertEqual((status, item["name"]), (201, "pen"))
        self.assertEqual(app.dispatch("PUT", "/items/1", {"price": -2})[0], 400)
        self.assertEqual(app.dispatch("PUT", "/items/1", {"price": 2})[1]["price"], 2)
        self.assertEqual(app.dispatch("GET", "/items/1")[0], 200)
        self.assertEqual(app.dispatch("DELETE", "/items/1")[0], 204)
        self.assertEqual(app.dispatch("GET", "/items/9")[0], 404)
'''
    check = '''
import re
from checklib import fail, held_out, read, run, visible

ROUTES = (("GET", "/health"), ("GET", "/items"), ("POST", "/items"), ("GET", "/items/ID"), ("PUT", "/items/ID"),
          ("DELETE", "/items/ID"))

def check(workspace):
    text = read(workspace, "API.md")
    normal = re.sub(r"(?:<id>|\\{id\\}|:id|<item_id>|\\{item_id\\})", "ID", text.replace("`", ""))
    missing = [f"{method} {path}" for method, path in ROUTES
               if not re.search(rf"\\b{method}\\s+{re.escape(path)}(?![/\\w])", normal)]
    if missing:
        fail("API.md does not document every route", "missing: " + ", ".join(missing))
    visible(workspace)
    held_out(workspace)
    return "every route documented; items are validated as the README says"

run(check)
'''
    return Task(
        id="m3-document-and-validate", title="Document a service's API and add the validation it lacks",
        kind="mixed",
        statement=f"""
Document the service in `svc/` as `API.md`: every route, written as its method and path (for example
`GET /items/<id>`), with its request and its responses, from the code. Writing it down shows input validation the
code is missing against the rules in `svc/README.md`: add it, so an item that breaks them is refused with status
400 when created or updated. Run the tests with `{TEST_COMMAND}`.
""",
        seed=seed, check=check, budgets=(50, 1200, 400_000), check_timeout=120, protected=["svc/README.md"],
        hidden={"test_hidden.py": hidden},
        key_facts=["Six routes: GET /health, GET /items (limit), POST /items, GET, PUT and DELETE /items/<id>",
                   "POST and PUT accepted any body: name must be 1-80 characters, price a number >= 0",
                   "Invalid bodies get 400 with an error; PUT validates only the fields it sends"],
        solution={"svc/app.py": validated, "API.md": "\n".join(api)},
        wrong={"svc/app.py": validated, "API.md": "\n".join(api[:4])})


TASKS = (m1, m2, m3)
