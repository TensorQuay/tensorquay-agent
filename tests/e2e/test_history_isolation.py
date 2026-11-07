"""SEC-017's stack-half checks without a stack. Run: python3 -m unittest tests/e2e/test_history_isolation.py"""

import json
from pathlib import Path
import sys
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parent))
import history_isolation_checks as checks  # noqa: E402

FIRST = "0192d0a1-0000-7000-8000-000000000001"
SECOND = "0192d0a1-0000-7000-8000-000000000002"


def mounts(state_source):
    return [
        {"Type": "bind", "Source": "/srv/tqa/workspaces/0192d0a1-0000-7000-8000-00000000000a",
         "Destination": "/workspace"},
        {"Type": "bind", "Source": state_source, "Destination": checks.STATE_PATH},
        {"Type": "bind", "Source": "/srv/tqa/tokens/" + SECOND, "Destination": "/run/tqa/token"},
    ]


def frame(seq, kind, **fields):
    return {"type": "event", "seq": seq, "event": dict(fields, type=kind)}


class Mounts(unittest.TestCase):
    def test_the_seconds_own_state_passes_and_the_firsts_fails(self):
        self.assertEqual(checks.check_mounts(mounts("/srv/tqa/runtime-state/" + SECOND), FIRST, SECOND)["mounts"], 3)
        for source in ("/srv/tqa/runtime-state/" + FIRST, "/srv/tqa/runtime-state/shared"):
            with self.subTest(source), self.assertRaises(AssertionError):
                checks.check_mounts(mounts(source), FIRST, SECOND)

    def test_any_mount_naming_the_first_fails(self):
        extra = mounts("/srv/tqa/runtime-state/" + SECOND) + [{"Source": "/srv/x/" + FIRST, "Destination": "/opt/x"}]
        with self.assertRaises(AssertionError):
            checks.check_mounts(extra, FIRST, SECOND)

    def test_two_state_mounts_fail(self):
        doubled = mounts("/srv/tqa/runtime-state/" + SECOND) * 2
        with self.assertRaises(AssertionError):
            checks.check_mounts(doubled, FIRST, SECOND)


class Walk(unittest.TestCase):
    control = "0192d0a1-0000-7000-8000-0000000000cc"

    def test_ids_are_read_from_the_output_and_the_first_must_be_absent(self):
        seen = "{}\n{}\n".format(self.control, SECOND)
        self.assertEqual(checks.check_walk(seen, FIRST, SECOND, self.control),
                         {"ids_seen": 2, "control_seen": True, "second_seen": True})
        with self.assertRaises(AssertionError):
            checks.check_walk(seen + FIRST + "\n", FIRST, SECOND, self.control)

    def test_a_walk_that_missed_the_control_proves_nothing(self):
        with self.assertRaises(AssertionError):
            checks.check_walk(SECOND + "\n", FIRST, SECOND, self.control)
        with self.assertRaises(AssertionError):
            checks.check_walk("", FIRST, SECOND, self.control)

    def test_the_walk_crosses_mounts_and_prunes_the_kernel_filesystems(self):
        self.assertNotIn("-xdev", checks.WALK)
        for path in ("/proc", "/sys", "/dev"):
            self.assertIn("-path " + path, checks.WALK)
        self.assertIn("grep -oE", checks.WALK)
        self.assertTrue(checks.control_command(self.control).endswith("/workspace/.tqa-probe/" + self.control))


class Forgery(unittest.TestCase):
    def test_the_forged_log_is_marked_and_names_the_first(self):
        lines = [json.loads(line) for line in checks.forged_log(FIRST).splitlines()]
        self.assertEqual([line["seq"] for line in lines], [1, 2])
        self.assertTrue(all(checks.MARKER in json.dumps(line) for line in lines))
        self.assertIn(FIRST, lines[0]["event"]["text"])
        for path in checks.FORGED_PATHS:
            self.assertTrue(path.startswith("/workspace/"), path)
            self.assertIn(path, checks.forge_command())


class History(unittest.TestCase):
    before = [frame(1, "message.user", text="first"), frame(2, "run.started", run_id="r1"),
              frame(3, "run.completed", run_id="r1")]

    def test_an_unchanged_prefix_and_a_completed_resume_pass(self):
        after = self.before + [frame(4, "message.user", text="again"), frame(5, "run.completed", run_id="r2")]
        self.assertEqual(checks.check_history(self.before, after), {"before": 3, "after": 5})

    def test_a_changed_prefix_a_marker_or_no_resume_fail(self):
        changed = [frame(1, "message.user", text="rewritten")] + self.before[1:] + [frame(4, "run.completed")]
        marked = self.before + [frame(4, "text.delta", text=checks.MARKER), frame(5, "run.completed")]
        for after in (changed, marked, list(self.before), self.before + [frame(4, "run.failed")]):
            with self.subTest(len(after)), self.assertRaises(AssertionError):
                checks.check_history(self.before, after)


if __name__ == "__main__":
    unittest.main()
