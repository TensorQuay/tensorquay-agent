"""G-13 (local tier): `tools/check.sh --kvm` refuses usage errors and names a missing requirement before it runs
anything. The suites themselves need a prepared KVM host and run only when invoked there."""

import os
from pathlib import Path
import subprocess
import unittest

SCRIPT = Path(__file__).resolve().parent / "check.sh"
REQUIRED = ("TQA_VMM_IMAGE", "TQA_FC_KERNEL_DIR", "TQA_FC_ROOTFS_DIR", "TQA_GATEWAY_IMAGE", "TQA_WEB_IMAGE",
            "TQA_RUNTIME_IMAGE", "TQA_WORKSTATION_IMAGE", "TQA_FAKE_MODEL_IMAGE")


def check(*args, **env):
    base = {"PATH": os.environ["PATH"], "HOME": os.environ.get("HOME", "/"), "LC_ALL": "C"}
    return subprocess.run(["bash", str(SCRIPT), *args], env={**base, **env}, capture_output=True, text=True,
                          timeout=30, check=False)


class KvmTier(unittest.TestCase):
    def test_usage_errors_exit_2(self):
        for args in (["--kvm", "extra"], ["--bogus"], ["--skip-audit", "--kvm"]):
            with self.subTest(args=args):
                result = check(*args)
                self.assertEqual(result.returncode, 2, result.stderr)
                self.assertIn("Usage", result.stderr)
                self.assertEqual(result.stdout, "", "nothing ran")

    def test_each_missing_requirement_is_named_before_anything_runs(self):
        for missing in REQUIRED:
            present = {name: "set" for name in REQUIRED if name != missing}
            with self.subTest(missing=missing):
                result = check("--kvm", **present)
                self.assertEqual(result.returncode, 2, result.stderr)
                self.assertIn(missing, result.stderr)
                self.assertEqual(result.stdout, "", "no gate ran")


if __name__ == "__main__":
    unittest.main()
