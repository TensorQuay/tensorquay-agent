"""WEB-E2E-126: the PDF scenario, the vision route and the on-disk evidence."""

import hashlib
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import Mock, patch

import demo_pdf

# This file's fixtures, found from its own location so the suite passes from any working directory.
FIXTURES = Path(__file__).resolve().parent / "fixtures"
LOCAL = ("[[routes]]\nname='local'\nprotocol='openai_chat'\nbase_url='http://fake-model:8080/v1'\n"
         "model='fixture'\nauth={kind='bearer'}\nkey_ref='secrets/local'\n")


class PdfDriverTests(unittest.TestCase):
    def test_the_scenario_reads_each_attached_file_by_its_phrase(self):
        scenario = json.loads((FIXTURES / "pdf.json").read_text())
        self.assertEqual(scenario["select"], "prompt")
        reads = [(response["when"], event["arguments"]) for response in scenario["responses"]
                 for event in response["events"] if event["type"] == "tool_call"]
        self.assertTrue(all(event.get("name") == "read_file" for response in scenario["responses"]
                            for event in response["events"] if event["type"] == "tool_call"))
        # Every argument the tool's strict schema names, the optional ones null (TOOL-U-032).
        for _, arguments in reads:
            self.assertEqual(set(arguments), {"path", "start_line", "end_line", "cursor"})
            self.assertEqual((arguments["start_line"], arguments["end_line"], arguments["cursor"]), (None, None, None))
        self.assertEqual([(when, arguments["path"]) for when, arguments in reads], [
            ("Read the attached report", "uploads/report.pdf"),
            ("Look at the attached report's pages", "uploads/report.pdf"),
            ("Read the attached files that can't be read", "uploads/locked.pdf"),
            ("Read the attached files that can't be read", "uploads/broken.pdf"),
            ("Read the attached files that can't be read", "uploads/nested.pdf"),
        ])
        # Each case ends in words, so the run reads Done and the page shows an answer.
        for when in dict.fromkeys(when for when, _ in reads):
            last = [response for response in scenario["responses"] if response["when"] == when][-1]
            self.assertEqual(last["events"][0]["type"], "text")

    def test_configure_adds_a_vision_route_to_the_same_model(self):
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary)
            (directory / "models.toml").write_text(LOCAL)
            with patch.object(demo_pdf.compose, "ownership"):
                demo_pdf.configure(directory, {})
            text = (directory / "models.toml").read_text()
        self.assertEqual(text.count("[[routes]]"), 2)
        local, vision = text.split("\n\n")
        self.assertEqual(local, LOCAL.rstrip("\n"))
        self.assertIn("name='vision'\n", vision)
        # Its own name in the picker: two routes reading alike are numbered there ("Fixture (1)"), and the shared
        # sign-in check then finds no catalogue entry of that label (the candidate run of 15 Nov).
        self.assertIn("display_name='Vision fixture'\n", vision)
        self.assertNotIn("display_name", local)
        self.assertTrue(vision.endswith("vision=true\n"), vision)
        # The same scripted model answers both routes; only the local one is marked as reading pictures.
        self.assertNotIn("vision=true", local)
        self.assertEqual(vision.count("model='fixture'"), 1)

    def test_configure_refuses_a_models_file_without_the_local_route(self):
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary)
            (directory / "models.toml").write_text("[[routes]]\nname='other'\n")
            with patch.object(demo_pdf.compose, "ownership"), self.assertRaisesRegex(ValueError, "local route"):
                demo_pdf.configure(directory, {})

    def test_scenario_mount_is_single_read_only_and_exact(self):
        mount = {"type": "bind", "source": str((FIXTURES / "pdf.json").resolve()),
                 "target": "/scenarios/pdf.json", "read_only": True}
        for changes in ({}, {"read_only": False}, {"source": "unexpected"}, {"target": "/etc/config"}):
            response = {"services": {"fake-model": {"volumes": [{**mount, **changes}]}}}
            with patch.object(demo_pdf.compose, "command", return_value=Mock(stdout=json.dumps(response))):
                if changes:
                    with self.assertRaises(AssertionError):
                        demo_pdf.verify_overlay({}, ["docker", "compose"])
                else:
                    demo_pdf.verify_overlay({}, ["docker", "compose"])

    def evidence(self, root, files, *, extra=None, change=None):
        """Two tasks' workspaces holding `files` (name to bytes) as uploads: the report twice (two routes), the three
        unreadable ones once; `extra` writes one more file, `change` alters one upload after its digest was taken."""
        record = {"tasks": []}
        groups = [("00000000-0000-0000-0000-000000000001", {"report.pdf": files["report.pdf"]}),
                  ("00000000-0000-0000-0000-000000000002", {"report.pdf": files["report.pdf"]}),
                  ("00000000-0000-0000-0000-000000000003", {name: files[name] for name in ("locked.pdf", "broken.pdf", "nested.pdf")})]
        for identity, uploads in groups:
            workspace = root / "workspaces" / identity
            (workspace / "uploads").mkdir(parents=True)
            (workspace / ".tqa").mkdir()
            (workspace / ".tqa" / "state.json").write_text("{}")
            digests = {}
            for name, data in uploads.items():
                (workspace / "uploads" / name).write_bytes(data)
                digests[name] = hashlib.sha256(data).hexdigest()
            record["tasks"].append({"workspaceId": identity, "uploads": digests})
        if extra:
            (root / "workspaces" / groups[0][0] / extra).write_text("written by the agent\n")
        if change:
            (root / "workspaces" / groups[0][0] / "uploads" / change).write_bytes(b"%PDF-1.4 changed")
        result = root / "browser.json"
        result.write_text(json.dumps(record))
        return {"TQA_STACK_RESULT": str(result), "TQA_STORAGE_ROOT": str(root)}

    FILES = {"report.pdf": b"%PDF-1.4 report", "locked.pdf": b"%PDF-1.4 locked", "broken.pdf": b"%PDF-1.7\n",
             "nested.pdf": b"%PDF-1.4 nested"}

    def test_uploads_stored_unchanged_and_nothing_else_written_pass(self):
        with tempfile.TemporaryDirectory() as temporary:
            demo_pdf.verify_result(self.evidence(Path(temporary), self.FILES))

    def test_a_changed_upload_fails(self):
        with tempfile.TemporaryDirectory() as temporary:
            with self.assertRaisesRegex(AssertionError, "differs from what was sent"):
                demo_pdf.verify_result(self.evidence(Path(temporary), self.FILES, change="report.pdf"))

    def test_a_file_the_agent_wrote_fails(self):
        with tempfile.TemporaryDirectory() as temporary:
            with self.assertRaisesRegex(AssertionError, "more than the uploads"):
                demo_pdf.verify_result(self.evidence(Path(temporary), self.FILES, extra="notes.txt"))

    def test_a_missing_upload_fails(self):
        with tempfile.TemporaryDirectory() as temporary:
            env = self.evidence(Path(temporary), self.FILES)
            (Path(temporary) / "workspaces" / "00000000-0000-0000-0000-000000000003" / "uploads" / "nested.pdf").unlink()
            with self.assertRaisesRegex(AssertionError, "not in the workspace"):
                demo_pdf.verify_result(env)


if __name__ == "__main__":
    unittest.main()
