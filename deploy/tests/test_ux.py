"""WEB-E2E-018: LAN origin, long scenario and bounded UX browser execution."""

import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import Mock, patch

import compose
import demo
import demo_ux


class UxDriverTests(unittest.TestCase):
    def test_lan_host_is_a_valid_non_loopback_ipv4_address(self):
        for address in ("127.0.0.1", "0.0.0.0", "::1", "224.0.0.1", "not-a-host", "example.invalid"):
            with self.subTest(address=address), self.assertRaises(ValueError):
                demo_ux.lan_host({"TQA_UX_HOST": address})
        self.assertEqual(demo_ux.lan_host({"TQA_UX_HOST": "192.0.2.10"}), "192.0.2.10")
        with patch.object(compose, "command", return_value=Mock(stdout='"192.0.2.11"')):
            self.assertEqual(demo_ux.lan_host({}), "192.0.2.11")
        with patch.object(compose, "command", return_value=Mock(stdout='null')):
            with self.assertRaises(ValueError):
                demo_ux.lan_host({})

    def test_public_origin_matches_the_explicit_lan_binding(self):
        with tempfile.TemporaryDirectory() as temporary:
            directory, env = Path(temporary), {"TQA_WORKSTATION_IMAGE": "sha256:" + "b" * 64, "TQA_GATEWAY_IMAGE": "gw",
                                               "TQA_WEB_IMAGE": "web"}
            with patch.object(compose, "command"), patch.object(compose, "ownership"), \
                    patch.object(compose, "engine_subnets", return_value=[]), \
                    patch.object(compose, "ensure_image"), patch.object(compose, "gateway_takes_pins", return_value=True):  # a unit gate never inspects or pulls an image
                compose.prepare(directory, env, runtime="sha256:" + "a" * 64,
                                port=12345, host="192.0.2.10")
            self.assertIn("public_origin='http://192.0.2.10:12345'", (directory / "gateway.toml").read_text())

    def test_browser_suite_selection_and_timeout_are_explicit(self):
        for suite, expected_timeout in (("basic", 600), ("ux", 600), ("environment", 600), ("queue", 600), ("egress", 600)):
            env = {"TQA_STACK_SUITE": suite}
            with patch.object(compose, "command", return_value=Mock(stdout="browser passed\n")) as command:
                demo.browser(env)
            self.assertEqual(command.call_args[1]["timeout"], expected_timeout)
            self.assertEqual(command.call_args[0][1]["TQA_STACK_SUITE"], suite)

    def test_workspace_evidence_requires_unique_valid_ids_and_actual_corrected_files(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            result = root / "browser.json"
            identity = "00000000-0000-0000-0000-000000000001"
            env = {"TQA_STACK_RESULT": str(result), "TQA_STORAGE_ROOT": str(root)}
            result.write_text(json.dumps({"conversations": [identity]}))
            with self.assertRaises(AssertionError):
                demo_ux.verify_workspaces(env)
            folder = root / "workspaces" / identity / "demo"
            folder.mkdir(parents=True)
            (folder / "arithmetic.py").write_text("def add(left, right):\n    return left + right\n")
            (folder / "README.md").write_text("# Arithmetic demo\nVerified: two arithmetic tests pass.\n")
            demo_ux.verify_workspaces(env)
            result.write_text(json.dumps({"conversations": [identity, identity]}))
            with self.assertRaises(AssertionError):
                demo_ux.verify_workspaces(env)
            result.write_text(json.dumps({"conversations": [identity, identity.replace("-", "")]}))
            with self.assertRaises(AssertionError):
                demo_ux.verify_workspaces(env)
            result.write_text(json.dumps({"conversations": ["../outside"]}))
            with self.assertRaises(ValueError):
                demo_ux.verify_workspaces(env)

    def test_evidence_cannot_follow_a_workspace_symlink_outside_storage(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            identity = "00000000-0000-0000-0000-000000000001"
            storage = root / "storage"
            (storage / "workspaces").mkdir(parents=True)
            outside = root / "outside"
            (outside / "demo").mkdir(parents=True)
            (outside / "demo/arithmetic.py").write_text("def add(left, right):\n    return left + right\n")
            (outside / "demo/README.md").write_text("# Arithmetic demo\nVerified: two arithmetic tests pass.\n")
            (storage / "workspaces" / identity).symlink_to(outside, target_is_directory=True)
            result = root / "browser.json"
            result.write_text(json.dumps({"conversations": [identity]}))
            with self.assertRaises(AssertionError):
                demo_ux.verify_workspaces({"TQA_STACK_RESULT": str(result), "TQA_STORAGE_ROOT": str(storage)})


if __name__ == "__main__":
    unittest.main()
