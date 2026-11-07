"""What every agent-teams evaluation task shares: its definition, how it is written as a tqa-e2e task directory, and
the check library copied into each task's check/.

A task is written from its ground truth: the seed, the check (and its expected values), the key facts for the judges,
and two answers the offline test plays against the check (`solution`, which must pass on the seed, and `wrong`,
which must fail), so a check is never trusted untested. Everything is deterministic: a fixed random seed per task.
"""

import json
import shutil
from dataclasses import dataclass, field
from pathlib import Path

import dryrun

RUN_SH = """# Exit 0: pass; 1: fail; anything else: the check itself is broken. One JSON verdict line on stdout.
set -u
command -v python3 >/dev/null || exit 2
cd /workspace || exit 2
export PYTHONDONTWRITEBYTECODE=1 PYTHONPATH=/workspace
python3 /check/check.py /workspace
"""

# Copied into every check/ as checklib.py. The check's own errors exit 2 (broken), never 1 (the model's).
CHECKLIB = r'''"""Helpers for the agent-teams evaluation's checks: a verdict, the answer, tolerant matching."""
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
'''


@dataclass
class Task:
    id: str  # lowercase, as tqa-e2e requires; the design's number first (r1-, p3-, c5-, m2-)
    title: str
    kind: str  # research, report, code or mixed
    statement: str  # task.md
    seed: dict  # {path: str or bytes}
    check: str  # check.py's body, using checklib
    key_facts: list
    solution: dict  # files written over the seed that must pass
    wrong: dict  # files written over the seed that must fail
    budgets: tuple = (80, 1800, 1_000_000)  # max_steps, wall_clock_secs, max_tokens
    protected: list = field(default_factory=list)
    expected: dict = None  # check/expected.json
    hidden: dict = field(default_factory=dict)  # check/hidden/<name>: held-out tests
    check_files: dict = field(default_factory=dict)  # check/<name>: anything else the check needs
    check_timeout: int = 180
    template: str = None
    derived_from: str = None  # the repository task it extends, for the record
    # The scripted sessions (dry-run/): shell commands the lead runs first, and the files it then writes (default: the
    # solution). D4's lead runs make_repo.py and writes three files rather than rewriting 60 near-identical ones, which
    # the runtime's no-progress guard stops, as a real agent would never do.
    dry_run_setup: list = field(default_factory=list)
    dry_run_files: dict = None


def text_or_bytes(path, content):
    path.parent.mkdir(parents=True, exist_ok=True)
    if isinstance(content, bytes):
        path.write_bytes(content)
    else:
        path.write_text(content, encoding="utf-8")


def write(task, root):
    """Writes the task as a tqa-e2e task directory under `root` (replacing it), plus key_facts.json."""
    directory = Path(root) / task.id
    if directory.exists():
        shutil.rmtree(directory)
    tags = ["teams-eval", f"teams-{task.kind}"]
    max_steps, wall, tokens = task.budgets
    lines = [f'id = "{task.id}"', f"title = {json.dumps(task.title)}", f"tags = {json.dumps(tags)}",
             f"budgets = {{ max_steps = {max_steps}, wall_clock_secs = {wall}, max_tokens = {tokens} }}",
             f"protected = {json.dumps(task.protected)}",
             f'check = {{ image = "runtime", timeout_secs = {task.check_timeout} }}']
    if task.template:
        lines.append(f'template = "{task.template}"')
    text_or_bytes(directory / "task.toml", "\n".join(lines) + "\n")
    text_or_bytes(directory / "task.md", task.statement.strip() + "\n")
    text_or_bytes(directory / "key_facts.json", json.dumps(task.key_facts, indent=1, ensure_ascii=False) + "\n")
    for path, content in sorted(task.seed.items()):
        text_or_bytes(directory / "seed" / path, content)
    text_or_bytes(directory / "check" / "run.sh", RUN_SH)
    text_or_bytes(directory / "check" / "checklib.py", CHECKLIB)
    text_or_bytes(directory / "check" / "check.py", task.check.strip() + "\n")
    if task.expected is not None:
        text_or_bytes(directory / "check" / "expected.json",
                      json.dumps(task.expected, indent=1, sort_keys=True, ensure_ascii=False) + "\n")
    for name, content in sorted(task.hidden.items()):
        text_or_bytes(directory / "check" / "hidden" / name, content)
    for name, content in sorted(task.check_files.items()):
        text_or_bytes(directory / "check" / name, content)
    for side, scenario in (("single", dryrun.single(task)), ("team", dryrun.team(task))):
        text_or_bytes(directory / "dry-run" / f"{side}.json", json.dumps(scenario, indent=1, ensure_ascii=False) + "\n")
    for name, files in (("solution", task.solution), ("wrong", task.wrong)):
        for path, content in sorted(files.items()):
            text_or_bytes(directory / "answers" / name / path, content)
    return directory
