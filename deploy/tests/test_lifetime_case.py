"""The shared GW-I-066 lifetime case's scenario and checks, without a stack."""

import sys
from pathlib import Path
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parent))
import lifetime_case as case  # noqa: E402


def frame(kind, **fields):
    return {"event": dict(fields, type=kind)}


def run(statuses, *, resumed=1, end="run.completed", last="kept\n"):
    stored = [frame("run.started")]
    for index, status in enumerate(statuses):
        stored += [frame("tool.start"), frame("tool.result", status=status,
                                              content=last if index == len(statuses) - 1 else "step\n")]
        if status == "interrupted":
            stored += [frame("run.resumed")] * resumed
    return stored + [frame(end)]


GOOD = ["ok"] * 9 + ["interrupted"] + ["ok"] * 3
LOG = 'WARN tqa_gateway::manager::worker: sandbox reached its max lifetime conversation_id=c how="busy"\n'


class Scenario(unittest.TestCase):
    def test_the_calls_outlast_the_cap_and_the_grace_and_each_stays_under_the_shell_timeout(self):
        self.assertEqual(case.SETTINGS, "max_lifetime_seconds=60\n")
        self.assertGreater(case.STEPS * case.STEP_SECONDS, case.MAX_LIFETIME_SECONDS + case.GRACE_SECONDS)
        self.assertLess(case.STEP_SECONDS, 30)
        commands = [response["events"][0]["arguments"]["command"] for response in case.SCENARIO["responses"][:-1]]
        self.assertEqual(len(commands), case.STEPS + 2)
        self.assertEqual((commands[0], commands[-1]), ("printf kept > marker", "cat marker"))

    def test_what_is_left_after_the_retirement_fits_the_new_sandboxs_cap(self):
        left = case.STEPS * case.STEP_SECONDS - (case.MAX_LIFETIME_SECONDS + case.GRACE_SECONDS)
        self.assertLess(left, case.MAX_LIFETIME_SECONDS)


class Check(unittest.TestCase):
    def test_a_busy_retirement_after_the_grace_passes(self):
        self.assertIn("retired 185 s in", case.check(run(GOOD), 185.0, LOG))

    def test_a_retirement_before_the_grace_ends_fails(self):
        with self.assertRaisesRegex(AssertionError, "retired 70"):
            case.check(run(GOOD), 70.0, LOG)

    def test_no_retirement_fails(self):
        with self.assertRaises(AssertionError):
            case.check(run(["ok"] * 13, resumed=0), None, LOG)

    def test_a_second_interruption_fails(self):
        with self.assertRaises(AssertionError):
            case.check(run(["ok"] * 5 + ["interrupted"] * 2 + ["ok"] * 6), 185.0, LOG)

    def test_a_failed_run_fails(self):
        with self.assertRaises(AssertionError):
            case.check(run(GOOD, end="run.failed"), 185.0, LOG)

    def test_a_lost_workspace_fails(self):
        with self.assertRaises(AssertionError):
            case.check(run(GOOD, last="cat: marker: No such file\n"), 185.0, LOG)

    def test_the_gateway_must_say_it_retired_a_busy_sandbox(self):
        with self.assertRaisesRegex(AssertionError, "no 'sandbox reached"):
            case.check(run(GOOD), 185.0, "")
        with self.assertRaises(AssertionError):
            case.check(run(GOOD), 185.0, LOG.replace("busy", "idle"))


class Watch(unittest.TestCase):
    def test_times_the_first_interruption_and_returns_at_the_end(self):
        now, pages = [0.0], [run(["ok"])[:-1], run(["ok", "interrupted"])[:-1], run(["ok", "interrupted", "ok"])]

        def fetch():
            return pages.pop(0) if len(pages) > 1 else pages[0]

        def sleep(seconds):
            now[0] += 100

        stored, interrupted = case.watch(fetch, clock=lambda: now[0], sleep=sleep)
        self.assertEqual(interrupted, 100)
        self.assertEqual(case.kinds(stored)[-1], "run.completed")

    def test_a_run_that_never_ends_fails_in_bounded_time(self):
        now = [0.0]

        def sleep(seconds):
            now[0] += 60

        with self.assertRaisesRegex(AssertionError, "did not end"):
            case.watch(lambda: run(["ok"])[:-1], seconds=300, clock=lambda: now[0], sleep=sleep)


if __name__ == "__main__":
    unittest.main()
