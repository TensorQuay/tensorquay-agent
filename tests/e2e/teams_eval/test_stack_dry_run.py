"""The stack dry run's verdict without a stack: what a passing run of both arms on the scripted model must show.

Run: python3 -m unittest tests/e2e/teams_eval/test_stack_dry_run.py"""

import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import stack_dry_run  # noqa: E402


def line(side, **fields):
    good = {"task": "r1", "side": side, "attempt": 1, "passed": True, "failures": [],
            "helpers": 1 if side == "team" else 0, "stop_reason": "done" if side == "team" else None}
    return {**good, **fields}


TASKS = ["r1"]
SWITCHES = [{"side": "single"}, {"side": "team"}]


class Verdicts(unittest.TestCase):
    def test_a_clean_run_passes(self):
        self.assertEqual(stack_dry_run.verdicts([line("single"), line("team")], TASKS, SWITCHES), [])

    def test_each_shortfall_is_named(self):
        found = stack_dry_run.verdicts([line("single", helpers=1), line("team", passed=False, helpers=0,
                                                                         stop_reason="deadline",
                                                                         failures=["deadline"])], TASKS, SWITCHES[:1])
        text = "\n".join(found)
        for expected in ("a single agent has a team", "r1 team: not passed", "failure classes ['deadline']",
                         "0 helper(s), ended by deadline", "1 switches, not 2"):
            self.assertIn(expected, text)

    def test_a_rerun_replaces_its_run(self):
        lines = [line("single", passed=None, rerun="provider_error"), line("single", attempt="1r", rerun_of=1),
                 line("team")]
        self.assertEqual(stack_dry_run.verdicts(lines, TASKS, SWITCHES), [])


if __name__ == "__main__":
    unittest.main()
