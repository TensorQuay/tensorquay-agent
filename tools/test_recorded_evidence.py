"""Recorded gate evidence: a soak result or a TEST-PLAN §15.1 result file stands in for running its ID, checked and
labelled with its SHA-256, never trusted on its name alone."""
import hashlib
import json
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import recorded_evidence  # noqa: E402  (beside this file)
from recorded_evidence import describe, parse  # noqa: E402

ENVIRONMENT = {"utc": "2026-10-01T12:00:00Z", "kernel": "Linux 7.0.0", "cpu": "cpu", "commit": "a" * 40,
               "dirty_tree": False}


def measured(identifier="PERF-020", verdict="pass", **environment):
    return {"id": identifier, "environment": dict(ENVIRONMENT, **environment), "verdict": verdict}


class Files(unittest.TestCase):
    def setUp(self):
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        self.root = Path(directory.name)

    def file(self, body, name="result.json"):
        path = self.root / name
        path.write_text(body if isinstance(body, str) else json.dumps(body))
        return path

    def entry(self, identifier, path, pin=None):
        return parse(identifier + "=" + str(path) + ("" if pin is None else "@" + pin))


class Inputs(Files):
    def test_an_entry_is_an_id_a_path_and_optionally_the_files_sha256(self):
        identifier, artifact = parse("PERF-021=results/x.json")
        self.assertEqual((identifier, artifact.path, artifact.sha256), ("PERF-021", Path("results/x.json"), None))
        _, pinned = parse("SOAK-001=r.json@" + "ab" * 32)
        self.assertEqual(pinned.sha256, "ab" * 32)

    def test_malformed_entries_are_refused(self):
        for entry in ("PERF-021", "PERF-021=", "=x.json", "perf-021=x.json", "PERF-021=x.json@notahash"):
            with self.subTest(entry=entry), self.assertRaises(ValueError):
                parse(entry)


class Soak(Files):
    def test_any_soak_id_takes_a_passed_gate_evidence_soak_result(self):
        path = self.file({"status": "passed", "gate_evidence": True, "chaos_seconds": 86400})
        for identifier in ("SOAK-001", "SOAK-002", "SOAK-003", "SOAK-004"):
            description = describe(*self.entry(identifier, path), gate="9")
            self.assertIn("sha256=" + hashlib.sha256(path.read_bytes()).hexdigest(), description)
            self.assertIn("chaos_seconds=86400", description)

    def test_the_soak_rules_are_kept(self):
        for body, gate in (({"status": "failed", "gate_evidence": True, "chaos_seconds": 86400}, "9"),
                           ({"status": "passed", "gate_evidence": False, "chaos_seconds": 86400}, "9"),
                           ({"status": "passed", "gate_evidence": True, "chaos_seconds": 7200}, "9")):
            with self.subTest(body=body), self.assertRaises(ValueError):
                describe(*self.entry("SOAK-001", self.file(body)), gate=gate)
        short = self.file({"status": "passed", "gate_evidence": True, "chaos_seconds": 7200})
        self.assertIn("chaos_seconds=7200", describe(*self.entry("SOAK-002", short), gate="2"))


class Measured(Files):
    def test_a_passing_result_for_the_id_is_taken_and_labelled(self):
        path = self.file(measured())
        description = describe(*self.entry("PERF-020", path), gate="9")
        self.assertIn("sha256=" + hashlib.sha256(path.read_bytes()).hexdigest(), description)
        self.assertIn("id=PERF-020 verdict=pass commit=" + "a" * 12, description)

    def test_a_result_of_one_part_of_the_id_counts_for_it(self):
        path = self.file(measured("PERF-011-new-task"))
        self.assertIn("id=PERF-011-new-task", describe(*self.entry("PERF-011", path), gate="9"))
        with self.assertRaises(ValueError):
            describe(*self.entry("PERF-01", path), gate="9")

    def test_each_refusal_says_why(self):
        cases = {
            "another measurement": (measured("PERF-010"), "is PERF-010, not PERF-020"),
            "failed": (measured(verdict="fail"), "verdict is fail, not pass"),
            "no verdict": ({"id": "PERF-020", "environment": ENVIRONMENT}, "verdict is None, not pass"),
            "no environment": ({"id": "PERF-020", "verdict": "pass"}, "has no environment"),
            "short environment": (measured(commit=None) | {"environment": {"utc": "x"}}, "environment lacks kernel"),
            "dirty tree": (measured(dirty_tree=True), "measured on a dirty tree"),
            "not an object": ([], "is not a result object"),
        }
        for name, (body, reason) in cases.items():
            with self.subTest(name), self.assertRaisesRegex(ValueError, reason):
                describe(*self.entry("PERF-020", self.file(body)), gate="9")

    def test_a_pinned_sha256_must_match_the_file(self):
        path = self.file(measured())
        digest = hashlib.sha256(path.read_bytes()).hexdigest()
        self.assertIn(digest, describe(*self.entry("PERF-020", path, digest), gate="9"))
        with self.assertRaisesRegex(ValueError, "sha256 differs from the one given"):
            describe(*self.entry("PERF-020", path, "0" * 64), gate="9")

    def test_missing_oversized_duplicate_field_and_non_json_files_are_refused(self):
        missing = self.root / "absent.json"
        cases = [missing, self.file(" " * (recorded_evidence.MAX_BYTES + 1), "big.json"),
                 self.file('{"id":"PERF-020","id":"PERF-020"}', "twice.json"), self.file("{", "bad.json")]
        for path in cases:
            with self.subTest(path=path.name), self.assertRaises(ValueError):
                describe(*self.entry("PERF-020", path), gate="9")


if __name__ == "__main__":
    unittest.main()
