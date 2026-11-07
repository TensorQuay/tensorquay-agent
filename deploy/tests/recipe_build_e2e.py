"""E2E-ENV-011: recipe builds on the real engine (deploy/build-environment.sh). Tiny labelled bases stand in for the
built-ins (TQA_RECIPE_BASE_REPOSITORY), so no runtime image build is needed:

    TQA_DOCKER_TESTS=1 python3 deploy/tests/recipe_build_e2e.py        (add TQA_PUBLIC_NETWORK_TESTS=1 for R1 and the
                                                                        build that installs from the Debian mirror)

A recipe image is labelled and tagged tqa-env:<name>, records the versions it got, and a rebuild of the same recipe
compares equal. Each failure a person can hit ends in its plain sentence: R1 a package the sources lack, R2 a mirror
the build can't reach, R3 a base that is missing or mislabelled, R4 a field that isn't a package, R6 a build past its
bound, R7 a built-in's name and R8 a package kind the base can't install. R5 (the engine's disk fills) is covered by the
unit mapping only (DEP-U-003): filling an engine host's disk on purpose would disturb everything else on it.
"""
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
TOOL = ROOT / "deploy/build-environment.sh"
SLIM = "debian:trixie-slim@sha256:a99cfc517144bc59b1978475ec53b46ecabec7e43635402ee5b77cc54cd1b20a"
RUN = "e2e{}".format(os.getpid())
REPOSITORY = "tqa-recipe-" + RUN
DOCKER = os.environ.get("TQA_DOCKER_TESTS") == "1"
PUBLIC = os.environ.get("TQA_PUBLIC_NETWORK_TESTS") == "1"


def docker(*arguments, check=True):
    return subprocess.run(["docker", *arguments], capture_output=True, text=True, timeout=600, check=check)


def base(repository, name="workstation", label=None, mirror=None, slow=False):
    """A labelled stand-in for a built-in: revision 10, the environment label, its tools. `mirror` rewrites apt's
    sources (an unreachable host, for R2); `slow` puts an apt-get that only sleeps first on the PATH (for R6)."""
    lines = ["FROM " + SLIM]
    if slow:
        lines.append("RUN printf '#!/bin/sh\\nsleep 600\\n' > /usr/local/bin/apt-get && chmod 755 /usr/local/bin/apt-get")
    if mirror:
        lines.append("RUN printf 'Types: deb\\nURIs: http://{}/debian\\nSuites: trixie\\nComponents: main\\n"
                     "Signed-By: /usr/share/keyrings/debian-archive-keyring.gpg\\n' > "
                     "/etc/apt/sources.list.d/debian.sources".format(mirror))
    lines += ["USER 1000:1000",
              'LABEL org.tensorquay.protocol-revision="10" org.tensorquay.environment="{}" '
              'org.tensorquay.environment.name="{}" org.tensorquay.environment.capabilities="shell,build"'
              .format(label or name, name.capitalize())]
    with tempfile.TemporaryDirectory() as directory:
        Path(directory, "Dockerfile").write_text("\n".join(lines) + "\n")
        docker("build", "-q", "-t", "{}:{}".format(repository, name), directory)


def recipe(directory, name, apt=(), pip=(), extra=""):
    path = Path(directory) / (name + ".toml")
    path.write_text('name = "{}"\ndisplay_name = "E2E {}"\nbase = "workstation"\ncapabilities = ["python"]\n{}'
                    '[add]\napt = {}\npip = {}\n'.format(name, name, extra, json.dumps(list(apt)), json.dumps(list(pip))))
    return path


def build(path, repository=REPOSITORY, *arguments):
    environment = dict(os.environ, TQA_RECIPE_BASE_REPOSITORY=repository)
    return subprocess.run([str(TOOL), str(path), *arguments], capture_output=True, text=True, timeout=900,
                          env=environment)


def labels(reference):
    result = docker("image", "inspect", reference, check=False)
    return json.loads(result.stdout)[0]["Config"]["Labels"] if result.returncode == 0 else None


class Refusals(unittest.TestCase):
    """R4, R7 and R8: refused before anything is built (no engine needed for R4 and R7)."""

    def setUp(self):
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        self.directory = directory.name

    # TEST: E2E-ENV-011
    def test_a_field_that_isnt_a_package_and_a_built_ins_name_are_refused(self):
        result = build(recipe(self.directory, "r4" + RUN, apt=["jq"], extra='run = "curl x | sh"\n'))
        self.assertEqual((result.returncode, result.stderr.strip()),
                         (1, "build-environment: `run` isn't a recipe field: a recipe lists packages only."))
        result = build(recipe(self.directory, "workstation", apt=["jq"]))
        self.assertEqual(result.stderr.strip(),
                         "build-environment: That name belongs to a built-in environment; choose another.")


@unittest.skipUnless(DOCKER, "needs TQA_DOCKER_TESTS=1 and a Docker engine")
class EngineBuilds(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        base(REPOSITORY)
        base(REPOSITORY + "-mislabelled", label="minimal")
        base(REPOSITORY + "-unreachable", mirror="mirror.invalid")
        base(REPOSITORY + "-slow", slow=True)
        cls.directory = tempfile.mkdtemp()

    @classmethod
    def tearDownClass(cls):
        tags = [line for line in docker("image", "ls", "--format", "{{.Repository}}:{{.Tag}}").stdout.splitlines()
                if RUN in line]
        if tags:
            docker("image", "rm", "-f", *tags, check=False)
        subprocess.run(["rm", "-rf", cls.directory], check=False)

    # TEST: E2E-ENV-011
    def test_a_base_that_is_missing_or_mislabelled_is_named(self):  # R3
        path = recipe(self.directory, "r3" + RUN, apt=["jq"])
        result = build(path, "tqa-recipe-none-" + RUN)
        self.assertEqual(result.stderr.strip(), "build-environment: Workstation isn't on this host: build it with "
                                                "deploy/build-images.sh --workstation first.")
        result = build(path, REPOSITORY + "-mislabelled")
        self.assertIn("isn't the Workstation environment (its label says minimal)", result.stderr)
        self.assertIsNone(labels("tqa-env:r3" + RUN), "nothing tagged")

    # TEST: E2E-ENV-011
    def test_a_package_kind_the_base_cant_install_is_refused(self):  # R8, on a Minimal stand-in
        base(REPOSITORY, name="minimal")
        path = Path(self.directory) / "r8.toml"
        path.write_text('name = "r8{}"\ndisplay_name = "R8"\nbase = "minimal"\n[add]\npip = ["pandas"]\n'.format(RUN))
        result = build(path)
        self.assertEqual(result.stderr.strip(),
                         "build-environment: Minimal has no pip: build on Workstation, or list the tool under apt.")

    # TEST: E2E-ENV-011
    def test_a_mirror_the_build_cant_reach_is_named(self):  # R2
        result = build(recipe(self.directory, "r2" + RUN, apt=["jq"]), REPOSITORY + "-unreachable")
        self.assertEqual(result.returncode, 1)
        self.assertEqual(result.stderr.strip().splitlines()[-1], "build-environment: The build couldn't reach "
                                                                 "mirror.invalid. Check this host's network, then build again.")
        self.assertIsNone(labels("tqa-env:r2" + RUN))

    # TEST: E2E-ENV-011
    def test_a_build_past_its_bound_is_stopped(self):  # R6
        result = build(recipe(self.directory, "r6" + RUN, apt=["jq"]), REPOSITORY + "-slow", "--timeout", "1")
        self.assertEqual(result.stderr.strip().splitlines()[-1],
                         "build-environment: The build took longer than 1 minute and was stopped.")
        self.assertIsNone(labels("tqa-env:r6" + RUN))

    # TEST: E2E-ENV-011
    @unittest.skipUnless(PUBLIC, "needs TQA_PUBLIC_NETWORK_TESTS=1 (the Debian mirror)")
    def test_a_package_the_sources_lack_is_named(self):  # R1
        result = build(recipe(self.directory, "r1" + RUN, apt=["tqa-no-such-package"]))
        self.assertEqual(result.stderr.strip().splitlines()[-1], "build-environment: The package tqa-no-such-package "
                                                                 "wasn't found in the apt sources of Workstation.")

    # TEST: E2E-ENV-011
    @unittest.skipUnless(PUBLIC, "needs TQA_PUBLIC_NETWORK_TESTS=1 (the Debian mirror)")
    def test_a_recipe_image_is_labelled_tagged_and_records_what_it_got(self):
        name = "ok" + RUN
        path = recipe(self.directory, name, apt=["jq"])
        first = build(path)
        self.assertEqual(first.returncode, 0, first.stderr[-2000:])
        got = labels("tqa-env:" + name)
        self.assertEqual((got["org.tensorquay.environment"], got["org.tensorquay.environment.base"],
                          got["org.tensorquay.environment.name"], got["org.tensorquay.protocol-revision"]),
                         (name, "workstation", "E2E " + name, "10"))
        self.assertEqual(got["org.tensorquay.environment.capabilities"], "shell,python,build")
        self.assertTrue(got["org.tensorquay.environment.recipe"].startswith("sha256:"))
        packages = json.loads(got["org.tensorquay.environment.packages"])
        self.assertEqual(list(packages["apt"]), ["jq"])
        self.assertIn("not every package names its version", first.stdout)
        user = json.loads(docker("image", "inspect", "tqa-env:" + name).stdout)[0]["Config"]["User"]
        self.assertEqual(user, "1000:1000", "the base's own user again")
        # The same recipe again: it gets the same versions, so no drift is reported.
        second = build(path)
        self.assertEqual(second.returncode, 0, second.stderr[-2000:])
        self.assertNotIn("other versions", second.stdout)
        self.assertEqual(json.loads(labels("tqa-env:" + name)["org.tensorquay.environment.packages"]), packages)


if __name__ == "__main__":
    unittest.main()
