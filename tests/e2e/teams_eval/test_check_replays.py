"""Two checks against the shapes of answers eval night 1 (11 Oct 2026, candidate16b) gave them, replayed offline: D4
takes a new test written into the project's existing test file and runs only the new tests on the two commits; R1
doesn't take "DNS misconfiguration" for a configuration push, and still takes "Misconfigured pushes" for one.

Run: python3 -m unittest tests/e2e/teams_eval/test_check_replays.py"""

import json
import shutil
import sys
import tempfile
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))

from test_fixtures import check  # noqa: E402

TASKS = HERE / "tasks"


def verdict(output):
    return json.loads(output.splitlines()[0])


class Replays(unittest.TestCase):
    def setUp(self):
        self.scratch = tempfile.TemporaryDirectory()
        self.addCleanup(self.scratch.cleanup)

    def workspace(self, task, answer=None):
        workspace = Path(self.scratch.name) / task
        shutil.copytree(TASKS / task / "seed", workspace)
        if answer:
            shutil.copytree(TASKS / task / "answers" / answer, workspace, dirs_exist_ok=True)
        return workspace

    def d4_with_method(self, method):
        """The reference answer with its new test file folded into test_wrap.py's Wrap class, as both sides did."""
        workspace = self.workspace("d4-bisect-test-fix", "solution")
        tests = workspace / "project" / "tests"
        (tests / "test_regression.py").unlink()
        with open(tests / "test_wrap.py", "a", encoding="utf-8") as existing:
            existing.write(method)
        return workspace

    def test_d4_takes_a_new_test_in_the_existing_file(self):
        workspace = self.d4_with_method(
            "\n    def test_a_line_exactly_width_long_stays_whole(self):\n"
            "        self.assertEqual(wrap(\"aaaa bbbb\", 9), [\"aaaa bbbb\"])\n")
        code, output = check(TASKS / "d4-bisect-test-fix", workspace)
        self.assertEqual(code, 0, output[-1500:])

    def test_d4_runs_only_the_new_tests_on_the_culprit(self):
        # test_breaks fails on the culprit as well, so the whole suite there would pass any new test as "catching" it.
        workspace = self.d4_with_method(
            "\n    def test_one_word(self):\n        self.assertEqual(wrap(\"a\", 5), [\"a\"])\n")
        code, output = check(TASKS / "d4-bisect-test-fix", workspace)
        self.assertEqual(code, 1, output[-1500:])
        self.assertIn("passes on the culprit", verdict(output)["reason"])

    def test_d4_without_a_new_test_fails_naming_it(self):
        workspace = self.d4_with_method("")
        code, output = check(TASKS / "d4-bisect-test-fix", workspace)
        self.assertEqual((code, verdict(output)["reason"]), (1, "no new test in project/tests/"))

    def r1_answer(self, headings):
        """causes.md with these (heading, cause) pairs, each citing three real incidents of its cause."""
        truth = json.loads((TASKS / "r1-incident-causes" / "check" / "expected.json").read_text())
        workspace = self.workspace("r1-incident-causes")
        parts = []
        for heading, key in headings:
            cited = sorted(path for path, cause in truth["causes"].items() if cause == key)[:3]
            parts.append(f"## {heading}\n\n" + "".join(f"- {path}\n" for path in cited))
        (workspace / "causes.md").write_text("\n".join(parts), encoding="utf-8")
        return workspace

    def test_r1_dns_misconfiguration_is_not_a_config_push(self):
        # Night 1's single side: config push counted 5 (split in two), so DNS came third.
        workspace = self.r1_answer([
            ("Disk space exhaustion from unrotated logs (9 incidents)", "disk-full"),
            ("Expired TLS certificates (8 incidents)", "expired-certificate"),
            ("DNS misconfiguration (6 incidents)", "dns"),
            ("Unreviewed config push switched a service to a test backend (5 incidents)", "config-push"),
            ("Memory leak in service or cache client (5 incidents)", "memory-leak")])
        code, output = check(TASKS / "r1-incident-causes", workspace)
        self.assertEqual(code, 1, output[-1500:])
        self.assertEqual(verdict(output)["reason"],
                         "cause 3 should be Bad configuration push, found 'DNS misconfiguration (6 incidents)'")

    def test_r1_a_misconfigured_push_still_names_the_config_push(self):
        workspace = self.r1_answer([
            ("Disks full of logs (9)", "disk-full"),
            ("Expired TLS certificate (8)", "expired-certificate"),
            ("Misconfigured pushes (7)", "config-push"),
            ("DNS misconfiguration (6)", "dns"),
            ("Memory leak (5)", "memory-leak")])
        code, output = check(TASKS / "r1-incident-causes", workspace)
        self.assertEqual(code, 0, output[-1500:])


if __name__ == "__main__":
    unittest.main()
