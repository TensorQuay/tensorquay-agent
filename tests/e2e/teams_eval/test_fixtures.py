"""The agent-teams evaluation's tasks without a stack: each check fails on its seed, passes the reference answer and
fails the wrong one (the seed check tqa-e2e asks of a task, run here on the host's python3); the set matches the
design; and the committed tasks are what the fixtures generate.

Run: python3 -m unittest tests/e2e/teams_eval/test_fixtures.py"""

import json
import os
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))

import generate  # noqa: E402
import runner  # noqa: E402

DESIGN = {"research": 6, "report": 5, "code": 6, "mixed": 3, "dependent": 5, "long": 2}


def check(task_dir, workspace):
    environment = dict(os.environ, PYTHONPATH=str(workspace), PYTHONDONTWRITEBYTECODE="1")
    result = subprocess.run([sys.executable, str(task_dir / "check" / "check.py"), str(workspace)], cwd=workspace,
                            capture_output=True, text=True, timeout=120, env=environment)
    return result.returncode, result.stdout + result.stderr


class Tasks(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.scratch = tempfile.TemporaryDirectory()
        cls.root = Path(cls.scratch.name) / "tasks"
        cls.names = generate.write_all(cls.root)

    @classmethod
    def tearDownClass(cls):
        cls.scratch.cleanup()

    def workspace(self, task_dir, answer=None, label=None):
        workspace = Path(self.scratch.name) / "work" / task_dir.name / (label or answer or "seed")
        shutil.copytree(task_dir / "seed", workspace)
        if answer:
            shutil.copytree(task_dir / "answers" / answer, workspace, dirs_exist_ok=True)
        return workspace

    def test_each_check_fails_the_seed_passes_the_solution_and_fails_the_wrong_answer(self):
        for name in self.names:
            task_dir = self.root / name
            with self.subTest(task=name):
                for answer, expected in ((None, 1), ("solution", 0), ("wrong", 1)):
                    code, output = check(task_dir, self.workspace(task_dir, answer))
                    self.assertEqual(code, expected, f"{answer or 'seed'}: {output[-1500:]}")
                    self.assertIn('"passed"', output.splitlines()[0])

    def test_each_scripted_session_solves_its_task(self):
        """The files each session's lead writes (and, for the team, the helper's taken file) pass the check."""
        for name in self.names:
            task_dir = self.root / name
            statement = (task_dir / "task.md").read_text()
            for side in ("single", "team"):
                with self.subTest(task=name, side=side):
                    scenario = json.loads((task_dir / "dry-run" / f"{side}.json").read_text())
                    self.assertEqual(scenario["select"], "prompt")
                    self.assertLess(len(json.dumps(scenario).encode()), 1 << 20, "the scripted model's bound")
                    lead = [r for r in scenario["responses"] if not r["when"].startswith("You are ")]
                    self.assertTrue(all(r["when"] in statement for r in lead), "the lead's phrase is in task.md")
                    self.assertTrue(all(1 <= len(r["events"]) <= 128 for r in scenario["responses"]))
                    workspace = self.workspace(task_dir, label=f"{side}-session")
                    calls = [r["events"][0] for r in lead if r["events"][0]["type"] == "tool_call"]
                    for event in calls:
                        if event["name"] == "shell":
                            subprocess.run(["sh", "-c", event["arguments"]["command"]], cwd=workspace, check=True,
                                           capture_output=True, timeout=300)
                        if event["name"] == "write_file":
                            target = workspace / event["arguments"]["path"]
                            target.parent.mkdir(parents=True, exist_ok=True)
                            target.write_text(event["arguments"]["content"])
                    names = [event["name"] for event in calls]
                    if side == "team":
                        self.assertEqual([n for n in names if n not in ("write_file", "shell")],
                                         ["start_member", "read_reports", "take_files", "done"])
                        taken = workspace / "uploads" / "scout-scout-notes.md"
                        taken.parent.mkdir(exist_ok=True)
                        taken.write_text("./README.md\n")
                    code, output = check(task_dir, workspace)
                    self.assertEqual(code, 0, output[-1500:])

    def test_the_set_matches_the_design(self):
        found = runner.select_tasks(self.root)
        kinds = {}
        for task in found:
            kinds[task.kind] = kinds.get(task.kind, 0) + 1
            self.assertTrue(task.key_facts)
            self.assertTrue((task.directory / "check" / "run.sh").is_file())
        expected = {kind: count for kind, count in DESIGN.items() if kind in kinds}
        self.assertEqual(kinds, expected)
        numbers = [task.id.split("-", 1)[0] for task in found]
        self.assertEqual(len(numbers), len(set(numbers)), "one task per design number")

    def test_the_committed_tasks_are_generated(self):
        self.assertEqual(generate.main(["--verify"]), 0)

    def test_answers_never_reach_the_seed(self):
        for name in self.names:
            self.assertFalse(any((self.root / name / "seed").rglob("answers")), name)
            facts = json.loads((self.root / name / "key_facts.json").read_text())
            self.assertTrue(all(isinstance(fact, str) for fact in facts))


if __name__ == "__main__":
    unittest.main()
