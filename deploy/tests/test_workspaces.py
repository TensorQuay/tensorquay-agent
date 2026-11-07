"""WEB-E2E-011: the workspaces scenario and its on-disk evidence."""

import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import Mock, patch

import demo_workspaces

# This file's fixtures, found from its own location so the suite passes from any working directory.
FIXTURES = Path(__file__).resolve().parent / "fixtures"


class WorkspacesDriverTests(unittest.TestCase):
    def test_each_task_follows_its_own_script_and_the_first_is_held_until_stop(self):
        scenario = json.loads((FIXTURES / "workspaces.json").read_text())
        self.assertEqual(scenario["select"], "prompt")
        by_task = {}
        for response in scenario["responses"]:
            by_task.setdefault(response["when"], []).append(response)
        self.assertEqual(set(by_task), set(demo_workspaces.TASKS.values()))
        # The first task holds the workspace's lease in a held model call (ADR-0035: no approval to wait on); Stop
        # ends it before its mark is written, so the shared workspace ends with the second task's mark only.
        first = by_task[demo_workspaces.TASKS["first"]]
        self.assertEqual([response.get("wait_ms") for response in first], [600000])
        for name in ("fork", "second"):
            replies = by_task[demo_workspaces.TASKS[name]]
            self.assertTrue(all("wait_ms" not in response for response in replies))
            calls = [event["arguments"]["command"] for response in replies for event in response["events"]
                     if event["type"] == "tool_call"]
            self.assertEqual(calls, ["printf {} >> runs.txt".format(demo_workspaces.MARK)])

    def test_scenario_mount_is_single_read_only_and_exact(self):
        mount = {"type": "bind", "source": str((FIXTURES / "workspaces.json").resolve()),
                 "target": "/scenarios/workspaces.json", "read_only": True}
        for changes in ({}, {"read_only": False}, {"source": "unexpected"}, {"target": "/etc/config"}):
            response = {"services": {"fake-model": {"volumes": [{**mount, **changes}]}}}
            with patch.object(demo_workspaces.compose, "command", return_value=Mock(stdout=json.dumps(response))):
                if changes:
                    with self.assertRaises(AssertionError):
                        demo_workspaces.verify_overlay({}, ["docker", "compose"])
                else:
                    demo_workspaces.verify_overlay({}, ["docker", "compose"])

    def evidence(self, root, *, shared=1, fork=1, same=False, order=("first", "fork", "second")):
        workspace, copy = "00000000-0000-0000-0000-000000000001", "00000000-0000-0000-0000-000000000002"
        if same:
            copy = workspace
        for identity, count in ((workspace, shared), (copy, fork)):
            directory = root / "workspaces" / identity
            directory.mkdir(parents=True, exist_ok=True)
            (directory / "runs.txt").write_text(demo_workspaces.MARK * count)
        result = root / "result.json"
        result.write_text(json.dumps({"workspace": workspace, "fork": copy, "order": list(order)}))
        return {"TQA_STACK_RESULT": str(result), "TQA_STORAGE_ROOT": str(root)}

    def test_the_evidence_is_accepted(self):
        with tempfile.TemporaryDirectory() as temporary:
            demo_workspaces.verify_result(self.evidence(Path(temporary).resolve()))

    def test_wrong_evidence_is_refused(self):
        for changes in ({"shared": 2}, {"shared": 0}, {"fork": 2}, {"fork": 0}, {"same": True},
                        {"order": ("first", "second", "fork")}):
            with self.subTest(changes=changes), tempfile.TemporaryDirectory() as temporary:
                with self.assertRaises(AssertionError):
                    demo_workspaces.verify_result(self.evidence(Path(temporary).resolve(), **changes))


if __name__ == "__main__":
    unittest.main()
