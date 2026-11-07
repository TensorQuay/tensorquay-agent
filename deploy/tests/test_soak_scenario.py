"""SOAK-002: paced work stays finite and script exhaustion cannot look like success."""
import json
import unittest

from soak_audit import AuditError
from soak_scenario import ScenarioProgress, scenario


class ScenarioTests(unittest.TestCase):
    def test_full_script_fits_the_server_bounds_and_keeps_doing_tools(self):
        document = scenario(False)
        self.assertLess(len(json.dumps(document).encode()), 1048576)
        self.assertEqual(len(document["responses"]), 127)
        for index, reply in enumerate(document["responses"][:-1]):
            self.assertEqual(reply["delay_ms"], 1000)
            self.assertLessEqual(len(reply["events"]), 128)
            calls = [event for event in reply["events"] if event["type"] == "tool_call"]
            self.assertEqual(len(calls), 0 if index % 3 == 2 else 1)
        commands = [reply["events"][-1]["arguments"]["command"]
                    for reply in document["responses"][:-1:3]]
        self.assertEqual(len(set(commands)), 42)
        self.assertTrue(all(command.startswith("printf soak-") for command in commands))

    def test_marker_detects_exhaustion_regression_and_skipped_responses(self):
        for bad in ("SOAK_RESPONSE_126\n", "SOAK_RESPONSE_002\n", "SOAK_SCRIPT_EXHAUSTED\n"):
            progress = ScenarioProgress()
            progress.record({"type": "text.delta", "text": "SOAK_RESPONSE_000\n"})
            progress.record({"type": "model.completed"})
            with self.subTest(bad=bad), self.assertRaises(AuditError):
                progress.record({"type": "text.delta", "text": bad})

    def test_retried_response_is_allowed_but_each_completion_advances_once(self):
        progress = ScenarioProgress()
        for _ in range(3):
            progress.record({"type": "text.delta", "text": "SOAK_RESPONSE_000\n"})
        progress.record({"type": "model.completed"})
        progress.record({"type": "text.delta", "text": "SOAK_RESPONSE_001\n"})
        progress.record({"type": "model.completed"})
        self.assertEqual(progress.completed, 2)
        with self.assertRaises(AuditError):
            progress.record({"type": "text.delta", "text": "SOAK_RESPONSE_000\n"})


if __name__ == "__main__":
    unittest.main()
