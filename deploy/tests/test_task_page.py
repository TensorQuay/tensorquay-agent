"""The feature matrix (G6): the task-page scenario and its evidence."""

import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import Mock, patch

import demo_task_page

# This file's fixtures, found from its own location so the suite passes from any working directory.
FIXTURES = Path(__file__).resolve().parent / "fixtures"

HELD = "00000000-0000-0000-0000-000000000001"
LONG = "00000000-0000-0000-0000-000000000002"


class TaskPageDriverTests(unittest.TestCase):
    def test_the_scenario_holds_one_reply_and_streams_another(self):
        scenario = json.loads((FIXTURES / "task-page.json").read_text())
        self.assertEqual(scenario["select"], "prompt")
        # The first two are this suite's own; later scenarios are appended after them (the c15 stack gaps).
        held, long = scenario["responses"][:2]
        self.assertEqual(held["when"], "Hold this reply")
        self.assertEqual(held["wait_ms"], 600000)
        self.assertEqual(long["when"], "Write a long answer with code")
        text = "".join(event["text"] for event in long["events"])
        self.assertIn("```python", text)
        self.assertTrue(text.endswith("End of the long answer."))
        self.assertGreaterEqual(len(long["events"]), 20)

    def test_scenario_mount_is_single_read_only_and_exact(self):
        mount = {"type": "bind", "source": str((FIXTURES / "task-page.json").resolve()),
                 "target": "/scenarios/task-page.json", "read_only": True}
        for changes in ({}, {"read_only": False}, {"source": "unexpected"}, {"target": "/etc/config"}):
            response = {"services": {"fake-model": {"volumes": [{**mount, **changes}]}}}
            with patch.object(demo_task_page.compose, "command", return_value=Mock(stdout=json.dumps(response))):
                if changes:
                    with self.assertRaises(AssertionError):
                        demo_task_page.verify_overlay({}, ["docker", "compose"])
                else:
                    demo_task_page.verify_overlay({}, ["docker", "compose"])

    def evidence(self, root, record):
        result = root / "result.json"
        result.write_text(json.dumps(record))
        return {"TQA_STACK_RESULT": str(result)}

    def test_the_evidence_is_accepted(self):
        with tempfile.TemporaryDirectory() as temporary:
            demo_task_page.verify_result(self.evidence(
                Path(temporary), {"held": {"id": HELD, "run_status": "cancelled"}, "long": {"id": LONG}}))

    def test_wrong_evidence_is_refused(self):
        for record in ({"held": {"id": HELD, "run_status": "completed"}, "long": {"id": LONG}},
                       {"held": {"id": HELD, "run_status": "cancelled"}},
                       {"held": {"id": "nope", "run_status": "cancelled"}, "long": {"id": LONG}}):
            with self.subTest(record=record), tempfile.TemporaryDirectory() as temporary:
                with self.assertRaises((AssertionError, KeyError, ValueError)):
                    demo_task_page.verify_result(self.evidence(Path(temporary), record))


if __name__ == "__main__":
    unittest.main()
