"""E2E-EXT-015's fixtures and checks without a stack. Run: python3 -m unittest tests/e2e/test_gate6.py"""

import copy
import sys
from pathlib import Path
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parent))
import gate6_evidence as evidence  # noqa: E402

BASE = ("[[connectors]]\nname = \"tracker\"\nurl = \"https://192.0.2.9:8443/mcp\"\n"
        "enabled_by_default = true\nca_file = \"/srv/ca.pem\"\n[connectors.auth]\nscheme = \"header\"\n"
        "header = \"X-Api-Key\"\nrequired = true\n\n[[connectors]]\nname = \"elsewhere\"\n"
        "url = \"https://192.0.2.9:8443/mcp\"\nenabled_by_default = false\n"
        "ca_file = \"/srv/ca.pem\"\n[connectors.auth]\nscheme = \"header\"\nheader = \"X-Api-Key\"\nrequired = true\n")


def connector(key_missing):
    return {"name": "tracker", "secret_ref": "secrets/tracker-api-key", "key_missing": key_missing,
            "enabled_by_default": False}


SECRETS = {"items": [{"ref": "secrets/tracker-api-key", "consumers": [{"kind": "connector", "name": "tracker"}]}],
           "limit": 100}


class Seed(unittest.TestCase):
    def test_tracker_is_off_by_default_and_names_the_shared_secret(self):
        text = evidence.seeded(BASE)
        self.assertNotIn("enabled_by_default = true", text)
        self.assertEqual(text.count('secret_ref = "secrets/tracker-api-key"'), 1)
        self.assertLess(text.index("secret_ref"), text.index("elsewhere"))

    def test_a_seed_that_is_not_the_fixtures_is_refused(self):
        with self.assertRaises(ValueError):
            evidence.seeded(BASE.replace("enabled_by_default = true", "enabled_by_default = false"))


class Checks(unittest.TestCase):
    def test_registration(self):
        before, after = {"items": [connector(True)]}, {"items": [connector(False)]}
        self.assertEqual(evidence.check_registration(before, after, SECRETS)["secret"], "secrets/tracker-api-key")
        # A connector still carrying a class is refused: the class was removed (ADR-0035).
        classed = {"items": [dict(connector(True), **{"class": "network"})]}
        for broken in ({"items": [connector(False)]}, {"items": []}, classed):
            with self.subTest(broken), self.assertRaises(AssertionError):
                evidence.check_registration(broken, after, SECRETS)
        shared = copy.deepcopy(SECRETS)
        shared["items"][0]["consumers"].append({"kind": "route", "name": "local"})
        with self.assertRaises(AssertionError):
            evidence.check_registration(before, after, shared)

    def test_member(self):
        own = {"items": [{"name": "tracker", "has_credential": False}]}
        control = {"connectors": [], "profile": None}
        task = {"connectors": ["tracker"], "profile": {"name": "issue-filing", "sets": ["connectors", "tools"]}}
        evidence.check_member(own, control, task)
        for changed in ({"own": {"items": [{"name": "tracker", "has_credential": True}]}},
                        {"control": {"connectors": ["tracker"]}}, {"task": dict(task, connectors=[])}):
            arguments = {"own": own, "control": control, "task": task, **changed}
            with self.subTest(changed), self.assertRaises(AssertionError):
                evidence.check_member(**arguments)

    def test_reads(self):
        read = {"consumer_kind": "connector", "consumer": "tracker", "conversation_id": "c1", "outcome": "ok",
                "count": 2}
        self.assertEqual(evidence.check_reads({"items": [read]}, "c1"), 2)
        for page in ({"items": []}, {"items": [dict(read, conversation_id="c2")]},
                     {"items": [dict(read, outcome="absent")]}):
            with self.subTest(page), self.assertRaises(AssertionError):
                evidence.check_reads(page, "c1")

    def test_the_profile_brings_only_the_connector(self):
        self.assertEqual(evidence.profile()["connectors"], ["tracker"])
        self.assertEqual(evidence.profile()["mcp_servers"], [])


if __name__ == "__main__":
    unittest.main()
