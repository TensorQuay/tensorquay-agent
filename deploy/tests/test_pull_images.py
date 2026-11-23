"""REL-U-013: deploy/pull-images.sh pulls a release's prebuilt images by version, pins each by the digest the registry
gave, tags the runtimes with the canonical environment tags the gateway registers (as deploy/build-images.sh does),
and prints step 1's three exports; a bad or missing version, or an image that came with no registry digest, is
refused. No Docker: a recording `docker` on PATH answers as a daemon would."""

import hashlib
from pathlib import Path
import subprocess
import tempfile
import textwrap
import unittest

ROOT = Path(__file__).resolve().parents[2]
SCRIPT = ROOT / "deploy" / "pull-images.sh"

# The recording docker: appends each call to $DOCKER_LOG; `image inspect` answers a pulled image's digest (or none
# when $DOCKER_NO_DIGEST names it) and a local ID for tqa-runtime:base.
FAKE_DOCKER = textwrap.dedent("""\
    #!/usr/bin/env python3
    import hashlib, os, sys
    args = sys.argv[1:]
    with open(os.environ["DOCKER_LOG"], "a") as log:
        log.write(" ".join(args) + "\\n")
    if args[:2] == ["image", "inspect"]:
        ref, form = args[-1], args[3]
        if "RepoDigests" in form:
            repository = ref.rsplit(":", 1)[0]
            if repository.rsplit("/", 1)[-1] != os.environ.get("DOCKER_NO_DIGEST"):
                # Another name's digest first, as an image pulled from a mirror too would list it.
                print("mirror.example/other@sha256:" + "0" * 64)
                print(repository + "@sha256:" + hashlib.sha256(ref.encode()).hexdigest())
        else:
            print("sha256:" + "1" * 64)
    """)

FAKE_GIT = textwrap.dedent("""\
    #!/bin/sh
    [ -n "$GIT_TAG" ] && echo "$GIT_TAG" && exit 0
    exit 128
    """)


def digest(ref):
    return hashlib.sha256(ref.encode()).hexdigest()


class PullImagesTests(unittest.TestCase):
    def run_script(self, *args, **env):
        with tempfile.TemporaryDirectory() as scratch:
            bin_dir = Path(scratch) / "bin"
            bin_dir.mkdir()
            for name, body in (("docker", FAKE_DOCKER), ("git", FAKE_GIT)):
                (bin_dir / name).write_text(body)
                (bin_dir / name).chmod(0o755)
            log = Path(scratch) / "docker.log"
            log.touch()
            environment = {"PATH": f"{bin_dir}:/usr/bin:/bin", "DOCKER_LOG": str(log), "HOME": scratch, **env}
            result = subprocess.run([str(SCRIPT), *args], capture_output=True, text=True, env=environment,
                                    timeout=60, check=False)
            return result, log.read_text().splitlines()

    def test_pulls_pins_tags_and_prints_the_exports(self):
        result, calls = self.run_script("18.0.0")
        self.assertEqual(result.returncode, 0, result.stderr)
        pulls = [call for call in calls if call.startswith("pull ")]
        self.assertEqual(pulls, [f"pull --quiet ghcr.io/tensorquay/tensorquay-agent-{name}:18.0.0"
                                 for name in ("runtime", "gateway", "web")])
        runtime = "ghcr.io/tensorquay/tensorquay-agent-runtime@sha256:" + digest(
            "ghcr.io/tensorquay/tensorquay-agent-runtime:18.0.0")
        self.assertIn(f"tag {runtime} tqa-runtime:base", calls)
        self.assertIn(f"tag {runtime} tqa-runtime:minimal", calls)
        self.assertFalse(any("workstation" in call or "desktop" in call for call in calls), calls)
        exports = [line for line in result.stdout.splitlines() if line.startswith("export ")]
        gateway = "ghcr.io/tensorquay/tensorquay-agent-gateway"
        web = "ghcr.io/tensorquay/tensorquay-agent-web"
        self.assertEqual(exports, [
            "export TQA_RUNTIME_IMAGE='sha256:" + "1" * 64 + "'",
            f"export TQA_GATEWAY_IMAGE='{gateway}@sha256:{digest(gateway + ':18.0.0')}'",
            f"export TQA_WEB_IMAGE='{web}@sha256:{digest(web + ':18.0.0')}'",
        ])

    def test_desktop_brings_the_workstation_and_both_canonical_tags(self):
        result, calls = self.run_script("--desktop", "v18.0.0")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("pull --quiet ghcr.io/tensorquay/tensorquay-agent-workstation:18.0.0", calls)
        self.assertIn("pull --quiet ghcr.io/tensorquay/tensorquay-agent-desktop:18.0.0", calls)
        tags = [call.split()[-1] for call in calls if call.startswith("tag ")]
        self.assertEqual(tags, ["tqa-runtime:base", "tqa-runtime:minimal", "tqa-runtime:workstation",
                                "tqa-runtime:desktop"])
        self.assertIn("Desktop is registered the same way", result.stdout)

    # TEST: REL-U-014
    def test_fake_model_brings_the_test_model_tagged_as_the_installer_runs_it(self):
        """The scripted test model is a published image too, so a try without a model server builds nothing."""
        result, calls = self.run_script("--fake-model", "v18.0.0")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("pull --quiet ghcr.io/tensorquay/tensorquay-agent-fake-model:18.0.0", calls)
        model = f"ghcr.io/tensorquay/tensorquay-agent-fake-model@sha256:{digest('ghcr.io/tensorquay/tensorquay-agent-fake-model:18.0.0')}"
        self.assertIn(f"tag {model} tqa-fake-model:local", calls)
        plain, plain_calls = self.run_script("v18.0.0")
        self.assertEqual(plain.returncode, 0, plain.stderr)
        self.assertFalse(any("fake-model" in call for call in plain_calls), plain_calls)

    def test_the_version_defaults_to_the_checkouts_release_tag(self):
        result, calls = self.run_script(GIT_TAG="v18.1.2")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("pull --quiet ghcr.io/tensorquay/tensorquay-agent-gateway:18.1.2", calls)

    def test_another_registry_holds_the_same_names(self):
        result, calls = self.run_script("18.0.0-rc.1", TQA_IMAGE_REGISTRY="localhost:5000/tq")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("pull --quiet localhost:5000/tq/tensorquay-agent-web:18.0.0-rc.1", calls)
        self.assertIn("export TQA_WEB_IMAGE='localhost:5000/tq/tensorquay-agent-web@sha256:", result.stdout)

    def test_refuses_without_a_release_or_with_a_bad_one(self):
        for args in ((), ("latest",), ("18.0",), ("18.0.0", "18.0.1"), ("--fake-model",)):
            with self.subTest(args=args):
                result, calls = self.run_script(*args)
                self.assertEqual(result.returncode, 2, result.stdout)
                self.assertEqual(calls, [], "nothing is pulled")

    def test_refuses_an_image_the_registry_gave_no_digest(self):
        result, calls = self.run_script("18.0.0", DOCKER_NO_DIGEST="tensorquay-agent-gateway")
        self.assertEqual(result.returncode, 1)
        self.assertIn("has no digest from the registry", result.stderr)
        self.assertFalse(any(call.startswith("pull --quiet ghcr.io/tensorquay/tensorquay-agent-web") for call in calls))
        self.assertNotIn("export ", result.stdout)


if __name__ == "__main__":
    unittest.main()
