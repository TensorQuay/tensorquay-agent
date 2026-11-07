"""REL-U-009: the gateway's and the runtime's release builds carry no `testing` feature, so no test hook (the gateway's
`trust_upstream_root`, the runtime's journal faults) is in a release binary. The images build them without features,
and only the crates' own tests switch `testing` on, through a dev-dependency on themselves, which Cargo's feature
resolver keeps out of a normal build. Reads Cargo's feature graph; no build, no network.

    python3 -m unittest discover -s tools/release -p 'test_release_features.py' -v
"""

from pathlib import Path
import re
import subprocess
import unittest

ROOT = Path(__file__).resolve().parents[2]
# Each release binary and the Dockerfile that builds it.
RELEASES = {
    "tqa-gateway": ROOT / "images/gateway/Dockerfile",
    "tqa-runtime": ROOT / "images/runtime/Dockerfile",
}


def feature_graph(package: str, edges: str) -> str:
    """Cargo's feature graph of `package` along `edges`, one node per line."""
    return subprocess.run(
        ["cargo", "tree", "--locked", "--offline", "--target", "all", "--prefix", "none",
         "-p", package, "-e", edges],
        cwd=ROOT, capture_output=True, text=True, check=True,
    ).stdout


class ReleaseFeaturesTest(unittest.TestCase):
    # TEST: REL-U-009
    def test_rel_u_009_a_release_build_has_no_testing_feature(self) -> None:
        for package, dockerfile in RELEASES.items():
            with self.subTest(package=package):
                builds = [line for line in dockerfile.read_text().splitlines() if "cargo build" in line]
                self.assertTrue(builds, dockerfile)
                for line in builds:
                    self.assertIn("--release", line)
                    self.assertNotRegex(line, r"--(all-)?features|-F\b", "the image builds the release without features")
                release = feature_graph(package, "normal,features")
                self.assertNotIn(f'{package} feature "testing"', release)

    # TEST: REL-U-009
    def test_rel_u_009_the_check_sees_the_feature_where_tests_switch_it_on(self) -> None:
        # The same graph with dev-dependencies has it: the check above would see it if a release build did.
        for package in RELEASES:
            with self.subTest(package=package):
                tests = feature_graph(package, "normal,dev,features")
                self.assertTrue(re.search(rf'^{re.escape(package)} feature "testing"', tests, re.MULTILINE), package)


if __name__ == "__main__":
    unittest.main()
