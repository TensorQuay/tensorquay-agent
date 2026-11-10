"""Short task titles on the real stack: the titles scenario, the suite's configuration and its evidence."""

import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import Mock, patch

import demo_titles


class TitlesDriverTests(unittest.TestCase):
    def test_the_configuration_turns_short_titles_on_for_this_suite_only(self):
        for base in ("[sandbox]\nmax_live=4\n[tasks]\nauto_title=false\n", "[sandbox]\nmax_live=4\n"):
            with self.subTest(base=base), tempfile.TemporaryDirectory() as temporary:
                config = Path(temporary) / "gateway.toml"
                config.write_text(base)
                with patch.object(demo_titles.compose, "ownership"):
                    demo_titles.configure(Path(temporary), {})
                body = config.read_text()
                self.assertEqual(body.count("[tasks]"), 1, body)
                self.assertIn("[tasks]\nauto_title=true\n", body)
                self.assertNotIn("auto_title=false", body)
                self.assertEqual(config.stat().st_mode & 0o777, 0o600)

    def test_each_task_follows_its_own_script_and_the_title_call_gets_the_summary(self):
        scenario = json.loads(Path("deploy/tests/fixtures/titles.json").read_text())
        self.assertEqual(scenario["select"], "prompt")
        by_task = {}
        for response in scenario["responses"]:
            by_task.setdefault(response["when"], []).append(response)
        self.assertEqual(set(by_task), {demo_titles.NAMED, demo_titles.RENAMED})
        # Two plain turns for the task the model names, one for the one the person renamed; no tool calls anywhere.
        self.assertEqual(len(by_task[demo_titles.NAMED]), 2)
        self.assertEqual(len(by_task[demo_titles.RENAMED]), 1)
        for replies in by_task.values():
            for reply in replies:
                self.assertEqual([event["type"] for event in reply["events"]], ["text"])
        # The tool-less title call is answered by the summary reply, before any selection, so no task's reply is used.
        self.assertEqual(scenario["summary"]["events"], [{"type": "text", "text": demo_titles.TITLE}])

    def test_scenario_mount_is_single_read_only_and_exact(self):
        mount = {"type": "bind", "source": str(Path("deploy/tests/fixtures/titles.json").resolve()),
                 "target": "/scenarios/titles.json", "read_only": True}
        for changes in ({}, {"read_only": False}, {"source": "unexpected"}, {"target": "/etc/config"}):
            response = {"services": {"fake-model": {"volumes": [{**mount, **changes}]}}}
            with patch.object(demo_titles.compose, "command", return_value=Mock(stdout=json.dumps(response))):
                if changes:
                    with self.assertRaises(AssertionError):
                        demo_titles.verify_overlay({}, ["docker", "compose"])
                else:
                    demo_titles.verify_overlay({}, ["docker", "compose"])

    def result(self, a, b):
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        path = Path(directory.name) / "result.json"
        path.write_text(json.dumps({"a": a, "b": b}))
        return {"TQA_STACK_RESULT": str(path)}

    def test_the_named_task_carries_the_models_title_and_the_renamed_one_keeps_the_persons(self):
        a = {"id": "task-a", "title": demo_titles.TITLE, "title_source": "model"}
        b = {"id": "task-b", "title": demo_titles.PERSON_TITLE, "title_source": "person"}
        demo_titles.verify_result(self.result(a, b))
        for wrong_a, wrong_b in (
                ({**a, "title_source": "message"}, b),
                ({**a, "title": "Clean up sales.csv: remove exact duplicate rows"}, b),
                (a, {**b, "title": demo_titles.TITLE}),
                (a, {**b, "title_source": "model"}),
                (a, {**b, "id": "task-a"}),
                ({**a, "id": ""}, b)):
            with self.subTest(a=wrong_a, b=wrong_b), self.assertRaises(AssertionError):
                demo_titles.verify_result(self.result(wrong_a, wrong_b))


if __name__ == "__main__":
    unittest.main()
