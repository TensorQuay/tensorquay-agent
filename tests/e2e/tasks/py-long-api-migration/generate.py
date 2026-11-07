"""Regenerates this task's seed/, check/expected.json and dry-run/ deterministically: python3 generate.py.

The task is long on purpose: thirty-six modules of about sixty lines each call a deprecated client in several
forms (keyword, positional, multi-line and through its constant), so a migration means reading most of them. That is
sized to need compaction on a 32k window.
"""
import json
import pathlib
import random
import shutil

HERE = pathlib.Path(__file__).resolve().parent
MODULES = 36
FORMS = ("keyword", "positional", "multiline", "constant")

OLD_API = '''"""Deprecated HTTP client; use lib.new_api instead (docs/MIGRATION.md)."""
import warnings

DEFAULT_RETRIES = 3


def fetch(url, retries=DEFAULT_RETRIES):
    """Return the response for `url`, trying at most `retries` times."""
    warnings.warn("lib.old_api is deprecated; use lib.new_api.get", DeprecationWarning, stacklevel=2)
    return {"url": url, "tries": retries}
'''
NEW_API = '''"""The HTTP client that replaces lib.old_api."""

DEFAULT_ATTEMPTS = 3


def get(url, attempts=DEFAULT_ATTEMPTS):
    """Return the response for `url`, trying at most `attempts` times."""
    return {"url": url, "tries": attempts}
'''
MIGRATION = """# Migrating from lib.old_api to lib.new_api

`lib.old_api` is deprecated and will be removed. Every caller moves to `lib.new_api`:

| Before | After |
|---|---|
| `from lib import old_api` | `from lib import new_api` |
| `old_api.fetch(url, retries=n)` | `new_api.get(url, attempts=n)` |
| `old_api.fetch(url, n)` | `new_api.get(url, n)` |
| `old_api.DEFAULT_RETRIES` | `new_api.DEFAULT_ATTEMPTS` |

The responses are identical, so behaviour must not change.
"""


def call(form, url, tries):
    if form == "keyword":
        return f'old_api.fetch("{url}", retries={tries})'
    if form == "positional":
        return f'old_api.fetch("{url}", {tries})'
    if form == "multiline":
        return f'old_api.fetch(\n        "{url}",\n        retries={tries},\n    )'
    return f'old_api.fetch("{url}", retries=old_api.DEFAULT_RETRIES)'


def module(number, rng):
    form = FORMS[number % len(FORMS)]
    tries = 3 if form == "constant" else rng.randrange(1, 6)
    url = f"https://svc.example/e{number:02d}/items"
    fields = rng.sample(["id", "name", "price", "stock", "owner", "region", "status", "updated"], 4)
    lines = [f'"""Endpoint e{number:02d}: reads /e{number:02d}/items and summarises it for the dashboard."""',
             "from lib import old_api", "",
             f"FIELDS = {fields!r}", f"SCALE = {rng.randrange(2, 9)}", "", ""]
    for helper in range(6):
        verb = rng.choice(["filter", "normalise", "rank", "bucket", "clip", "weight"])
        lines += [f"def helper_{helper}(values):",
                  f'    """Step {helper + 1} of the summary: {verb} the values.',
                  "",
                  f"    The dashboard for /e{number:02d} shows {', '.join(fields[:2])}; this step keeps the order of",
                  "    the input and drops missing values, so a partial response still yields a line.",
                  '    """',
                  "    kept = [value for value in values if value is not None]",
                  f"    return [value * SCALE + {helper} for value in kept]", "", ""]
    lines += ["def describe():",
              '    """Return the fields this endpoint reports, joined for the dashboard header."""',
              '    return ", ".join(FIELDS)', "", "",
              "def run():",
              '    """Fetch the endpoint and return its dashboard line."""',
              f"    response = {call(form, url, tries)}",
              "    total = sum(helper_2(helper_1(helper_0([1, 2, None, 3]))))",
              "    return f\"{response['url']}#{response['tries']}:{total}:{describe()}\"", ""]
    scale = int(lines[4].split("= ")[1])
    values = [(((v * scale + 0) * scale + 1) * scale + 2) for v in (1, 2, 3)]
    expected = f"{url}#{tries}:{sum(values)}:{', '.join(fields)}"
    return "\n".join(lines), expected


def main():
    rng = random.Random(2026092602)
    seed = HERE / "seed"
    shutil.rmtree(seed, ignore_errors=True)
    for directory in ("lib", "app", "docs", "tests"):
        (seed / directory).mkdir(parents=True)
    (seed / "lib" / "__init__.py").write_text("")
    (seed / "lib" / "old_api.py").write_text(OLD_API)
    (seed / "lib" / "new_api.py").write_text(NEW_API)
    (seed / "app" / "__init__.py").write_text("")
    (seed / "tests" / "__init__.py").write_text("")
    (seed / "docs" / "MIGRATION.md").write_text(MIGRATION)
    (seed / "README.md").write_text("# dashboard\n\nThirty-six endpoint summaries under `app/`; the HTTP client is in `lib/`.\n")
    expected = {}
    for number in range(MODULES):
        text, expected[f"m{number:02d}"] = module(number, rng)
        (seed / "app" / f"m{number:02d}.py").write_text(text)
    (seed / "tests" / "test_app.py").write_text(
        "import unittest\n\nfrom app import m00, m01\n\n\nclass SmokeTest(unittest.TestCase):\n"
        "    def test_lines_name_their_endpoint(self):\n"
        "        self.assertTrue(m00.run().startswith(\"https://svc.example/e00/\"))\n"
        "        self.assertTrue(m01.run().startswith(\"https://svc.example/e01/\"))\n")
    (HERE / "check" / "expected.json").write_text(json.dumps(expected, indent=1, sort_keys=True) + "\n")
    solve = ("sed -i -e 's/from lib import old_api/from lib import new_api/' -e 's/old_api\\.fetch(/new_api.get(/g' "
             "-e 's/old_api\\.DEFAULT_RETRIES/new_api.DEFAULT_ATTEMPTS/g' -e 's/retries=/attempts=/g' app/*.py")
    wrong = "sed -i -e 's/from lib import old_api/from lib import new_api/' -e 's/old_api\\.fetch(/new_api.get(/g' app/*.py"
    for name, command, final in (("solve", solve, "Migrated every caller in app/ to lib.new_api."),
                                 ("wrong", wrong, "Switched the callers to new_api.get.")):
        scenario = {"model": "fixture", "responses": [
            {"events": [{"type": "tool_call", "name": "grep", "arguments": {"pattern": "old_api", "path": "app", "output_mode": "count"}, "split_into": 1}],
             "usage": {"prompt_tokens": 700, "completion_tokens": 40}},
            {"events": [{"type": "tool_call", "name": "shell", "arguments": {"command": command}, "split_into": 2}],
             "usage": {"prompt_tokens": 1800, "completion_tokens": 120}},
            {"events": [{"type": "text", "text": final}], "usage": {"prompt_tokens": 2100, "completion_tokens": 20}}]}
        (HERE / "dry-run" / f"{name}.json").write_text(json.dumps(scenario, indent=1) + "\n")


if __name__ == "__main__":
    main()
