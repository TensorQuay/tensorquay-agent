"""The agent-teams evaluation's L3 task (long, docs/evals/agent-teams.md section 2)."""

from common import Task

TEST_COMMAND = "python3 -m unittest discover -s tests -t ."

# L3: a 300-file repository with 12 seeded defects, security and correctness, each with a held-out test that fails
# on the seed. The attempt reports each defect it finds with its location and fixes at least six, with tests.
L3_DEFECTS = [
    ("web/files.py", "serve", '''import os


def serve(root, name):
    """The bytes of the file `name` under `root`; a name outside root is refused with ValueError."""
    with open(os.path.join(root, name), "rb") as file:
        return file.read()
''', '''import os


def serve(root, name):
    """The bytes of the file `name` under `root`; a name outside root is refused with ValueError."""
    base = os.path.realpath(root)
    path = os.path.realpath(os.path.join(base, name))
    if os.path.commonpath([base, path]) != base:
        raise ValueError("outside the root")
    with open(path, "rb") as file:
        return file.read()
''', '''    def test_defect_01_path_traversal(self):
        import os, tempfile
        from web.files import serve
        root = tempfile.mkdtemp()
        os.makedirs(os.path.join(root, "pub"))
        for path, text in (("secret", "s"), (os.path.join("pub", "a.txt"), "a")):
            with open(os.path.join(root, path), "w") as file:
                file.write(text)
        self.assertEqual(serve(os.path.join(root, "pub"), "a.txt"), b"a")
        with self.assertRaises(ValueError):
            serve(os.path.join(root, "pub"), "../secret")
'''),
    ("db/queries.py", "find_user", '''def find_user(connection, name):
    """The users with this exact name."""
    return connection.execute(f"SELECT name FROM users WHERE name = '{name}'").fetchall()
''', '''def find_user(connection, name):
    """The users with this exact name."""
    return connection.execute("SELECT name FROM users WHERE name = ?", (name,)).fetchall()
''', '''    def test_defect_02_sql_injection(self):
        import sqlite3
        from db.queries import find_user
        connection = sqlite3.connect(":memory:")
        connection.execute("CREATE TABLE users (name TEXT)")
        connection.executemany("INSERT INTO users VALUES (?)", [("ana",), ("bo",)])
        self.assertEqual(find_user(connection, "ana"), [("ana",)])
        self.assertEqual(find_user(connection, "x' OR '1'='1"), [])
'''),
    ("util/calc.py", "evaluate", '''def evaluate(expression):
    """The value of an arithmetic expression of numbers, + - * / and brackets; anything else is a ValueError."""
    return eval(expression)
''', '''import ast
import operator

_OPS = {ast.Add: operator.add, ast.Sub: operator.sub, ast.Mult: operator.mul, ast.Div: operator.truediv,
        ast.USub: operator.neg, ast.UAdd: operator.pos}


def _value(node):
    if isinstance(node, ast.Constant) and isinstance(node.value, (int, float)) and not isinstance(node.value, bool):
        return node.value
    if isinstance(node, ast.BinOp) and type(node.op) in _OPS:
        return _OPS[type(node.op)](_value(node.left), _value(node.right))
    if isinstance(node, ast.UnaryOp) and type(node.op) in _OPS:
        return _OPS[type(node.op)](_value(node.operand))
    raise ValueError("not arithmetic")


def evaluate(expression):
    """The value of an arithmetic expression of numbers, + - * / and brackets; anything else is a ValueError."""
    try:
        tree = ast.parse(expression, mode="eval")
    except SyntaxError as error:
        raise ValueError("not arithmetic") from error
    return _value(tree.body)
''', '''    def test_defect_03_eval(self):
        from util.calc import evaluate
        self.assertEqual(evaluate("2 + 3 * (4 - 1)"), 11)
        with self.assertRaises(ValueError):
            evaluate("__import__('os').getcwd()")
'''),
    ("auth/tokens.py", "new_token", '''import random


def new_token():
    """A session token: 32 hex characters, unguessable."""
    return "%032x" % random.getrandbits(128)
''', '''import secrets


def new_token():
    """A session token: 32 hex characters, unguessable."""
    return secrets.token_hex(16)
''', '''    def test_defect_04_predictable_token(self):
        import pathlib
        from auth.tokens import new_token
        self.assertRegex(new_token(), r"^[0-9a-f]{32}$")
        source = pathlib.Path("auth/tokens.py").read_text()
        self.assertNotIn("import random", source)
'''),
    ("cache/store.py", "load", '''import pickle


def load(data):
    """The cached value from its bytes (a JSON document); anything else is a ValueError."""
    return pickle.loads(data)


def dump(value):
    return pickle.dumps(value)
''', '''import json


def load(data):
    """The cached value from its bytes (a JSON document); anything else is a ValueError."""
    try:
        return json.loads(data.decode("utf-8"))
    except (UnicodeDecodeError, json.JSONDecodeError) as error:
        raise ValueError("not a cached value") from error


def dump(value):
    return json.dumps(value).encode("utf-8")
''', '''    def test_defect_05_unsafe_deserialisation(self):
        import pickle
        from cache.store import dump, load
        self.assertEqual(load(dump({"a": [1, 2]})), {"a": [1, 2]})
        with self.assertRaises(ValueError):
            load(pickle.dumps({"a": 1}))
'''),
    ("api/admin.py", "delete_user", '''USERS = {"ana": {"admin": True}, "bo": {"admin": False}, "cy": {"admin": False}}


def delete_user(actor, target):
    """Deletes `target`; only an administrator may (PermissionError otherwise)."""
    return USERS.pop(target, None) is not None
''', '''USERS = {"ana": {"admin": True}, "bo": {"admin": False}, "cy": {"admin": False}}


def delete_user(actor, target):
    """Deletes `target`; only an administrator may (PermissionError otherwise)."""
    if not USERS.get(actor, {}).get("admin"):
        raise PermissionError(actor)
    return USERS.pop(target, None) is not None
''', '''    def test_defect_06_missing_authorisation(self):
        from api import admin
        with self.assertRaises(PermissionError):
            admin.delete_user("bo", "cy")
        self.assertIn("cy", admin.USERS)
        self.assertTrue(admin.delete_user("ana", "cy"))
'''),
    ("lists/paging.py", "page", '''def page(items, number, size):
    """Page `number` (1-based) of `items`, `size` per page."""
    items = list(items)
    start = (number - 1) * size
    return items[start:start + size - 1]
''', '''def page(items, number, size):
    """Page `number` (1-based) of `items`, `size` per page."""
    items = list(items)
    start = (number - 1) * size
    return items[start:start + size]
''', '''    def test_defect_07_pagination_drops_an_item(self):
        from lists.paging import page
        self.assertEqual(page(range(10), 2, 5), [5, 6, 7, 8, 9])
        self.assertEqual(page(range(3), 1, 5), [0, 1, 2])
'''),
    ("timeutil/expiry.py", "is_expired", '''def is_expired(expires_at, now):
    """Whether something that expires at `expires_at` has expired at `now` (both aware datetimes)."""
    return expires_at > now
''', '''def is_expired(expires_at, now):
    """Whether something that expires at `expires_at` has expired at `now` (both aware datetimes)."""
    return expires_at <= now
''', '''    def test_defect_08_expiry_reversed(self):
        from datetime import datetime, timedelta, timezone
        from timeutil.expiry import is_expired
        now = datetime(2026, 10, 7, tzinfo=timezone.utc)
        self.assertTrue(is_expired(now - timedelta(seconds=1), now))
        self.assertFalse(is_expired(now + timedelta(hours=1), now))
'''),
    ("billing/split.py", "shares", '''def shares(total_cents, people):
    """`total_cents` split between `people` as evenly as possible, summing to the total."""
    return [int(total_cents / people)] * people
''', '''def shares(total_cents, people):
    """`total_cents` split between `people` as evenly as possible, summing to the total."""
    base, extra = divmod(total_cents, people)
    return [base + (1 if index < extra else 0) for index in range(people)]
''', '''    def test_defect_09_split_loses_cents(self):
        from billing.split import shares
        self.assertEqual(sum(shares(100, 3)), 100)
        self.assertEqual(sorted(shares(100, 3)), [33, 33, 34])
'''),
    ("core/registry.py", "register", '''def register(name, plugins=[]):
    """A new registry list holding `name` after `plugins`."""
    plugins.append(name)
    return plugins
''', '''def register(name, plugins=None):
    """A new registry list holding `name` after `plugins`."""
    plugins = list(plugins or [])
    plugins.append(name)
    return plugins
''', '''    def test_defect_10_shared_default(self):
        from core.registry import register
        self.assertEqual(register("a"), ["a"])
        self.assertEqual(register("b"), ["b"])
'''),
    ("jobs/runner.py", "run", '''def run(job):
    """Runs `job` (a callable); True when it succeeded, False when it raised."""
    try:
        job()
    except Exception:
        pass
    return True
''', '''def run(job):
    """Runs `job` (a callable); True when it succeeded, False when it raised."""
    try:
        job()
    except Exception:
        return False
    return True
''', '''    def test_defect_11_failure_reported_as_success(self):
        from jobs.runner import run
        self.assertTrue(run(lambda: None))
        self.assertFalse(run(lambda: 1 / 0))
'''),
    ("text/escape.py", "html", '''def html(text):
    """`text` escaped for HTML: & < > and double quotes."""
    return text.replace("<", "&lt;").replace(">", "&gt;").replace("&", "&amp;")
''', '''def html(text):
    """`text` escaped for HTML: & < > and double quotes."""
    return text.replace("&", "&amp;").replace("<", "&lt;").replace(">", "&gt;").replace('"', "&quot;")
''', '''    def test_defect_12_broken_escaping(self):
        from text.escape import html
        self.assertEqual(html('<a href="x">&'), "&lt;a href=&quot;x&quot;&gt;&amp;")
'''),
]

L3_FILLER_PACKAGES = ["web", "db", "util", "auth", "cache", "api", "lists", "timeutil", "billing", "core", "jobs",
                      "text"]


def l3_filler(package, index):
    """A harmless module: a few small, correct functions, so the defects hide among ordinary code."""
    return f'''"""{package}: helpers, part {index}."""


def normalise_{index}(value):
    return str(value).strip().lower()


def clamp_{index}(value, low=0, high={10 + index}):
    return max(low, min(high, value))


def pairs_{index}(items):
    items = list(items)
    return list(zip(items, items[1:]))


def count_{index}(items, wanted):
    return sum(1 for item in items if item == wanted)
'''


def l3():
    seed = {"tests/__init__.py": "", "README.md": (f"# platform\n\nA platform's code, about 300 files. Tests: "
                                                   f"`{TEST_COMMAND}`.\n")}
    for package in L3_FILLER_PACKAGES:
        seed[f"{package}/__init__.py"] = ""
        for index in range(1, 23):
            seed[f"{package}/helpers_{index:02d}.py"] = l3_filler(package, index)
    for path, _, broken, _, _ in L3_DEFECTS:
        seed[path] = broken
    seed["tests/test_helpers.py"] = ("import unittest\n\nfrom web.helpers_01 import clamp_1, pairs_1\n\n\n"
                                     "class Helpers(unittest.TestCase):\n    def test_clamp(self):\n"
                                     "        self.assertEqual(clamp_1(50), 11)\n\n    def test_pairs(self):\n"
                                     "        self.assertEqual(pairs_1('abc'), [('a', 'b'), ('b', 'c')])\n")
    hidden = "import unittest\n\n\nclass Defects(unittest.TestCase):\n" + "\n".join(test for *_, test in L3_DEFECTS)
    fixed = (0, 1, 2, 5, 6, 8)
    audit = ["# Audit", "", "| # | Where | Defect | Fixed |", "|---|---|---|---|"]
    for number, (path, function, *_rest) in enumerate(L3_DEFECTS, 1):
        done = "yes" if number - 1 in fixed else "no"
        audit.append(f"| {number} | `{path}` `{function}` | see the held-out list | {done} |")
    solution = {"AUDIT.md": "\n".join(audit) + "\n"}
    for index in fixed:
        path, _, _, good, _ = L3_DEFECTS[index]
        solution[path] = good
    solution["tests/test_fixes.py"] = ("import unittest\n\n\nclass Fixes(unittest.TestCase):\n"
                                       + "\n".join(L3_DEFECTS[index][4] for index in fixed))
    # The six fixes and their tests, but the audit names only eight defects.
    wrong = dict(solution)
    wrong["AUDIT.md"] = "\n".join(audit[:4 + 8]) + "\n"
    defects = [path for path, *_ in L3_DEFECTS]
    check = '''
import re
import subprocess
import sys
from checklib import HERE, expected, fail, read, run, visible

def check(workspace):
    visible(workspace)
    audit = read(workspace, "AUDIT.md")
    found = [path for path in expected()["defects"] if path in audit]
    if len(found) < 10:
        fail(f"AUDIT.md locates {len(found)} of the 12 defects (10 needed)")
    added = [path for path in (workspace / "tests").glob("test_*.py") if path.name != "test_helpers.py"]
    if not added:
        fail("no new test for the fixes in tests/")
    hidden = str(HERE / "hidden")
    result = subprocess.run([sys.executable, "-W", "ignore", "-m", "unittest", "discover", "-v", "-s", hidden, "-t",
                             hidden], cwd=workspace, capture_output=True, text=True, timeout=180,
                            env={"PYTHONPATH": str(workspace), "PYTHONDONTWRITEBYTECODE": "1", "PATH": "/usr/bin:/bin"})
    passed = re.findall(r"test_defect_(\\d+)\\S* \\(.*\\) \\.\\.\\. ok", result.stdout + result.stderr)
    if len(passed) < 6:
        fail(f"{len(passed)} defects are fixed (6 needed)", result.stdout + result.stderr)
    return f"{len(found)} of 12 defects located and {len(passed)} fixed, the repository's tests passing"

run(check)
'''
    return Task(
        id="l3-repository-audit", title="Audit a 300-file repository: find 12 defects, fix six with tests",
        kind="long",
        statement=f"""
Audit the repository here (about 300 files) for defects, security and correctness alike: there are twelve. Report
each one you find in `AUDIT.md` with its file and function, what's wrong and what it allows, and fix at least six of
them, each with a test in `tests/` that fails before your fix and passes after it. The existing tests must still pass;
run them with `{TEST_COMMAND}`.
""",
        seed=seed, check=check, budgets=(250, 7200, 2_500_000), check_timeout=300,
        expected={"defects": defects}, hidden={"test_hidden.py": hidden},
        key_facts=["web/files.py serve: path traversal", "db/queries.py find_user: SQL injection",
                   "util/calc.py evaluate: eval on input", "auth/tokens.py new_token: predictable random",
                   "cache/store.py load: pickle on untrusted bytes", "api/admin.py delete_user: no admin check",
                   "lists/paging.py page: drops the last item", "timeutil/expiry.py is_expired: reversed",
                   "billing/split.py shares: loses cents", "core/registry.py register: shared mutable default",
                   "jobs/runner.py run: a failure reported as success",
                   "text/escape.py html: & escaped last, quotes not",
                   "At least six fixed, each with a test"],
        solution=solution, wrong=wrong)
