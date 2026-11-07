"""REL-U-001/002 for the Firecracker artefacts: the guest kernel's SBOM names the exact source tarball and is checked
against the Dockerfile's pins and the built files; a rootfs is read back from its ext4 image, never trusted by name.

Run: python3 -m unittest discover -s tools/release -p 'test_*.py' -v
"""

import hashlib
import json
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

import sbom_firecracker as firecracker


def digest(data):
    return hashlib.sha256(data).hexdigest()


class Kernel(unittest.TestCase):
    """A kernel build directory as images/rootfs/kernel/build.sh leaves it: vmlinux, config and manifest.json."""

    def setUp(self):
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        self.directory = Path(directory.name)
        self.pins = firecracker.kernel_pins()
        (self.directory / "vmlinux").write_bytes(b"\x7fELF-vmlinux")
        (self.directory / "config").write_text("CONFIG_MODULES is not set\n")
        self.manifest = {
            "schema": 1, "release": self.pins.version, "licence": "GPL-2.0-only",
            "source_sha256": self.pins.source_sha256,
            "base_config": "firecracker {} microvm-kernel-ci-x86_64-6.18.config".format(self.pins.firecracker),
            "base_config_sha256": self.pins.base_config_sha256,
            "fragment_sha256": self.pins.fragment_sha256,
            "config_sha256": digest(b"CONFIG_MODULES is not set\n"),
            "vmlinux_sha256": digest(b"\x7fELF-vmlinux"),
        }
        self.save()

    def save(self):
        (self.directory / "manifest.json").write_text(json.dumps(self.manifest))

    def test_the_pins_are_read_from_the_kernel_dockerfile(self):
        self.assertEqual(self.pins.version, "6.18.54")
        self.assertEqual(self.pins.source_sha256, "9df30b02dd8102bbd0be52556288ef6889ddbe7f1ddb96fbf847d0becf3eacac")
        self.assertEqual(len(self.pins.fragment_sha256), 64)

    def test_linux_is_listed_with_its_exact_tarball_and_checksum(self):
        linux = firecracker.kernel_package(self.pins)
        self.assertEqual((linux.name, linux.version, linux.license), ("linux", "6.18.54", "GPL-2.0-only"))
        self.assertEqual(linux.download_location, "https://cdn.kernel.org/pub/linux/kernel/v6.x/linux-6.18.54.tar.xz")
        self.assertEqual(linux.sha256, self.pins.source_sha256)

    def test_a_matching_build_passes(self):
        self.assertEqual(firecracker.kernel_problems(self.directory, self.pins), [])

    def test_a_vmlinux_or_config_other_than_the_manifests_fails(self):
        (self.directory / "vmlinux").write_bytes(b"\x7fELF-other")
        (self.directory / "config").write_text("CONFIG_MODULES=y\n")
        problems = "\n".join(firecracker.kernel_problems(self.directory, self.pins))
        self.assertIn("C6: vmlinux", problems)
        self.assertIn("C6: config", problems)

    def test_a_build_from_other_sources_than_the_pins_fails(self):
        for key in ("release", "source_sha256", "base_config_sha256", "fragment_sha256"):
            with self.subTest(key):
                saved = self.manifest[key]
                self.manifest[key] = "0" * 64
                self.save()
                self.assertIn("C1: the manifest's {}".format(key),
                              "\n".join(firecracker.kernel_problems(self.directory, self.pins)))
                self.manifest[key] = saved
                self.save()

    def test_a_missing_file_fails_without_raising(self):
        (self.directory / "config").unlink()
        self.assertIn("C6: config is missing", firecracker.kernel_problems(self.directory, self.pins))


class Rootfs(unittest.TestCase):
    def setUp(self):
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        self.directory = Path(directory.name)

    def test_an_image_other_than_the_manifests_fails(self):
        (self.directory / "rootfs.ext4").write_bytes(b"ext4")
        manifest = {"rootfs": {"file": "rootfs.ext4", "sha256": "0" * 64}}
        self.assertEqual(firecracker.rootfs_problems(self.directory, manifest),
                         ["C6: rootfs.ext4 is not the image its manifest describes"])
        manifest["rootfs"]["sha256"] = digest(b"ext4")
        self.assertEqual(firecracker.rootfs_problems(self.directory, manifest), [])

    @unittest.skipUnless(shutil.which("mkfs.ext4") and shutil.which("debugfs"), "needs e2fsprogs")
    def test_the_tree_is_read_back_from_the_ext4_image(self):
        tree = self.directory / "tree"
        (tree / "usr/sbin").mkdir(parents=True)
        (tree / "usr/sbin/tqa-vm-init").write_bytes(b"\x7fELF-init")
        (tree / "usr/sbin/tqa-vm-init").chmod(0o755)
        (tree / "etc").mkdir()
        (tree / "etc/motd").write_text("hello\n")
        image = self.directory / "rootfs.ext4"
        subprocess.run(["mkfs.ext4", "-q", "-F", "-O", "^has_journal", "-d", str(tree), str(image), "4M"], check=True,
                       capture_output=True, timeout=60)
        out = self.directory / "out"
        out.mkdir()
        firecracker.dump_rootfs(image, out)
        self.assertEqual((out / "usr/sbin/tqa-vm-init").read_bytes(), b"\x7fELF-init")
        self.assertTrue((out / "usr/sbin/tqa-vm-init").stat().st_mode & 0o111)
        self.assertEqual((out / "etc/motd").read_text(), "hello\n")

    def test_an_unreadable_image_is_an_error(self):
        (self.directory / "rootfs.ext4").write_bytes(b"not ext4")
        with self.assertRaises(RuntimeError):
            firecracker.dump_rootfs(self.directory / "rootfs.ext4", self.directory)


if __name__ == "__main__":
    unittest.main()
