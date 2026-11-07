"""WEB-E2E-032's driver: the preview files, the scripted model's command, the witness and the evidence checks."""

import base64
import io
import json
from pathlib import Path
import re
import shutil
import subprocess
import tempfile
import unittest
from unittest.mock import Mock, patch
import urllib.request
import zipfile

import demo_previews

WITNESS = "http://192.0.2.10:40123"


class PreviewFilesTests(unittest.TestCase):
    def test_every_file_is_what_its_name_says(self):
        files = demo_previews.fixtures(WITNESS)
        self.assertEqual(set(files), {"report.html", "brief.pdf", "long.pdf", "broken.pdf", "sales.xlsx",
                                      "plan.ods", "bomb.xlsx", "damaged.xlsx"})
        with zipfile.ZipFile(io.BytesIO(files["sales.xlsx"])) as book:
            self.assertIn("xl/workbook.xml", book.namelist())
            totals = book.read("xl/worksheets/1.xml").decode()
            # A formula carries its saved result, which the preview shows.
            self.assertIn("<f>SUM(B2:D2)</f><v>33</v>", totals)
        with zipfile.ZipFile(io.BytesIO(files["plan.ods"])) as sheet:
            first = sheet.infolist()[0]
            self.assertEqual((first.filename, first.compress_type), ("mimetype", zipfile.ZIP_STORED))
            self.assertIn('table:number-rows-repeated="1000000"', sheet.read("content.xml").decode())
        # The bomb claims 200 MiB for one part, in its directory, and is tiny.
        self.assertLess(len(files["bomb.xlsx"]), 1024)
        with zipfile.ZipFile(io.BytesIO(files["bomb.xlsx"])) as bomb:
            self.assertEqual(bomb.infolist()[0].file_size, 200 * 1024 * 1024)
        self.assertFalse(zipfile.is_zipfile(io.BytesIO(files["damaged.xlsx"])))

    def test_the_pdfs_have_true_offsets_and_their_pages(self):
        for name, pages, script in (("brief.pdf", 1, True), ("long.pdf", 25, False)):
            body = demo_previews.fixtures(WITNESS)[name]
            xref = int(re.search(rb"startxref\n(\d+)\n%%EOF", body).group(1))
            self.assertTrue(body[xref:].startswith(b"xref\n"), name)
            entries = re.findall(rb"(\d{10}) 00000 n ", body[xref:])
            for number, offset in enumerate(entries, start=1):
                if int(offset):
                    self.assertTrue(body[int(offset):].startswith(b"%d 0 obj" % number), (name, number))
            self.assertIn(b"/Count %d" % pages, body)
            self.assertEqual(b"/OpenAction" in body, script, name)

    def test_the_report_tries_the_witness_and_moves_the_top_only_when_framed(self):
        page = demo_previews.fixtures(WITNESS)["report.html"].decode()
        for way in ("/fetch", "/beacon", "/send-beacon", "/popup", "/form", "/top"):
            self.assertIn(WITNESS + way, page)
        self.assertIn("ws://192.0.2.10:40123/socket", page)
        self.assertIn('fetch("/api/v1/auth/me")', page)
        self.assertIn("if (window.top !== window.self)", page)


class ScenarioTests(unittest.TestCase):
    def test_one_shell_call_writes_every_file_then_an_answer(self):
        scenario = demo_previews.scenario(WITNESS)
        self.assertEqual(scenario["select"], "prompt")
        responses = scenario["responses"]
        self.assertEqual([response["when"] for response in responses], [demo_previews.PROMPT] * 2)
        calls = [event for event in responses[0]["events"] if event["type"] == "tool_call"]
        self.assertEqual([call["name"] for call in calls], ["shell"])
        command = calls[0]["arguments"]["command"]
        self.assertLess(len(command.encode()), demo_previews.COMMAND_LIMIT)
        for name, content in demo_previews.fixtures(WITNESS).items():
            self.assertIn("printf %s '{}' | base64 -d > previews/{}".format(base64.b64encode(content).decode(), name),
                          command)
        for name, size in demo_previews.LARGE.items():
            self.assertIn("head -c {} /dev/zero".format(size), command)
            self.assertIn("> previews/" + name, command)
        self.assertEqual(responses[1]["events"][0]["type"], "text")

    def test_a_command_past_the_argument_limit_is_refused(self):
        with patch.object(demo_previews, "COMMAND_LIMIT", 100), self.assertRaises(ValueError):
            demo_previews.commands(WITNESS)


class WitnessTests(unittest.TestCase):
    def test_it_records_every_request_but_its_own_log(self):
        witness = demo_previews.Witness("127.0.0.1").start()
        try:
            for path in ("/control", "/fetch?x=1", "/log"):
                with urllib.request.urlopen(witness.origin + path, timeout=5) as answer:
                    self.assertEqual(answer.status, 200)
            request = urllib.request.Request(witness.origin + "/form", data=b"field=1", method="POST")
            urllib.request.urlopen(request, timeout=5).close()
            with urllib.request.urlopen(witness.origin + "/log", timeout=5) as answer:
                self.assertEqual(json.loads(answer.read()), ["/control", "/fetch", "/form"])
        finally:
            witness.stop()


class ConfigureTests(unittest.TestCase):
    def test_the_scenario_is_written_then_handed_to_the_containers_user(self):
        # The containers run as 1000:1000 and the file is private (0600), so it must be theirs once written.
        calls = []
        with tempfile.TemporaryDirectory() as temporary, \
                patch.object(demo_previews.compose, "ownership",
                             side_effect=lambda directory, env, owner=None: calls.append(("own", owner))), \
                patch.object(demo_previews.compose, "write",
                             side_effect=lambda path, content: calls.append(("write", path.name))):
            env = {}
            demo_previews.configure(temporary, env, Mock(origin=WITNESS))
            self.assertEqual(calls, [("own", None), ("write", "previews.json"), ("own", "1000:1000")])
            self.assertEqual(env["TQA_WITNESS_URL"], WITNESS)
            self.assertTrue(env["TQA_PREVIEWS_SCENARIO"].endswith("/previews.json"))


class EvidenceTests(unittest.TestCase):
    def test_the_scenario_mount_is_single_read_only_and_exact(self):
        env = {"TQA_PREVIEWS_SCENARIO": "/tmp/run/previews.json"}
        mount = {"type": "bind", "source": env["TQA_PREVIEWS_SCENARIO"], "target": "/scenarios/previews.json",
                 "read_only": True}
        for changes in ({}, {"read_only": False}, {"source": "/elsewhere.json"}, {"target": "/etc/config"}):
            response = {"services": {"fake-model": {"volumes": [{**mount, **changes}]}}}
            with patch.object(demo_previews.compose, "command", return_value=Mock(stdout=json.dumps(response))):
                if changes:
                    with self.assertRaises(AssertionError):
                        demo_previews.verify_overlay(env, ["docker", "compose"])
                else:
                    demo_previews.verify_overlay(env, ["docker", "compose"])

    def evidence(self, root, missing=None):
        identity = "00000000-0000-0000-0000-000000000001"
        folder = root / "workspaces" / identity / demo_previews.FOLDER
        folder.mkdir(parents=True)
        for name in list(demo_previews.fixtures(WITNESS)) + list(demo_previews.LARGE):
            if name != missing:
                (folder / name).write_bytes(b"x")
        result = root / "result.json"
        result.write_text(json.dumps({"workspaceId": identity}))
        return {"TQA_STACK_RESULT": str(result), "TQA_STORAGE_ROOT": str(root)}

    def witness(self, seen):
        witness = Mock(origin=WITNESS)
        witness.seen.return_value = seen
        return witness

    def test_the_evidence_is_accepted(self):
        with tempfile.TemporaryDirectory() as temporary:
            demo_previews.verify_result(self.evidence(Path(temporary)), self.witness(["/control"]))

    @unittest.skipUnless(Path("/sbin/mke2fs").exists() and Path("/sbin/debugfs").exists(), "needs e2fsprogs")
    def test_a_microvm_workspace_image_is_read_without_mounting(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            env = self.evidence(root)
            identity = json.loads(Path(env["TQA_STACK_RESULT"]).read_text())["workspaceId"]
            workspaces = root / "workspaces"
            source = workspaces / identity
            image = workspaces / (identity + ".ext4")
            subprocess.run(["/sbin/mke2fs", "-q", "-t", "ext4", "-d", str(source), str(image), "4M"], check=True)
            shutil.rmtree(source)
            demo_previews.verify_result(env, self.witness(["/control"]))
            self.assertIn("plan.ods", demo_previews.workspace_files(workspaces, identity))

    def test_a_missing_file_or_anything_the_witness_saw_is_refused(self):
        for missing, seen in (("plan.ods", ["/control"]), (None, ["/control", "/fetch"]), (None, [])):
            with self.subTest(missing=missing, seen=seen), tempfile.TemporaryDirectory() as temporary:
                with self.assertRaises(AssertionError):
                    demo_previews.verify_result(self.evidence(Path(temporary), missing), self.witness(seen))


if __name__ == "__main__":
    unittest.main()


class MicrovmTests(unittest.TestCase):
    # TEST: TK-U-044
    def test_the_vm_host_is_an_image_under_test(self):
        """Candidate 14b: the previews demo on the microVM backend failed its sandbox image guard, because the VM host
        image its sandboxes run was never counted under test, as the microVM stacks count it."""
        microvm = demo_previews.firecracker_stack()
        runtime, vmm = "sha256:" + "a" * 64, "sha256:" + "c" * 64
        env = {"TQA_VMM_IMAGE": "tqa-vmm:candidate", "TQA_FC_KERNEL_DIR": "/kernel",
               "TQA_FC_GATEWAY_ADDRESS": "192.0.2.1", "TQA_PINNED_IMAGES": runtime}
        with tempfile.TemporaryDirectory() as directory:
            (Path(directory) / "gateway.toml").write_text("backend='docker'\n")
            with patch.object(demo_previews, "firecracker_stack", return_value=microvm), \
                    patch.object(microvm, "image_id", return_value=vmm), \
                    patch.object(microvm, "firecracker_config", return_value="[runtime]\n"), \
                    patch.object(microvm, "artefacts"), \
                    patch.object(demo_previews.compose, "ownership"):
                demo_previews.to_microvm(directory, env, runtime)
        self.assertEqual(env["TQA_PINNED_IMAGES"], ",".join(sorted([runtime, vmm])))
