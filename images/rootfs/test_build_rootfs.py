"""SBX-F-010 rootfs build and SBX-F-005 lineage (a template's rootfs is built from its own digest: GW-I-026).
Unit tests always run; the Docker builds need TQA_DOCKER_TESTS=1
plus TQA_VMM_IMAGE (a built images/vmm target) and the runtime images tqa-runtime:base and tqa-runtime:workstation.
"""

import copy
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

import build_rootfs as rootfs

INSPECT = {
    "Id": "sha256:" + "ab" * 32,
    "Os": "linux",
    "Architecture": "amd64",
    "Created": "2026-09-25T11:25:03.123456789Z",
    "RepoDigests": [],
    "Config": {
        "User": "1000:1000",
        "Env": ["PATH=/usr/bin:/bin", "HOME=/tmp"],
        "WorkingDir": "/workspace",
        "Entrypoint": ["/usr/bin/tini", "--", "/usr/local/bin/tqa-runtime"],
        "Cmd": None,
        "Labels": {"org.tensorquay.protocol-revision": "5"},
    },
}
RESULT = "tree_sha256={}\nruntime_sha256={}\ninit_sha256={}\ne2fsprogs=1.47.2\n".format("1" * 64, "2" * 64, "3" * 64)


def with_config(**changes):
    inspect = copy.deepcopy(INSPECT)
    inspect["Config"].update(changes)
    return inspect


class Unit(unittest.TestCase):
    def test_sbx_f_010_image_configuration_is_carried_into_the_guest(self):
        self.assertEqual(rootfs.image_config(INSPECT), {
            "user": "1000:1000", "env": ["PATH=/usr/bin:/bin", "HOME=/tmp"], "working_dir": "/workspace",
            "entrypoint": ["/usr/bin/tini", "--", "/usr/local/bin/tqa-runtime"], "cmd": [],
        })
        for bad in (with_config(User="0:0"), with_config(User="runtime"), with_config(User=""),
                    with_config(User="1000"), with_config(Entrypoint=None, Cmd=None),
                    with_config(Entrypoint=["tini"]), {**INSPECT, "Architecture": "arm64"}):
            with self.subTest(bad=bad["Config"]), self.assertRaises(rootfs.BuildError):
                rootfs.image_config(bad)

    def test_sbx_f_010_derived_values_are_deterministic(self):
        self.assertEqual(rootfs.fs_uuid(INSPECT["Id"]), rootfs.fs_uuid(INSPECT["Id"]))
        self.assertNotEqual(rootfs.fs_uuid(INSPECT["Id"]), rootfs.fs_uuid("sha256:" + "cd" * 32))
        self.assertEqual(rootfs.source_date_epoch("1970-01-01T00:00:10.999999999Z"), 10)
        self.assertEqual(rootfs.source_date_epoch("1970-01-01T01:00:00+01:00"), 0)
        self.assertEqual(rootfs.protocol_revision(with_config(Labels=None)), 1)
        for bad in ("", "yesterday", "1970-01-01 00:00:00"):
            with self.assertRaises(rootfs.BuildError):
                rootfs.source_date_epoch(bad)
        with self.assertRaises(rootfs.BuildError):
            rootfs.protocol_revision(with_config(Labels={"org.tensorquay.protocol-revision": "five"}))

    def test_sbx_f_010_incomplete_results_are_refused(self):
        self.assertEqual(rootfs.parse_result(RESULT)["e2fsprogs"], "1.47.2")
        for bad in ("", RESULT.replace("init_sha256", "other"), RESULT.replace("1" * 64, "xyz"),
                    RESULT + "extra=1\n", RESULT.replace("1.47.2", "unknown")):
            with self.assertRaises(rootfs.BuildError):
                rootfs.parse_result(bad)

    def test_sbx_f_005_lineage_mismatch_is_refused(self):
        result = rootfs.parse_result(RESULT)
        rootfs.check_lineage(result, "2" * 64)
        with self.assertRaises(rootfs.BuildError):
            rootfs.check_lineage(result, "4" * 64)

    def test_sbx_f_010_manifest_fields(self):
        document = rootfs.manifest(INSPECT, rootfs.parse_result(RESULT), "5" * 64, 4096, "sha256:" + "ef" * 32)
        self.assertEqual(document["source_image"]["protocol_revision"], 5)
        self.assertEqual(document["rootfs"], {"file": "rootfs.ext4", "sha256": "5" * 64, "bytes": 4096,
                                              "tree_sha256": "1" * 64})
        self.assertEqual(document["runtime"]["sha256"], "2" * 64)
        self.assertEqual(document["builder"]["e2fsprogs"], "1.47.2")

    def test_sbx_c_014_the_manifest_records_the_image_policy_by_key_name_only(self):
        """The backend applies Docker's image policy from these; a value never lands in the manifest."""
        inspect = with_config(Env=["PATH=/usr/bin:/bin", "API_TOKEN=super-secret-value", "BARE"],
                              Volumes={"/data": {}, "/cache": {}})
        document = rootfs.manifest(inspect, rootfs.parse_result(RESULT), "5" * 64, 4096, "sha256:" + "ef" * 32)
        self.assertEqual(document["schema"], 2)
        self.assertEqual(document["image"], {"env_keys": ["API_TOKEN", "BARE", "PATH"], "volumes": ["/cache", "/data"]})
        self.assertNotIn("super-secret-value", json.dumps(document))
        plain = rootfs.manifest(INSPECT, rootfs.parse_result(RESULT), "5" * 64, 4096, "sha256:" + "ef" * 32)
        self.assertEqual(plain["image"], {"env_keys": ["HOME", "PATH"], "volumes": []})


class E2fsckRefusal(unittest.TestCase):
    """SBX-F-010: when e2fsck refuses a new image, its exit code is named and its whole report kept for diagnosis."""

    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.work, self.out = Path(temporary.name) / "work", Path(temporary.name) / "out"
        self.work.mkdir()
        self.out.mkdir()

    def test_sbx_f_010_the_exit_code_is_named_and_the_report_kept_owner_only(self):
        report = "e2fsck 1.47.2 (1-Jan-2025)\nPass 1: Checking inodes, blocks, and sizes\nInode 12 ... Fix? no\n"
        (self.work / "e2fsck.code").write_text("4\n")
        (self.work / "e2fsck.log").write_text(report)
        error = rootfs.e2fsck_refusal(self.work, self.out)
        (kept,) = self.out.glob(".e2fsck-*.log")
        self.assertEqual(str(error), f"e2fsck exited 4 on the new rootfs; its report is in {kept}")
        self.assertEqual(kept.read_text(), report)
        self.assertEqual(kept.stat().st_mode & 0o777, 0o600)

    def test_sbx_f_010_a_malformed_code_or_a_missing_report_is_still_named(self):
        (self.work / "e2fsck.code").write_text("four\n")
        self.assertEqual(str(rootfs.e2fsck_refusal(self.work, self.out)),
                         "e2fsck exited unknown on the new rootfs; it left no report")

    def test_sbx_f_010_another_failure_is_not_an_e2fsck_refusal(self):
        self.assertIsNone(rootfs.e2fsck_refusal(self.work, self.out))


@unittest.skipUnless(os.environ.get("TQA_DOCKER_TESTS") == "1", "needs Docker (TQA_DOCKER_TESTS=1)")
class DockerBuild(unittest.TestCase):
    """Builds real images; each build takes minutes."""

    def setUp(self):
        self.vmm = os.environ.get("TQA_VMM_IMAGE", "")
        if not self.vmm:
            self.fail("TQA_VMM_IMAGE must name a built images/vmm image")
        self.out = Path(tempfile.mkdtemp(prefix="rootfs-test-"))
        self.addCleanup(shutil.rmtree, self.out)

    def debugfs(self, image, command):
        return rootfs.docker("run", "--rm", "--network", "none", "--mount",
                             f"type=bind,src={image.parent},dst=/in,readonly", "--entrypoint", "debugfs",
                             self.vmm, "-R", command, f"/in/{image.name}").decode()

    def assert_posture(self, target, source):
        document = json.loads((target / "manifest.json").read_text())
        image = target / "rootfs.ext4"
        self.assertEqual(document["rootfs"]["sha256"], rootfs.sha256_file(image))
        self.assertEqual(document["runtime"]["sha256"], rootfs.image_runtime_sha256(source))
        self.assertRegex(self.debugfs(image, "stat /workspace"), r"User:\s+1000\s+Group:\s+1000")
        self.assertRegex(self.debugfs(image, "stat /usr/local/bin/tqa-runtime"), r"User:\s+0\s+Group:\s+0")
        self.assertRegex(self.debugfs(image, "stat /usr/sbin/tqa-vm-init"), r"Mode:\s+0755")
        recorded = json.loads(self.debugfs(image, "cat /etc/tqa-vm/image.json"))
        self.assertEqual(recorded["user"], "1000:1000")
        return document

    def test_sbx_f_010_sbx_f_005_base_builds_reproducibly(self):
        documents = []
        for name in ("a", "b"):
            (self.out / name).mkdir()
            target = rootfs.build("tqa-runtime:base", self.vmm, self.out / name)
            documents.append(self.assert_posture(target, "tqa-runtime:base"))
        self.assertEqual(documents[0], documents[1])

    def test_sbx_f_010_sbx_f_005_workstation_builds(self):
        self.assert_posture(rootfs.build("tqa-runtime:workstation", self.vmm, self.out), "tqa-runtime:workstation")

    def test_sbx_f_010_existing_output_is_not_replaced(self):
        target = rootfs.build("tqa-runtime:base", self.vmm, self.out)
        before = subprocess.run(["ls", "-l", str(target)], capture_output=True, check=True).stdout
        with self.assertRaises(rootfs.BuildError):
            rootfs.build("tqa-runtime:base", self.vmm, self.out)
        self.assertEqual(before, subprocess.run(["ls", "-l", str(target)], capture_output=True, check=True).stdout)


if __name__ == "__main__":
    unittest.main()
