"""REL-U-015: tools/release/join-images.sh joins a version's amd64 and arm64 images under the version, one index per
image, and prints each index's digest. No Docker: a recording `docker` on PATH answers as the registry would.

18.2.0's release run (23 Nov 2026) failed at this step: the runner's Docker had been updated and refused the step's
index annotations ("annotations are not supported for Docker manifest lists"), so no version tag was published though
every architecture's image was. The join takes no annotation, and a version can be joined again by hand
(`workflow_dispatch` with `join_version`) without building anything."""

from pathlib import Path
import subprocess
import tempfile
import textwrap
import unittest

ROOT = Path(__file__).resolve().parents[2]
SCRIPT = ROOT / "tools" / "release" / "join-images.sh"
WORKFLOW = ROOT / ".github" / "workflows" / "images.yml"
IMAGES = "gateway=tqa-gateway:@ web=tqa-web:@ runtime=tqa-runtime:base-@"

# The recording docker: appends each call to $DOCKER_LOG. `buildx imagetools create` fails as the updated runner's did
# when given an annotation, and when $DOCKER_MISSING names a source tag; `imagetools inspect` prints a digest.
FAKE_DOCKER = textwrap.dedent("""\
    #!/usr/bin/env python3
    import hashlib, os, sys
    args = sys.argv[1:]
    with open(os.environ["DOCKER_LOG"], "a") as log:
        log.write(" ".join(args) + "\\n")
    if args[:3] == ["buildx", "imagetools", "create"]:
        if any(arg.startswith("--annotation") for arg in args):
            sys.exit("ERROR: annotations are not supported for Docker manifest lists")
        if any(arg == os.environ.get("DOCKER_MISSING") for arg in args):
            sys.exit("ERROR: " + os.environ["DOCKER_MISSING"] + ": not found")
    if args[:3] == ["buildx", "imagetools", "inspect"]:
        print("sha256:" + hashlib.sha256(args[-1].encode()).hexdigest())
    """)


class JoinImages(unittest.TestCase):
    def run_script(self, *args, **env):
        with tempfile.TemporaryDirectory() as scratch:
            bin_dir = Path(scratch) / "bin"
            bin_dir.mkdir()
            (bin_dir / "docker").write_text(FAKE_DOCKER)
            (bin_dir / "docker").chmod(0o755)
            log = Path(scratch) / "docker.log"
            log.touch()
            summary = Path(scratch) / "summary.md"
            environment = {"PATH": f"{bin_dir}:/usr/bin:/bin", "DOCKER_LOG": str(log), "HOME": scratch,
                           "IMAGES": IMAGES, "REGISTRY": "ghcr.io/tensorquay", "GITHUB_STEP_SUMMARY": str(summary),
                           **env}
            result = subprocess.run(["bash", str(SCRIPT), *args], capture_output=True, text=True, env=environment,
                                    timeout=60, check=False)
            return result, log.read_text().splitlines(), summary.read_text() if summary.exists() else ""

    # TEST: REL-U-015
    def test_each_image_is_joined_from_both_architectures_without_annotations(self):
        result, calls, summary = self.run_script("v18.2.0")
        self.assertEqual(result.returncode, 0, result.stderr)
        creates = [call for call in calls if call.startswith("buildx imagetools create")]
        self.assertEqual(creates, [
            f"buildx imagetools create -t ghcr.io/tensorquay/tensorquay-agent-{name}:18.2.0 "
            f"ghcr.io/tensorquay/tensorquay-agent-{name}:18.2.0-amd64 "
            f"ghcr.io/tensorquay/tensorquay-agent-{name}:18.2.0-arm64"
            for name in ("gateway", "web", "runtime")])
        self.assertFalse(any("--annotation" in call for call in calls), calls)
        # Each index's digest is said, for the release page.
        for name in ("gateway", "web", "runtime"):
            self.assertIn(f"`ghcr.io/tensorquay/tensorquay-agent-{name}` | `sha256:", summary)
        self.assertIn("### Images of 18.2.0", summary)

    def test_a_missing_architecture_stops_the_join_and_says_which(self):
        missing = "ghcr.io/tensorquay/tensorquay-agent-web:18.2.0-arm64"
        result, calls, _ = self.run_script("18.2.0", DOCKER_MISSING=missing)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn(missing, result.stderr)
        # Nothing after the failure: the runtime is never joined on a half-published version.
        self.assertFalse(any("tensorquay-agent-runtime" in call for call in calls), calls)

    def test_a_version_that_is_not_one_is_refused_before_any_call(self):
        for bad in ("", "main", "18.2", "v18.2.0-amd64", "18.2.0; rm -rf /"):
            with self.subTest(version=bad):
                result, calls, _ = self.run_script(bad)
                self.assertEqual(result.returncode, 2, result.stderr)
                self.assertEqual(calls, [])

    def test_the_workflow_joins_with_the_script_and_can_join_a_published_version_again(self):
        text = WORKFLOW.read_text()
        self.assertIn("tools/release/join-images.sh", text)
        self.assertNotIn("--annotation", text, "the runner's Docker refuses index annotations on these images")
        self.assertIn("join_version", text, "a published version can be joined again without a build")


if __name__ == "__main__":
    unittest.main()
