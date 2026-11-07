"""E2E-ENV-010, E2E-ENV-012 and E2E-PROFILE-018: template pins, the shared scenario and retained workspace evidence without a daemon."""

import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import Mock, patch

import demo_environments

# This file's fixtures, found from its own location so the suite passes from any working directory.
FIXTURES = Path(__file__).resolve().parent / "fixtures"


class EnvironmentDriverTests(unittest.TestCase):
    def test_scenario_file_writes_supply_the_required_parent_policy(self):
        scenario = json.loads((FIXTURES / "environment.json").read_text())
        writes = [event["arguments"] for response in scenario["responses"]
                  for event in response["events"] if event.get("name") == "write_file"]
        # E2E-ENV-010's three, and E2E-PROFILE-018's one, which its profile refuses.
        self.assertEqual(len(writes), 4)
        for arguments in writes:
            with self.subTest(path=arguments["path"]):
                self.assertEqual(set(arguments), {"path", "content", "create_parents"})
                self.assertIsInstance(arguments["create_parents"], bool)

    def test_each_task_gets_only_its_own_replies(self):
        """E2E-ENV-010, E2E-ENV-012 and E2E-PROFILE-018 share one model: each reply names its task's first message (select: prompt)."""
        scenario = json.loads((FIXTURES / "environment.json").read_text())
        self.assertEqual(scenario["select"], "prompt")
        phrases = [response["when"] for response in scenario["responses"]]
        self.assertEqual(phrases, ["Create a small test file"] * 6 + ["Run one command in this environment"] * 2
                         + ["Try to save a note in this workspace"] * 2)
        commands = [event["arguments"]["command"] for response in scenario["responses"][6:]
                    for event in response["events"] if event.get("name") == "shell"]
        self.assertEqual(commands, ["uname -s && python3 --version"])

    def test_scenario_mount_is_read_only_and_cannot_expose_another_path(self):
        mount = {"type": "bind", "source": str((FIXTURES / "environment.json").resolve()), "target": "/scenarios/environment.json", "read_only": True}
        for changed, valid in (({}, True), ({"read_only": False}, False), ({"target": "/etc/config"}, False), ({"source": "unexpected"}, False)):
            config = {"services": {"fake-model": {"volumes": [{**mount, **changed}]}}}
            with patch.object(demo_environments.compose, "command", return_value=Mock(stdout=json.dumps(config))):
                if valid:
                    demo_environments.verify_overlay({}, ["docker", "compose"])
                else:
                    with self.assertRaises(AssertionError):
                        demo_environments.verify_overlay({}, ["docker", "compose"])

    def test_templates_have_distinct_validated_pins_and_declared_capabilities(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            config = root / "gateway.toml"
            config.write_text("[runtime]\nbackend='docker'\n")
            pins = {"minimal": "sha256:" + "a" * 64, "workstation": "sha256:" + "b" * 64}
            with patch.object(demo_environments.compose, "ownership"):
                demo_environments.configure(root, {}, pins)
            text = config.read_text()
            self.assertIn('name="workstation"', text)
            self.assertIn('display_name="Workstation"', text)
            self.assertIn('capabilities=["shell", "python", "node", "build", "browser", "data"]', text)
            self.assertIn('capabilities=["shell"]', text)
            self.assertEqual(text.count('default=true'), 1)
            for pin in pins.values():
                self.assertIn(pin, text)
            self.assertEqual(config.stat().st_mode & 0o777, 0o600)

    def test_tags_and_missing_images_cannot_be_written_into_demo_templates(self):
        for pins in ({"minimal": "tqa-runtime:base", "workstation": "sha256:" + "b" * 64}, {}):
            with self.subTest(pins=pins), self.assertRaises(ValueError):
                demo_environments.configure(Path("unused"), {}, pins)

    def test_evidence_requires_the_original_file_and_png_output(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            identity = "00000000-0000-0000-0000-000000000001"
            result = root / "browser.json"
            result.write_text(json.dumps({"conversationId": identity}))
            env = {"TQA_STACK_RESULT": str(result), "TQA_STORAGE_ROOT": str(root)}
            folder = root / "workspaces" / identity / "environment"
            folder.mkdir(parents=True)
            target = folder / "test_persisted.py"
            target.write_text(demo_environments.TEST_FILE)
            with self.assertRaises(AssertionError):
                demo_environments.verify_workspace(env)
            (folder / "page.png").write_bytes(b"\x89PNG\r\n\x1a\n" + b"fixture" * 100)
            demo_environments.verify_workspace(env)
            target.write_text("replacement")
            with self.assertRaises(AssertionError):
                demo_environments.verify_workspace(env)

    def test_workspace_evidence_rejects_a_symlink_outside_storage(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            identity = "00000000-0000-0000-0000-000000000001"
            result = root / "browser.json"
            result.write_text(json.dumps({"conversationId": identity}))
            (root / "workspaces").mkdir()
            outside = root / "outside"
            outside.mkdir()
            (root / "workspaces" / identity).symlink_to(outside, target_is_directory=True)
            with self.assertRaises(AssertionError):
                demo_environments.verify_workspace({"TQA_STACK_RESULT": str(result), "TQA_STORAGE_ROOT": str(root)})


if __name__ == "__main__":
    unittest.main()
