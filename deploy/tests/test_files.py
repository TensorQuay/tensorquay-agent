"""WEB-E2E-021: the files scenario and its on-disk evidence."""

import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import Mock, patch

import demo_files

# This file's fixtures, found from its own location so the suite passes from any working directory.
FIXTURES = Path(__file__).resolve().parent / "fixtures"


class FilesDriverTests(unittest.TestCase):
    def test_the_scenario_writes_then_revises_the_report(self):
        scenario = json.loads((FIXTURES / "files.json").read_text())
        self.assertEqual(scenario["select"], "prompt")
        calls = [(response["when"], event["arguments"]["command"]) for response in scenario["responses"]
                 for event in response["events"] if event["type"] == "tool_call" and event.get("name") == "shell"]
        # WEB-E2E-027: the one file tool call writes by its absolute path, so the page's Open must normalise it.
        writes = [(response["when"], event["arguments"]["path"]) for response in scenario["responses"]
                  for event in response["events"] if event["type"] == "tool_call" and event.get("name") == "write_file"]
        self.assertEqual(len(writes), 1)
        self.assertTrue(writes[0][1].startswith("/workspace/"), writes[0])
        self.assertEqual(len(calls), 4)
        self.assertEqual(calls[0][0], "Summarise the attached sales file")
        self.assertIn("uploads/sales.csv", calls[0][1])
        self.assertIn("> report.md", calls[1][1])
        # The feature matrix (G2): a task of its own writes a chart and a hidden note, leaving the first task's files.
        self.assertEqual(calls[2][0], "Add a chart and a note")
        self.assertIn("> chart.png", calls[2][1])
        self.assertIn("> .notes", calls[2][1])
        # An upload the run edits: its changes list it; a later upload is never listed.
        self.assertEqual(calls[3][0], "Correct the attached figures")
        self.assertIn(">> uploads/notes.csv", calls[3][1])

    def test_scenario_mount_is_single_read_only_and_exact(self):
        mount = {"type": "bind", "source": str((FIXTURES / "files.json").resolve()),
                 "target": "/scenarios/files.json", "read_only": True}
        for changes in ({}, {"read_only": False}, {"source": "unexpected"}, {"target": "/etc/config"}):
            response = {"services": {"fake-model": {"volumes": [{**mount, **changes}]}}}
            with patch.object(demo_files.compose, "command", return_value=Mock(stdout=json.dumps(response))):
                if changes:
                    with self.assertRaises(AssertionError):
                        demo_files.verify_overlay({}, ["docker", "compose"])
                else:
                    demo_files.verify_overlay({}, ["docker", "compose"])

    def evidence(self, root, *, report=demo_files.REPORT, snapshots=2, recorded=("report.md",), refused=None):
        identity = "00000000-0000-0000-0000-000000000001"
        workspace = root / "workspaces" / identity
        (workspace / "uploads").mkdir(parents=True)
        (workspace / "uploads" / "sales.csv").write_text("region,total\n")
        (workspace / "report.md").write_text(report)
        manifests = root / "changes" / identity / "snapshots"
        manifests.mkdir(parents=True)
        for index in range(snapshots):
            (manifests / "{}.json".format(index)).write_text(
                json.dumps({"files": {path: {} for path in recorded}, "truncated": False}))
        result = root / "result.json"
        result.write_text(json.dumps({"workspaceId": identity, "upload": "region,total\n",
                                      "refused": refused or [413, "upload_too_large"]}))
        return {"TQA_STACK_RESULT": str(result), "TQA_STORAGE_ROOT": str(root)}

    def test_the_evidence_is_accepted(self):
        with tempfile.TemporaryDirectory() as temporary:
            demo_files.verify_result(self.evidence(Path(temporary)))

    def test_wrong_evidence_is_refused(self):
        for changes in ({"report": "# Draft\n"}, {"snapshots": 1}, {"recorded": (".tqa/state.json",)},
                        {"refused": [201, ""]}):
            with self.subTest(changes=changes), tempfile.TemporaryDirectory() as temporary:
                with self.assertRaises(AssertionError):
                    demo_files.verify_result(self.evidence(Path(temporary), **changes))


if __name__ == "__main__":
    unittest.main()
