"""Helpers for the agent-teams evaluation's checks: a verdict, the answer, tolerant matching."""
import json
import os
import re
import subprocess
import sys
import traceback
from pathlib import Path

HERE = Path(__file__).resolve().parent


class Verdict(Exception):
    def __init__(self, passed, reason, detail=""):
        super().__init__(reason)
        self.passed, self.reason, self.detail = passed, reason, detail


def fail(reason, detail=""):
    raise Verdict(False, reason, detail)


def run(check):
    """Runs `check(workspace)`, which returns a passing reason or raises Verdict; any other error is the check's."""
    workspace = Path(sys.argv[1] if len(sys.argv) > 1 else "/workspace")
    try:
        reason = check(workspace)
    except Verdict as verdict:
        print(json.dumps({"passed": verdict.passed, "reason": verdict.reason}))
        if verdict.detail:
            print(verdict.detail[-4000:])
        sys.exit(0 if verdict.passed else 1)
    except Exception:
        print(json.dumps({"passed": False, "reason": "the check itself failed"}))
        traceback.print_exc(limit=3)
        sys.exit(2)
    print(json.dumps({"passed": True, "reason": reason}))
    sys.exit(0)


def read(workspace, name):
    path = workspace / name
    if not path.is_file():
        fail(f"{name} is missing")
    try:
        return path.read_text(encoding="utf-8")
    except UnicodeDecodeError:
        fail(f"{name} is not UTF-8 text")


def norm(text):
    """Lower case, markdown emphasis and code marks dropped, thousands separators removed, spaces collapsed."""
    text = re.sub(r"[*_`]", "", text.lower())
    text = re.sub(r"(?<=\d),(?=\d{3}\b)", "", text)
    return re.sub(r"\s+", " ", text).strip()


def has(text, *alternatives):
    """Whether the normalised text holds any of the alternatives (each itself normalised)."""
    text = norm(text)
    return any(norm(alternative) in text for alternative in alternatives)


def has_all(text, groups):
    """Whether every group (a tuple of alternatives) is present."""
    return all(has(text, *group) for group in groups)


def sections(text, level=2):
    """[(heading, body)] for the markdown headings of one level, in order."""
    marks = "#" * level
    found = []
    for part in re.split(rf"(?m)^{marks}\s+", text)[1:]:
        heading, _, body = part.partition("\n")
        found.append((heading.strip(), body))
    return found


def table(text):
    """The rows of the first markdown table (header first) as lists of cell strings."""
    rows = []
    for line in text.splitlines():
        line = line.strip()
        if line.startswith("|") and line.endswith("|"):
            cells = [cell.strip() for cell in line.strip("|").split("|")]
            if all(re.fullmatch(r":?-{3,}:?", cell) for cell in cells if cell):
                continue
            rows.append(cells)
        elif rows:
            break
    return rows


def numbers(text):
    """The numbers in a text, thousands separators and currency signs ignored."""
    return [float(value) for value in re.findall(r"-?\d+(?:\.\d+)?", re.sub(r"(?<=\d),(?=\d{3})", "", text))]


def tests(workspace, directory, top=None, timeout=120):
    """Runs a unittest discovery from the workspace (the held-out tests find it as their working directory);
    (passed, output). A discovery that finds no test does not pass."""
    environment = dict(os.environ, PYTHONPATH=str(workspace), PYTHONDONTWRITEBYTECODE="1")
    result = subprocess.run([sys.executable, "-m", "unittest", "discover", "-s", str(directory), "-t",
                             str(top or directory)], cwd=workspace, capture_output=True, text=True, timeout=timeout,
                            env=environment)
    output = result.stdout + result.stderr
    return result.returncode == 0 and "Ran 0 tests" not in output, output


def held_out(workspace):
    """Runs check/hidden/ against the workspace; fails the attempt naming the failure."""
    passed, output = tests(workspace, HERE / "hidden")
    if not passed:
        fail("a held-out test fails", output)


def visible(workspace, directory="tests"):
    """Runs the workspace's own tests as `python3 -m unittest discover -s tests -t .` does."""
    passed, output = tests(workspace, workspace / directory, workspace)
    if not passed:
        fail("the workspace's own tests fail", output)


def expected():
    return json.loads((HERE / "expected.json").read_text())
