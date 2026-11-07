"""Writes the agent-teams evaluation's tasks into tasks/ from their ground truth in fixtures/: python3 generate.py.
`--verify` regenerates them elsewhere and fails when the committed tasks differ, so nobody edits a task by hand.

The tasks live apart from tests/e2e/tasks on purpose: every directory there joins every tqa-e2e run (the gates'
too), while these run only through runner.py. Each is a tqa-e2e task directory (task.toml, task.md, seed/, check/),
so `tqa-e2e --check-only --tasks tests/e2e/teams_eval/tasks` checks an attempt the way an attempt's check runs, plus
key_facts.json for the judges, answers/ (a passing and a failing answer) for the offline test, and
dry-run/ (the scripted model's sessions, one per side) for stack_dry_run.py.
"""

import argparse
import filecmp
import shutil
import sys
import tempfile
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE / "fixtures"))

import common  # noqa: E402
import research  # noqa: E402

TASKS_DIR = HERE / "tasks"
MODULES = [research]
for name in ("reports", "code_tasks", "mixed", "dependent", "long_tasks"):
    if (HERE / "fixtures" / f"{name}.py").is_file():
        MODULES.append(__import__(name))


def tasks():
    return [make() for module in MODULES for make in module.TASKS]


def write_all(root):
    root = Path(root)
    root.mkdir(parents=True, exist_ok=True)
    made = {common.write(task, root).name for task in tasks()}
    for stale in root.iterdir():
        if stale.is_dir() and stale.name not in made:
            shutil.rmtree(stale)
    return sorted(made)


def differences(left, right):
    comparison = filecmp.dircmp(left, right)
    out = [f"only in {comparison.left}: {name}" for name in comparison.left_only]
    out += [f"only in {comparison.right}: {name}" for name in comparison.right_only]
    out += [f"differs: {Path(comparison.left) / name}" for name in comparison.diff_files]
    for name in comparison.common_dirs:
        out += differences(Path(left) / name, Path(right) / name)
    return out


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--verify", action="store_true")
    args = parser.parse_args(argv)
    if not args.verify:
        print("\n".join(write_all(TASKS_DIR)))
        return 0
    with tempfile.TemporaryDirectory() as scratch:
        write_all(scratch)
        found = differences(scratch, TASKS_DIR) if TASKS_DIR.is_dir() else ["tasks/ is missing"]
    if found:
        print("tasks/ differs from the fixtures; run python3 generate.py\n" + "\n".join(found[:20]))
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
