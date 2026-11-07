"""The shared E2E-IDLE-019 idle case's scenario, bounds and checks, without a stack."""

import sys
from pathlib import Path
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parent))
import idle_case as case  # noqa: E402


def frame(seq, kind, **fields):
    return {"seq": seq, "event": dict(fields, type=kind)}


def woken(last=case.MARKER + "\n"):
    kinds = [("run.started", {}), ("tool.result", {"status": "ok", "content": ""}), ("run.completed", {}),
             ("run.started", {}), ("tool.result", {"status": "ok", "content": last}), ("run.completed", {})]
    return [frame(seq, kind, **fields) for seq, (kind, fields) in enumerate(kinds, start=1)]


class Clock:
    def __init__(self):
        self.now = 100.0

    def __call__(self):
        return self.now

    def sleep(self, seconds):
        self.now += seconds


class Scenario(unittest.TestCase):
    def test_the_window_parts_a_delete_from_an_idle_retirement_and_the_stopped_call_fits_the_shell_timeout(self):
        self.assertEqual(case.SETTINGS, "idle_seconds=45\n")
        self.assertLess(case.DELETE_SECONDS, case.IDLE_SECONDS - case.EARLY_SECONDS)
        self.assertLess(case.STOPPED_CALL_SECONDS, 30)
        self.assertEqual(case.SCENARIO["select"], "prompt")

    def test_each_task_follows_its_own_script(self):
        def script(when):
            return [response["events"][0].get("arguments", {}).get("command", response["events"][0].get("text"))
                    for response in case.SCENARIO["responses"] if response["when"] == when]
        self.assertEqual({response["when"] for response in case.SCENARIO["responses"]},
                         {case.FINISHED, case.STOPPED, case.DELETED})
        self.assertEqual(script(case.FINISHED)[0::2],
                         ["printf '{}' > marker".format(case.MARKER), "cat marker"])
        self.assertEqual(script(case.STOPPED)[0], "echo started && sleep {}".format(case.STOPPED_CALL_SECONDS))
        self.assertEqual(script(case.DELETED), ["Done."])
        # No phrase is part of another, so no message matches two scripts.
        phrases = [case.FINISHED, case.STOPPED, case.DELETED]
        self.assertFalse([a for a in phrases for b in phrases if a != b and a in b])

    def test_retirement_counts_only_inside_the_window(self):
        self.assertIn("destroyed 50 s", case.check_retired(50, "finished"))
        for seconds in (case.IDLE_SECONDS - case.EARLY_SECONDS - 1, case.IDLE_SECONDS + case.LATE_SECONDS + 1):
            with self.assertRaises(AssertionError):
                case.check_retired(seconds, "finished")

    def test_a_delete_counts_only_at_once(self):
        case.check_deleted(2)
        with self.assertRaises(AssertionError):
            case.check_deleted(case.DELETE_SECONDS + 1)

    def test_until_gone_measures_from_the_run_end_and_is_bounded(self):
        clock = Clock()
        since, polls = clock(), iter([True, True, True, False])
        self.assertEqual(case.until_gone(lambda: next(polls), since, 10, clock, clock.sleep), 1.5)
        with self.assertRaises(AssertionError):
            case.until_gone(lambda: True, clock(), 2, clock, clock.sleep)

    def test_woken_needs_the_marker_a_new_sandbox_and_one_contiguous_conversation(self):
        self.assertIn("new sandbox", case.check_woken(woken(), "a", "b"))
        for stored, first, second in ((woken(), "a", "a"), (woken("missing"), "a", "b"), (woken()[:-1], "a", "b"),
                                      (woken()[:2] + woken()[3:], "a", "b")):
            with self.assertRaises(AssertionError):
                case.check_woken(stored, first, second)


if __name__ == "__main__":
    unittest.main()
