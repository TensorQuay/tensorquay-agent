"""REL-U-002: the completeness check passes a complete SBOM and refuses each kind of gap, naming it.

Run: python3 -m unittest discover -s tools/release -p 'test_*.py' -v
"""

import copy
import json
from pathlib import Path
import tempfile
import unittest

import sbom_check as check
import sbom_sources as sources
import sbom_spdx as spdx
from test_sbom import cargo_fixture, image_tree, write

DIGEST = "sha256:" + "cd" * 32
POLICY = check.Policy(allowed={"MIT", "Apache-2.0", "ISC", "Apache-2.0 WITH LLVM-exception"},
                      exceptions={}, forbidden_npm=["GPL", "GPL-*", "LGPL-*"])


def dpkg_owns(root, package, *paths):
    write(root, "var/lib/dpkg/info/{}.list".format(package), "".join(path + "\n" for path in paths))


def executable(root, relative, content=b"\x7fELF-binary"):
    path = Path(root) / relative.lstrip("/")
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(content)
    path.chmod(0o755)


class Fixture(unittest.TestCase):
    """A complete image: Debian packages owning their binaries, a declared component, npm packages and a crate
    closure, and the SBOM the generator makes for it."""

    def setUp(self):
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        self.root = Path(directory.name)
        image_tree(self.root)
        executable(self.root, "/usr/bin/bash")
        dpkg_owns(self.root, "bash", "/usr/bin/bash")
        # A library is checked by its ELF magic, executable bit or not.
        path = self.root / "usr/lib/x86_64-linux-gnu/libc.so.6"
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(b"\x7fELF-library")
        dpkg_owns(self.root, "libc6", "/usr/lib/x86_64-linux-gnu/libc.so.6")
        # The project's own binary, a declared component, and a native addon inside an npm package.
        executable(self.root, "/usr/local/bin/tqa-runtime")
        executable(self.root, "/usr/local/lib/node/bin/node")
        executable(self.root, "/usr/lib/node_modules/lodash/native.node")
        # Plain data is not checked.
        write(self.root, "etc/motd", "hello\n")
        self.metadata, self.tree, self.lock = cargo_fixture()
        self.crates = sources.cargo_packages(self.metadata, self.tree, self.lock)
        node = sources.Package(name="node", version="24.21.0", kind="generic", license="MIT",
                               download_location="https://nodejs.org/dist/v24.21.0/node-v24.21.0.tar.xz")
        self.components = [check.Declared(package=node, paths=["/usr/local/lib/node/"])]
        self.packages = (sources.debian_packages(self.root) + sources.npm_packages(self.root, ["usr/lib/node_modules"])
                         + self.crates + [node])
        self.document = spdx.document(spdx.Subject("tqa-runtime", "base", DIGEST), self.packages, created=0)

    def expectation(self, **changes):
        values = dict(root=self.root, digest=DIGEST, npm_dirs=["usr/lib/node_modules"], crates=self.crates,
                      declared=self.components, own_binaries=["/usr/local/bin/tqa-runtime"], policy=POLICY)
        values.update(changes)
        return check.Expectation(**values)

    def problems(self, document=None, **changes):
        return check.problems(document or self.document, self.expectation(**changes))


class Complete(Fixture):
    def test_a_complete_sbom_passes(self):
        self.assertEqual(self.problems(), [])


class Refusals(Fixture):
    def assert_refused(self, problems, *needles):
        self.assertTrue(problems, "refused")
        text = "\n".join(problems)
        for needle in needles:
            self.assertIn(needle, text)

    def test_c1_a_package_missing_from_the_document(self):
        document = copy.deepcopy(self.document)
        document["packages"] = [package for package in document["packages"] if package["name"] != "libc6"]
        self.assert_refused(self.problems(document), "C1", "libc6 2.41-12")

    def test_c1_a_package_without_its_source_location(self):
        document = copy.deepcopy(self.document)
        next(package for package in document["packages"] if package["name"] == "bash")["downloadLocation"] = "NOASSERTION"
        self.assert_refused(self.problems(document), "C1", "bash")

    def test_c2_the_engines_console_placeholder_is_not_software(self):
        write(self.root, "dev/console", "")
        (self.root / "dev/console").chmod(0o755)
        self.assertEqual(self.problems(), [])

    def test_c2_a_binary_nobody_declared(self):
        executable(self.root, "/opt/tool/tool")
        self.assert_refused(self.problems(), "C2", "/opt/tool/tool")

    def test_c2_a_packages_binary_moved_by_dpkg_divert_is_still_its_packages(self):
        # TOOL-U-064: the Desktop image moves xdotool off the PATH with dpkg-divert. dpkg records the move in
        # var/lib/dpkg/diversions (from, to, package; ":" for a local diversion); the package's list keeps the old path.
        (self.root / "usr/bin/bash").rename(self.root / "usr/bin/bash.moved")
        executable(self.root, "/usr/libexec/tqa/bash")
        write(self.root, "var/lib/dpkg/diversions", "/usr/bin/bash\n/usr/libexec/tqa/bash\n:\n")
        self.assertEqual([p for p in self.problems() if "/usr/libexec/tqa/bash" in p], [])
        # A diversion names only where an owned path went: it never owns a path no package has.
        executable(self.root, "/usr/libexec/tqa/planted")
        write(self.root, "var/lib/dpkg/diversions",
              "/usr/bin/bash\n/usr/libexec/tqa/bash\n:\n/usr/bin/nothing\n/usr/libexec/tqa/planted\n:\n")
        self.assert_refused(self.problems(), "C2", "/usr/libexec/tqa/planted")

    def test_c2_a_library_without_the_executable_bit_is_still_a_binary(self):
        path = self.root / "opt/lib/libhidden.so"
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(b"\x7fELF")
        self.assert_refused(self.problems(), "C2", "/opt/lib/libhidden.so")

    def test_c3_a_dev_only_crate_listed(self):
        dev = sources.cargo_packages(self.metadata, "proptest v1.11.0\n", self.lock)
        document = spdx.document(spdx.Subject("tqa-runtime", "base", DIGEST), self.packages + dev, created=0)
        self.assert_refused(self.problems(document), "C3", "proptest 1.11.0")

    def test_c3_an_npm_package_the_image_does_not_hold(self):
        extra = sources.Package(name="left-pad", version="1.3.0", kind="npm", license="MIT",
                                download_location="https://registry.npmjs.org/left-pad/-/left-pad-1.3.0.tgz")
        document = spdx.document(spdx.Subject("tqa-runtime", "base", DIGEST), self.packages + [extra], created=0)
        self.assert_refused(self.problems(document), "C3", "left-pad 1.3.0")

    def test_c4_a_linked_crate_under_a_licence_outside_the_policy(self):
        metadata = copy.deepcopy(self.metadata)
        metadata["packages"][1]["license"] = "GPL-3.0-only"
        crates = sources.cargo_packages(metadata, self.tree, self.lock)
        packages = [package for package in self.packages if package.kind != "cargo"] + crates
        document = spdx.document(spdx.Subject("tqa-runtime", "base", DIGEST), packages, created=0)
        self.assert_refused(self.problems(document, crates=crates), "C4", "serde", "GPL-3.0-only")

    def test_c4_one_allowed_alternative_is_enough(self):
        self.assertTrue(check.allowed("GPL-3.0-only OR MIT", POLICY.allowed))
        self.assertTrue(check.allowed("(MIT OR Apache-2.0) AND ISC", POLICY.allowed))
        self.assertFalse(check.allowed("MIT AND GPL-3.0-only", POLICY.allowed))
        self.assertTrue(check.allowed("Apache-2.0 WITH LLVM-exception", POLICY.allowed))
        # Parentheses that enclose only part of an expression are not stripped.
        self.assertTrue(check.allowed("(GPL-3.0-only OR MIT) AND (ISC OR GPL-2.0-only)", POLICY.allowed))
        self.assertFalse(check.allowed("(GPL-3.0-only OR MIT) AND (GPL-2.0-only)", POLICY.allowed))

    def test_c4_a_forbidden_npm_licence(self):
        write(self.root, "usr/lib/node_modules/copyleft/package.json",
              json.dumps({"name": "copyleft", "version": "1.0.0", "license": "GPL-3.0-only"}))
        packages = [package for package in self.packages if package.kind != "npm"] + sources.npm_packages(
            self.root, ["usr/lib/node_modules"])
        document = spdx.document(spdx.Subject("tqa-runtime", "base", DIGEST), packages, created=0)
        self.assert_refused(self.problems(document), "C4", "copyleft")

    def test_c5_a_package_nothing_contains(self):
        document = copy.deepcopy(self.document)
        document["relationships"] = document["relationships"][:-1]
        self.assert_refused(self.problems(document), "C5")

    def test_c5_a_licence_reference_without_its_text(self):
        document = copy.deepcopy(self.document)
        document["hasExtractedLicensingInfos"] = document["hasExtractedLicensingInfos"][1:]
        self.assert_refused(self.problems(document), "C5", "LicenseRef-")

    def test_c6_another_images_digest(self):
        self.assert_refused(self.problems(digest="sha256:" + "ef" * 32), "C6")

    def test_problems_are_bounded_and_counted(self):
        for index in range(50):
            executable(self.root, "/opt/many/tool-{}".format(index))
        problems = self.problems()
        self.assertLessEqual(len(problems), check.MAX_LISTED + 1)
        self.assertIn("50 unowned", "\n".join(problems))


if __name__ == "__main__":
    unittest.main()
