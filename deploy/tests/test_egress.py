"""SEC-001 (web side): a real denied fetch through the gateway's policy proxy."""

import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import Mock, patch

import demo_egress

# This file's fixtures, found from its own location so the suite passes from any working directory.
FIXTURES = Path(__file__).resolve().parent / "fixtures"


class EgressDriverTests(unittest.TestCase):
    def test_configuration_allows_one_unrelated_host_and_keeps_the_file_private(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            config = root / "gateway.toml"
            config.write_text("[sandbox]\nmax_live=4\n")
            with patch.object(demo_egress.compose, "ownership"):
                demo_egress.configure(root, {})
            body = config.read_text()
            self.assertIn("[egress]\nlisten=\"0.0.0.0:3128\"\n", body)
            self.assertIn("[[egress.allow]]\nhost=\"allowed.example\"\nports=[443]\n", body)
            self.assertNotIn("example.invalid", body)
            self.assertEqual(config.stat().st_mode & 0o777, 0o600)

    def test_scenario_fetches_only_the_unlisted_host_then_answers(self):
        responses = json.loads((FIXTURES / "egress.json").read_text())["responses"]
        self.assertEqual(len(responses), 2)
        call = responses[0]["events"]
        # The scripted model requires split_into on every tool call; it refuses to start otherwise.
        self.assertEqual(call, [{"type": "tool_call", "name": "fetch_url",
                                 "arguments": {"url": "https://example.invalid/data"}, "split_into": 2}])
        self.assertEqual(responses[1]["events"][0]["type"], "text")

    def test_scenario_mount_must_be_single_exact_and_read_only(self):
        mount = {"type": "bind", "source": str((FIXTURES / "egress.json").resolve()),
                 "target": "/scenarios/egress.json", "read_only": True}
        for changes in ({}, {"read_only": False}, {"source": "unexpected"}, {"target": "/etc/config"}):
            response = {"services": {"fake-model": {"volumes": [{**mount, **changes}]}}}
            with patch.object(demo_egress.compose, "command", return_value=Mock(stdout=json.dumps(response))):
                if changes:
                    with self.assertRaises(AssertionError):
                        demo_egress.verify_overlay({}, ["docker", "compose"])
                else:
                    demo_egress.verify_overlay({}, ["docker", "compose"])


if __name__ == "__main__":
    unittest.main()
