"""The agent-teams evaluation's D4 task (dependent, docs/evals/agent-teams.md section 2)."""

import json
import random

from common import Task

# D4: a regression somewhere in a 60-commit history. Finding the commit, writing a test that fails on it, and fixing
# it are separable, but the fix depends on the find and the test on the reproduction. The history is made by the
# workspace's make_repo.py with fixed dates, so every commit's hash is the same wherever it runs.
D4_CULPRIT_INDEX = 41  # the commit (0-based) that brings the regression

D4_BASE = {
    "textkit/__init__.py": "",
    "textkit/wrap.py": """def wrap(text, width):
    \"\"\"Lines of at most `width` characters, broken at spaces; a word longer than width stands alone.\"\"\"
    lines, line = [], ""
    for word in text.split():
        if line and len(line) + 1 + len(word) > width:
            lines.append(line)
            line = word
        else:
            line = f"{line} {word}" if line else word
    if line:
        lines.append(line)
    return lines
""",
    "textkit/slug.py": """import re


def slug(text):
    return re.sub(r"[^a-z0-9]+", "-", text.lower()).strip("-")
""",
    "tests/__init__.py": "",
    "tests/test_wrap.py": """import unittest
from textkit.wrap import wrap


class Wrap(unittest.TestCase):
    def test_short(self):
        self.assertEqual(wrap("a b c", 10), ["a b c"])

    def test_breaks(self):
        self.assertEqual(wrap("aaa bbb ccc", 7), ["aaa bbb", "ccc"])
""",
    "README.md": "# textkit\n\nSmall text helpers. Tests: `python3 -m unittest discover -s tests -t .`\n",
}


def d4_commits():
    """[(message, {path: content})], each commit's files changed over the one before (cumulative trees)."""
    rng = random.Random(64)
    tree = dict(D4_BASE)
    commits = [("textkit: wrap and slug", dict(tree))]
    helpers = []
    for index in range(1, 60):
        if index == D4_CULPRIT_INDEX:
            # The regression: an "optimisation" that counts the joining space twice, so lines break one early.
            tree["textkit/wrap.py"] = tree["textkit/wrap.py"].replace(
                "if line and len(line) + 1 + len(word) > width:", "if line and len(line) + 2 + len(word) > width:")
            tree["textkit/wrap.py"] = tree["textkit/wrap.py"].replace(
                "    lines, line = [], \"\"",
                "    # A faster check with one fewer addition.\n    lines, line = [], \"\"")
            commits.append(("wrap: cheaper length check", dict(tree)))
            continue
        name = f"h{index:02d}"
        helpers.append(name)
        tree[f"textkit/{name}.py"] = (f"def {name}(text):\n    \"\"\"Helper {index}.\"\"\"\n"
                                      f"    return text{rng.choice(['.upper()', '.lower()', '.strip()', '[::-1]'])}\n")
        if index % 7 == 0:
            tree["README.md"] = tree["README.md"] + f"\n- {name}: helper {index}\n"
        commits.append((f"{name}: add helper {index}", dict(tree)))
    return commits


D4_MAKE_REPO = """\"\"\"Creates project/ with textkit's history (60 commits, fixed dates): python3 make_repo.py\"\"\"
import json
import os
import subprocess
from pathlib import Path

HERE = Path(__file__).resolve().parent


def make(target):
    target = Path(target)
    commits = json.loads((HERE / "history.json").read_text(encoding="utf-8"))
    env = dict(os.environ, GIT_AUTHOR_NAME="textkit", GIT_AUTHOR_EMAIL="dev@example.invalid",
               GIT_COMMITTER_NAME="textkit", GIT_COMMITTER_EMAIL="dev@example.invalid")
    subprocess.run(["git", "init", "-q", "-b", "main", str(target)], check=True)
    previous = set()
    for index, (message, tree) in enumerate(commits):
        for path in previous - set(tree):
            (target / path).unlink()
        for path, content in tree.items():
            (target / path).parent.mkdir(parents=True, exist_ok=True)
            (target / path).write_text(content, encoding="utf-8")
        previous = set(tree)
        date = f"2026-0{1 + index // 20}-{1 + index % 20:02d}T10:00:00+00:00"
        subprocess.run(["git", "-C", str(target), "add", "-A"], check=True)
        subprocess.run(["git", "-C", str(target), "commit", "-q", "-m", message],
                       check=True, env=dict(env, GIT_AUTHOR_DATE=date, GIT_COMMITTER_DATE=date))


if __name__ == "__main__":
    make(HERE / "project")
"""

D4_ISSUE = """# Bug: wrap() breaks lines one character early

Since a recent change, `wrap("aaaa bbbb", 9)` gives `["aaaa", "bbbb"]`, but "aaaa bbbb" is exactly 9 characters and
should stay on one line. It worked in the first releases.
"""


def d4_hash(commits, index):
    """The commit hash make_repo.py gives the commit at `index` (made here the same way, so it's known)."""
    import subprocess
    import tempfile
    from pathlib import Path
    with tempfile.TemporaryDirectory() as scratch:
        Path(scratch, "history.json").write_text(json.dumps(commits, ensure_ascii=False), encoding="utf-8")
        Path(scratch, "make_repo.py").write_text(D4_MAKE_REPO, encoding="utf-8")
        subprocess.run(["python3", "make_repo.py"], cwd=scratch, check=True, capture_output=True)
        return subprocess.run(["git", "-C", str(Path(scratch, "project")), "rev-list", "--reverse", "HEAD"],
                              check=True, capture_output=True, text=True).stdout.split()[index]


D4_TEST = """import unittest
from textkit.wrap import wrap


class Regression(unittest.TestCase):
    def test_a_line_exactly_width_long_stays_whole(self):
        self.assertEqual(wrap("aaaa bbbb", 9), ["aaaa bbbb"])
        self.assertEqual(wrap("aa bb cc dd", 5), ["aa bb", "cc dd"])
"""

D4_CHECK = """
import ast
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path
from checklib import HERE, expected, fail, read, run, tests

def made(scratch):
    \"\"\"The check's own copy of the project with its history, as make_repo.py makes it.\"\"\"
    source = Path(scratch) / "made"
    if not source.exists():
        shutil.copy(HERE / "history.json", Path(scratch) / "history.json")
        shutil.copy(HERE / "make_repo.py", Path(scratch) / "make_repo.py")
        subprocess.run([sys.executable, "make_repo.py"], cwd=scratch, check=True, capture_output=True)
        (Path(scratch) / "project").rename(source)
    return source

def tree_at(commit, scratch):
    \"\"\"The project as it was at `commit`.\"\"\"
    source = made(scratch)
    target = Path(scratch) / commit[:12]
    subprocess.run(["git", "-C", str(source), "worktree", "add", "-q", "--detach", str(target), commit], check=True,
                   capture_output=True)
    return target

def test_ids(project):
    \"\"\"The test methods in tests/test_*.py, as unittest names them: tests.<module>.<class>.<method>.\"\"\"
    ids = set()
    for path in sorted((project / "tests").glob("test_*.py")):
        try:
            tree = ast.parse(path.read_text(encoding="utf-8"))
        except (SyntaxError, UnicodeDecodeError) as error:
            fail(f"project/tests/{path.name} can't be read: {error}")
        for node in tree.body:
            if isinstance(node, ast.ClassDef):
                ids |= {f"tests.{path.stem}.{node.name}.{item.name}" for item in node.body
                        if isinstance(item, (ast.FunctionDef, ast.AsyncFunctionDef)) and item.name.startswith("test")}
    return ids

def only(tree, ids):
    \"\"\"Runs just these tests in `tree`; (passed, output).\"\"\"
    environment = dict(os.environ, PYTHONPATH=str(tree), PYTHONDONTWRITEBYTECODE="1")
    result = subprocess.run([sys.executable, "-m", "unittest", *sorted(ids)], cwd=tree, capture_output=True, text=True,
                            timeout=120, env=environment)
    return result.returncode == 0, result.stdout + result.stderr

def check(workspace):
    project = workspace / "project"
    if not (project / "textkit" / "wrap.py").is_file():
        fail("project/ (made by make_repo.py) is missing")
    want = expected()
    found = read(workspace, "BISECT.md")
    named = set(re.findall(r"\\b[0-9a-f]{7,40}\\b", found))
    if not any(want["culprit"].startswith(name) for name in named):
        fail("BISECT.md doesn't name the commit that brought the regression")
    with tempfile.TemporaryDirectory() as scratch:
        # A new test is a test method the project's own tests didn't have, in a new file or an existing one (night 1,
        # 11 Oct: both sides added theirs to test_wrap.py). Only the new tests run on the two commits: the old ones
        # fail on the culprit too, so the whole suite would "catch" it with any new test.
        new = test_ids(project) - test_ids(made(scratch))
        if not new:
            fail("no new test in project/tests/")
        passed, output = tests(project, project / "tests", project)
        if not passed:
            fail("project's tests fail after the fix", output)
        files = {name.split(".")[1] for name in new}
        for commit, should_pass in ((want["culprit"], False), (want["parent"], True)):
            tree = tree_at(commit, scratch)
            for stem in files:
                shutil.copy(project / "tests" / f"{stem}.py", tree / "tests" / f"{stem}.py")
            passed, output = only(tree, new)
            if passed != should_pass:
                fail(("the new test passes on the culprit, so it doesn't catch the regression" if should_pass is False
                      else "the new test fails on the culprit's parent, before the regression"), output)
    wrap = read(workspace, "project/textkit/wrap.py")
    if "+ 2 +" in wrap:
        fail("the fix keeps the doubled space")
    return "the culprit is named, the new test fails on it and passes before it, and the fix makes it pass"

run(check)
"""


def d4():
    commits = d4_commits()
    culprit, parent = d4_hash(commits, D4_CULPRIT_INDEX), d4_hash(commits, D4_CULPRIT_INDEX - 1)
    history = json.dumps(commits, ensure_ascii=False) + "\n"
    seed = {"make_repo.py": D4_MAKE_REPO, "history.json": history, "ISSUE.md": D4_ISSUE,
            "README.md": ("# textkit's history\n\nRun `python3 make_repo.py` to create `project/`, a Git repository "
                          "with "
                          "textkit's 60 commits. The bug is described in `ISSUE.md`.\n")}
    final_tree = commits[-1][1]
    fixed = dict(final_tree)
    fixed["textkit/wrap.py"] = final_tree["textkit/wrap.py"].replace("len(line) + 2 + len(word)",
                                                                     "len(line) + 1 + len(word)")
    solution = {f"project/{path}": content for path, content in fixed.items()}
    solution["project/tests/test_regression.py"] = D4_TEST
    solution["BISECT.md"] = (f"# The regression\n\nCommit {culprit[:12]} (\"wrap: cheaper length check\") counts the "
                             "joining space twice, so a line exactly `width` long breaks one word early. Found with "
                             "`git bisect` and the test in `project/tests/test_regression.py`, which fails on it and "
                             "passes on its parent.\n")
    # The right fix and test, but the wrong commit named (its neighbour).
    wrong = dict(solution)
    wrong["BISECT.md"] = solution["BISECT.md"].replace(culprit[:12], parent[:12])
    return Task(
        id="d4-bisect-test-fix", title="Find the commit behind a regression, test it, fix it", kind="dependent",
        statement="""
`ISSUE.md` describes a regression in textkit. Run `python3 make_repo.py` to create `project/`, a Git repository with
its history. Find the commit that brought the regression and name it (its hash) and what it changed in `BISECT.md`;
add a test to `project/tests/` that fails on that commit and passes on the one before it; and fix the regression in
`project/` so that all its tests pass. Run the tests from `project/` with `python3 -m unittest discover -s tests -t .`.
""",
        seed=seed, check=D4_CHECK, budgets=(80, 2100, 1_000_000), check_timeout=180,
        expected={"culprit": culprit, "parent": parent},
        check_files={"history.json": history, "make_repo.py": D4_MAKE_REPO},
        key_facts=[f"The culprit is commit {culprit[:12]}, \"wrap: cheaper length check\" (the 42nd of 60)",
                   "It counts the joining space twice (len(line) + 2 + len(word)), so lines break one early",
                   "The new test fails on the culprit and passes on its parent",
                   "The fix restores + 1; all of project's tests pass"],
        solution=solution, wrong=wrong, dry_run_setup=["python3 make_repo.py"],
        dry_run_files={path: solution[path] for path in ("project/textkit/wrap.py", "project/tests/test_regression.py",
                                                         "BISECT.md")})
