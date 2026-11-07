"""WEB-E2E-018: real admission uses one slot, held by a scripted model call until Stop."""

import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import Mock, patch

import demo_queue

# This file's fixtures, found from its own location so the suite passes from any working directory.
FIXTURES = Path(__file__).resolve().parent / "fixtures"


class QueueDriverTests(unittest.TestCase):
    def test_queue_configuration_limits_slots_and_writes_no_permissions(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            config = root / "gateway.toml"
            config.write_text("[sandbox]\nmax_live=4\n")
            with patch.object(demo_queue.compose, "ownership"):
                demo_queue.configure(root, {})
            body = config.read_text()
            self.assertIn("[admission]\nslots=1\nper_user=1\n", body)
            self.assertNotIn("permissions", body)
            self.assertNotIn("idle_seconds", body)
            self.assertEqual(config.stat().st_mode & 0o777, 0o600)

    def test_the_holder_is_held_until_stop_and_the_queued_task_answers_at_once(self):
        scenario = json.loads((FIXTURES / "queue.json").read_text())
        self.assertEqual(scenario["select"], "prompt")
        by_task = {}
        for response in scenario["responses"]:
            by_task.setdefault(response["when"], []).append(response)
        self.assertEqual(set(by_task), {demo_queue.HOLD, demo_queue.QUEUED})
        [held] = by_task[demo_queue.HOLD]
        self.assertEqual(held["wait_ms"], 600000)
        self.assertEqual([event["type"] for event in held["events"]], ["text"])
        [answer] = by_task[demo_queue.QUEUED]
        self.assertNotIn("wait_ms", answer)
        self.assertEqual([event["type"] for event in answer["events"]], ["text"])

    def test_queue_scenario_mount_must_be_single_exact_and_read_only(self):
        mount = {"type": "bind", "source": str((FIXTURES / "queue.json").resolve()),
                 "target": "/scenarios/queue.json", "read_only": True}
        for changes in ({}, {"read_only": False}, {"source": "unexpected"}, {"target": "/etc/config"}):
            response = {"services": {"fake-model": {"volumes": [{**mount, **changes}]}}}
            with patch.object(demo_queue.compose, "command", return_value=Mock(stdout=json.dumps(response))):
                if changes:
                    with self.assertRaises(AssertionError):
                        demo_queue.verify_overlay({}, ["docker", "compose"])
                else:
                    demo_queue.verify_overlay({}, ["docker", "compose"])


    def result(self, runs):
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        path = Path(directory.name) / "result.json"
        path.write_text(json.dumps({"runs": runs}))
        return {"TQA_STACK_RESULT": str(path)}

    def test_the_result_holds_a_stopped_holder_and_a_done_task_per_width(self):
        # The outcomes are each task's run_status as the gateway's task list reports it, not words the spec wrote.
        pair = lambda n: [{"conversationId": "h%d" % n, "outcome": "cancelled"},
                          {"conversationId": "q%d" % n, "outcome": "completed"}]
        runs = pair(1) + pair(2) + pair(3) + pair(4)
        demo_queue.verify_result(self.result(runs))
        swapped = [dict(run, outcome="completed" if run["outcome"] == "cancelled" else "cancelled") for run in runs]
        same = pair(1)[:1] + [{"conversationId": "h1", "outcome": "completed"}] + runs[2:]
        # The spec's own old words are not a status the gateway reports.
        words = [dict(run, outcome={"cancelled": "stopped", "completed": "done"}[run["outcome"]]) for run in runs]
        failed = runs[:1] + [dict(runs[1], outcome="failed")] + runs[2:]
        for wrong in (runs[:6], swapped, same, words, failed, [], [dict(runs[0], conversationId="")] + runs[1:]):
            with self.subTest(wrong=wrong), self.assertRaises(AssertionError):
                demo_queue.verify_result(self.result(wrong))


if __name__ == "__main__":
    unittest.main()
