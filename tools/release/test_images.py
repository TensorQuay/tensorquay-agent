"""REL-I-001: every release image gets an SBOM passing C1–C6, and an undeclared binary planted in a copy of an image
fails C2. REL-I-002: each built-in template's rootfs lists exactly its source image plus `tqa-vm-init`, and the guest
kernel's SBOM links the exact tarball and publishes its config. REL-I-003: the web image ships no package manager and
is on Debian 13; Workstation's npm is the pinned one and its Chromium is at least the audited security update.
REL-I-004: the Desktop's Chromium reads the quiet policy and switches the repository holds, the password manager and
autofill off. REL-I-005: the Workstation and the Desktop have a font for Chinese and Noto Color Emoji.
Needs Docker and the images built at one commit (no
network, nothing runs in them); REL-I-002 also needs the templates' rootfs and the kernel built from them:

    TQA_DOCKER_TESTS=1 TQA_IMAGE_TAG=<tag> [TQA_FC_ROOTFS_DIR=<rootfs dir> TQA_FC_KERNEL_DIR=<kernel dir>] \\
        python3 -m unittest discover -s tools/release -p 'test_images.py' -v

The images are tqa-gateway:<tag>, tqa-web:<tag>, tqa-runtime:base-<tag>, tqa-runtime:workstation-<tag>,
tqa-runtime:desktop-<tag> (the Desktop, ADR-0039) and tqa-vmm:<tag>; each rootfs is `<rootfs dir>/<source image id>/` (images/rootfs/build_rootfs.py).
"""

import io
import json
import os
from pathlib import Path
import re
import subprocess
import tarfile
import tempfile
import unittest

import sbom
import sbom_artefacts as artefacts
import sbom_firecracker as firecracker

TAG = os.environ.get("TQA_IMAGE_TAG", "")
ROOTFS_DIR = os.environ.get("TQA_FC_ROOTFS_DIR", "")
KERNEL_DIR = os.environ.get("TQA_FC_KERNEL_DIR", "")
DOCKER = os.environ.get("TQA_DOCKER_TESTS") == "1" and TAG
IMAGES = {
    "gateway": "tqa-gateway:{}",
    "web": "tqa-web:{}",
    "runtime-base": "tqa-runtime:base-{}",
    "runtime-workstation": "tqa-runtime:workstation-{}",
    "runtime-desktop": "tqa-runtime:desktop-{}",
    "vmm": "tqa-vmm:{}",
}


def docker(*arguments):
    return subprocess.run(("docker",) + arguments, check=True, capture_output=True, text=True,
                          timeout=sbom.TIMEOUT).stdout.strip()


# Debian 13's security update of 2026-10-29 (the c17 release dry run's vulnerability gate: 104 HIGH in 155; before it,
# 2026-10-15's for the c16y pre-check and 2026-10-01's for the c15 release gate); an older Chromium fails REL-I-003.
CHROMIUM_FLOOR = "156.0.8078.73-1~deb13u1"
# The other Debian packages the c17 gate found HIGH, at their security fixes (debian-security, 20 to 29 Oct 2026): a
# runtime image that holds one holds at least this version.
DEBIAN_FLOORS = {
    "libheif1": "1.23.6-2~deb13u1",
    "libbrotli1": "1.1.0-2+deb13u1",
    "ca-certificates": "20250419+deb13u1",
}
# npm's bundled undici at least its fixed release (the c17 gate: 6.28.1 HIGH, fixed in 6.29.1).
UNDICI_FLOOR = (6, 29, 1)
REPOSITORY = Path(__file__).resolve().parents[2]


def listed(document):
    return {(package["name"], package["versionInfo"]) for package in document["packages"][1:]}


@unittest.skipUnless(DOCKER, "needs Docker (TQA_DOCKER_TESTS=1, TQA_IMAGE_TAG)")
class ReleaseImages(unittest.TestCase):
    def test_every_image_gets_a_complete_sbom_and_package_list(self):
        with tempfile.TemporaryDirectory() as out:
            for name, image in IMAGES.items():
                with self.subTest(name):
                    document, problems, digest = sbom.generate(name, image.format(TAG), "0.0.0-test", created=0)
                    self.assertEqual(problems, [])
                    sbom.write(Path(out), name, document, digest)
            index = json.loads((Path(out) / "index.json").read_text(encoding="utf-8"))
            self.assertEqual(sorted(index), sorted(IMAGES))
            for name, entry in index.items():
                self.assertTrue((Path(out) / entry["spdx"]).is_file(), name)
                self.assertIn(entry["digest"], (Path(out) / entry["packages"]).read_text(encoding="utf-8"))

    def test_an_undeclared_binary_planted_in_a_copy_fails_c2(self):
        container = docker("create", "--network", "none", IMAGES["gateway"].format(TAG), "true")
        planted = "tqa-sbom-planted:{}".format(os.getpid())
        try:
            with tempfile.TemporaryDirectory() as directory:
                tool = Path(directory) / "tool"
                tool.write_bytes(b"\x7fELF-planted")
                tool.chmod(0o755)
                docker("cp", str(tool), container + ":/planted-tool")
            docker("commit", container, planted)
            _, problems, _ = sbom.generate("gateway", planted, "0.0.0-test", created=0)
            self.assertIn("C2: /planted-tool is not owned by a package or a declared component", problems)
        finally:
            docker("rm", "-f", container)
            subprocess.run(["docker", "image", "rm", "-f", planted], capture_output=True, timeout=sbom.TIMEOUT)


class ImageFiles:
    """Files of an image, read from a created container that never starts."""

    def __init__(self, image):
        self.container = docker("create", "--network", "none", image, "true")

    def read(self, path):
        """The file's bytes, or None when the image does not hold it."""
        done = subprocess.run(["docker", "cp", self.container + ":" + path, "-"], capture_output=True,
                              timeout=sbom.TIMEOUT)
        if done.returncode != 0:
            return None
        with tarfile.open(fileobj=io.BytesIO(done.stdout)) as archive:
            member = archive.next()
            return archive.extractfile(member).read() if member.isfile() else b""

    def close(self):
        docker("rm", "-f", self.container)


def dpkg_version(status, package):
    """The installed version of `package` in a dpkg status file."""
    for stanza in status.split("\n\n"):
        fields = dict(line.split(": ", 1) for line in stanza.splitlines() if ": " in line and not line[0].isspace())
        if fields.get("Package") == package and fields.get("Status", "").endswith(" installed"):
            return fields["Version"]
    return None


@unittest.skipUnless(DOCKER, "needs Docker (TQA_DOCKER_TESTS=1, TQA_IMAGE_TAG)")
class ImageHardening(unittest.TestCase):
    """REL-I-003, from the dependency audit of 2026-09-28."""

    def files(self, name):
        files = ImageFiles(IMAGES[name].format(TAG))
        self.addCleanup(files.close)
        return files

    def test_the_web_image_ships_node_without_a_package_manager_on_debian_13(self):
        web = self.files("web")
        self.assertTrue((web.read("/etc/debian_version") or b"").startswith(b"13."))
        self.assertIsNotNone(web.read("/usr/local/bin/node"))
        for path in ("/usr/local/lib/node_modules/npm/package.json", "/usr/local/lib/node_modules/corepack/package.json",
                     "/usr/local/bin/npm", "/usr/local/bin/npx", "/usr/local/bin/corepack", "/usr/local/bin/yarn",
                     "/opt/yarn-v1.22.22/package.json"):
            with self.subTest(path):
                self.assertIsNone(web.read(path))

    def test_workstation_npm_is_the_pinned_release(self):
        pinned = re.search(r"^ARG NPM_VERSION=(\S+)", (REPOSITORY / "images/runtime/Dockerfile").read_text(),
                           re.MULTILINE)
        self.assertIsNotNone(pinned)
        shipped = json.loads(self.files("runtime-workstation").read("/usr/local/lib/node_modules/npm/package.json"))
        self.assertEqual(shipped["version"], pinned.group(1))
        # The bundled packages the Dockerfile replaces with fixed releases (the c15 release gate) are those releases.
        dockerfile = (REPOSITORY / "images/runtime/Dockerfile").read_text()
        for name in ("undici", "brace-expansion"):
            with self.subTest(name):
                fixed = re.search(r"/{0}/-/{0}-(\S+)\.tgz /tmp/{0}\.tgz$".format(name), dockerfile, re.MULTILINE)
                self.assertIsNotNone(fixed)
                bundled = json.loads(self.files("runtime-workstation").read(
                    "/usr/local/lib/node_modules/npm/node_modules/{}/package.json".format(name)))
                self.assertEqual(bundled["version"], fixed.group(1))
                if name == "undici":
                    version = tuple(int(part) for part in bundled["version"].split("-")[0].split("."))
                    self.assertGreaterEqual(version, UNDICI_FLOOR, "undici {}".format(bundled["version"]))

    def test_every_runtime_image_names_the_sandbox_user(self):
        # The gateway runs every sandbox as 1000:1000 (tqa-sandbox's docker/config.rs). A program that looks the user up
        # refuses a uid with no entry: the Desktop's session bus did ("Could not get password database information for
        # UID … 1000: not found"), so no c16 Desktop started.
        for name in ("runtime-base", "runtime-workstation", "runtime-desktop"):
            with self.subTest(name):
                image = IMAGES[name].format(TAG)
                user = docker("run", "--rm", "--network", "none", "--user", "1000:1000", "--entrypoint", "getent",
                              image, "passwd", "1000")
                fields = user.split(":")
                self.assertEqual(fields[2:4], ["1000", "1000"], user)
                self.assertEqual(fields[5], "/workspace", user)
                group = docker("run", "--rm", "--network", "none", "--user", "1000:1000", "--entrypoint", "getent",
                               image, "group", "1000")
                self.assertEqual(group.split(":")[2], "1000", group)

    def test_workstation_chromium_is_at_least_the_audited_security_update(self):
        status = self.files("runtime-workstation").read("/var/lib/dpkg/status").decode()
        version = dpkg_version(status, "chromium")
        self.assertIsNotNone(version)
        newer_or_equal = subprocess.run(["dpkg", "--compare-versions", version, "ge", CHROMIUM_FLOOR]).returncode == 0
        self.assertTrue(newer_or_equal, "chromium {} is older than {}".format(version, CHROMIUM_FLOOR))

    def test_runtime_images_hold_the_audited_debian_fixes(self):
        for name in ("runtime-base", "runtime-workstation", "runtime-desktop"):
            status = self.files(name).read("/var/lib/dpkg/status").decode()
            for package, floor in DEBIAN_FLOORS.items():
                version = dpkg_version(status, package)
                if version is None:
                    continue
                with self.subTest(image=name, package=package):
                    compared = subprocess.run(["dpkg", "--compare-versions", version, "ge", floor])
                    self.assertEqual(compared.returncode, 0, "{} {} is older than {}".format(package, version, floor))


@unittest.skipUnless(DOCKER, "needs Docker (TQA_DOCKER_TESTS=1, TQA_IMAGE_TAG)")
class DesktopInputTool(unittest.TestCase):
    """REL-I-003 for TOOL-U-064 (the c16k real-world run, 17 Oct 2026: the model drove its desktop with xdotool from
    the shell). The Desktop image keeps xdotool for the computer tool, off the sandbox user's PATH."""

    def test_xdotool_is_the_computer_tools_and_off_the_path(self):
        files = ImageFiles(IMAGES["runtime-desktop"].format(TAG))
        self.addCleanup(files.close)
        self.assertIsNone(files.read("/usr/bin/xdotool"), "xdotool is on the sandbox user's PATH")
        self.assertIsNotNone(files.read("/usr/libexec/tqa/xdotool"), "the computer tool's xdotool is missing")


@unittest.skipUnless(DOCKER, "needs Docker (TQA_DOCKER_TESTS=1, TQA_IMAGE_TAG)")
class DesktopBrowserPolicy(unittest.TestCase):
    """REL-I-004 (the c16k real-world run, 17 Oct 2026): the Desktop's browser offered "Save password?" and "Save
    address?" in the person's and the model's pictures, and asked content-autofill.googleapis.com about every form, a
    blocked notice in the task. The built image holds the repository's policy and switches where Chromium reads
    them."""

    def test_the_desktop_reads_the_quiet_policy_with_saving_and_autofill_off(self):
        files = ImageFiles(IMAGES["runtime-desktop"].format(TAG))
        self.addCleanup(files.close)
        shipped = files.read("/etc/chromium/policies/managed/tqa-quiet.json")
        self.assertIsNotNone(shipped, "no managed policy in the Desktop image")
        policy = json.loads(shipped)
        self.assertEqual(policy, json.loads((REPOSITORY / "images/runtime/chromium-policy.json").read_text()))
        for key in ("PasswordManagerEnabled", "AutofillAddressEnabled", "AutofillCreditCardEnabled"):
            with self.subTest(key):
                self.assertIs(policy.get(key), False)
        switches = (files.read("/etc/chromium.d/tqa-quiet") or b"").decode()
        self.assertEqual(switches, (REPOSITORY / "images/runtime/chromium-quiet-flags.sh").read_text())
        self.assertIn("AutofillServerCommunication", switches)


@unittest.skipUnless(DOCKER, "needs Docker (TQA_DOCKER_TESTS=1, TQA_IMAGE_TAG)")
class ImageFonts(unittest.TestCase):
    """REL-I-005 (the c17 canvas capture, 23 Oct 2026): 16z's Desktop had no CJK and no emoji font, so Chinese text and
    emoji drew as empty boxes for the person and the model. fontconfig, which Chromium and the Desktop's applications
    ask, finds both in the Workstation and the Desktop."""

    def fc_list(self, image, *pattern):
        # As the sandbox user, offline; stderr (fontconfig's "no writable cache" notes) is not the answer.
        return docker("run", "--rm", "--network", "none", "--user", "1000:1000", "--entrypoint", "fc-list", image,
                      *pattern)

    def test_the_workstation_and_desktop_have_a_chinese_font_and_noto_color_emoji(self):
        for name in ("runtime-workstation", "runtime-desktop"):
            image = IMAGES[name].format(TAG)
            with self.subTest(name, font="zh"):
                self.assertNotEqual(self.fc_list(image, ":lang=zh", "family"), "", "no font for Chinese")
            with self.subTest(name, font="emoji"):
                families = self.fc_list(image, ":", "family").splitlines()
                self.assertIn("Noto Color Emoji", families)


@unittest.skipUnless(DOCKER and ROOTFS_DIR and KERNEL_DIR,
                     "needs Docker and the built Firecracker artefacts (TQA_FC_ROOTFS_DIR, TQA_FC_KERNEL_DIR)")
class FirecrackerArtefacts(unittest.TestCase):
    def test_each_template_rootfs_is_its_source_image_plus_the_guest_init(self):
        init = {(crate.name, crate.version) for crate in sbom.crates(artefacts.Artefact(
            name="tqa-vm-init", version="", crates=[artefacts.Crate("tqa-vm-init", artefacts.MUSL)], own_paths=[]))}
        for template in ("base", "workstation", "desktop"):
            with self.subTest(template):
                image = "tqa-runtime:{}-{}".format(template, TAG)
                image_document, problems, image_id = sbom.generate("runtime-" + template, image, "0.0.0-test", 0)
                self.assertEqual(problems, [])
                directory = Path(ROOTFS_DIR) / image_id.removeprefix("sha256:")
                manifest = firecracker.rootfs_manifest(directory)
                self.assertEqual(manifest["source_image"]["id"], image_id)
                document, problems, digest = sbom.generate_rootfs("rootfs-" + template, directory, "0.0.0-test", 0)
                self.assertEqual(problems, [])
                self.assertEqual(digest, "sha256:" + manifest["rootfs"]["sha256"])
                extra = listed(document) - listed(image_document)
                self.assertEqual({name for name, _ in extra} - {name for name, _ in init}, {"musl"})
                self.assertEqual(listed(image_document) - listed(document), set())

    def test_the_kernel_links_its_exact_tarball_and_publishes_its_config(self):
        document, problems, digest = sbom.generate_kernel(KERNEL_DIR, 0)
        self.assertEqual(problems, [])
        self.assertEqual(digest, "sha256:" + firecracker.sha256_file(Path(KERNEL_DIR) / "vmlinux"))
        with tempfile.TemporaryDirectory() as out:
            sbom.write(Path(out), "kernel", document, digest, kernel_dir=KERNEL_DIR)
            entry = json.loads((Path(out) / "index.json").read_text(encoding="utf-8"))["kernel"]
            self.assertEqual((Path(out) / entry["config"]).read_bytes(), (Path(KERNEL_DIR) / "config").read_bytes())
        pins = firecracker.kernel_pins()
        self.assertEqual((entry["source"], entry["source_sha256"]), (
            "https://cdn.kernel.org/pub/linux/kernel/v6.x/linux-{}.tar.xz".format(pins.version), pins.source_sha256))


if __name__ == "__main__":
    unittest.main()
