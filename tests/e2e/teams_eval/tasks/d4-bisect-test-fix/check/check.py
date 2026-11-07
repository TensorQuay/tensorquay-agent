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
    """The check's own copy of the project with its history, as make_repo.py makes it."""
    source = Path(scratch) / "made"
    if not source.exists():
        shutil.copy(HERE / "history.json", Path(scratch) / "history.json")
        shutil.copy(HERE / "make_repo.py", Path(scratch) / "make_repo.py")
        subprocess.run([sys.executable, "make_repo.py"], cwd=scratch, check=True, capture_output=True)
        (Path(scratch) / "project").rename(source)
    return source

def tree_at(commit, scratch):
    """The project as it was at `commit`."""
    source = made(scratch)
    target = Path(scratch) / commit[:12]
    subprocess.run(["git", "-C", str(source), "worktree", "add", "-q", "--detach", str(target), commit], check=True,
                   capture_output=True)
    return target

def test_ids(project):
    """The test methods in tests/test_*.py, as unittest names them: tests.<module>.<class>.<method>."""
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
    """Runs just these tests in `tree`; (passed, output)."""
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
    named = set(re.findall(r"\b[0-9a-f]{7,40}\b", found))
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
