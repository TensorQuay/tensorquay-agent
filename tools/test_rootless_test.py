"""tools/rootless-test.sh: it enters only the rootless daemon's user namespace by default, and also its detached network
namespace only when TQA_ROOTLESS_NETNS=daemon asks for it (RT-I-023). Docker and nsenter are stubs that record
their arguments, so this needs neither."""

import os
import stat
import subprocess
import tempfile
import unittest
from pathlib import Path

SCRIPT = Path(__file__).with_name("rootless-test.sh")


class RootlessTestTests(unittest.TestCase):
    def setUp(self):
        self.directory = Path(tempfile.mkdtemp())
        self.runtime = self.directory / "run"
        (self.runtime / "dockerd-rootless").mkdir(parents=True)
        (self.runtime / "docker.pid").write_text("4242\n")
        self.bin = self.directory / "bin"
        self.bin.mkdir()
        self.calls = self.directory / "nsenter.args"
        self.stub("docker", "printf '%s\\n' '[\"name=seccomp,profile=builtin\",\"name=rootless\"]'")
        self.stub("nsenter", f"printf '%s\\n' \"$@\" > {self.calls}")

    def stub(self, name, body):
        path = self.bin / name
        path.write_text("#!/bin/sh\n" + body + "\n")
        path.chmod(path.stat().st_mode | stat.S_IXUSR)

    def run_script(self, netns=None):
        env = {"PATH": f"{self.bin}:/usr/bin:/bin", "XDG_RUNTIME_DIR": str(self.runtime)}
        if netns is not None:
            env["TQA_ROOTLESS_NETNS"] = netns
        return subprocess.run(["bash", str(SCRIPT), "cargo", "test"], env=env, capture_output=True, text=True)

    def arguments(self):
        return self.calls.read_text().split("\n")[:-1]

    def test_by_default_only_the_user_namespace_is_entered(self):
        result = self.run_script()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.arguments(), ["--user", "--preserve-credentials", "--target", "4242", "--", "cargo", "test"])

    def test_daemon_also_joins_the_detached_network_namespace(self):
        # rootlesskit's child lives in the containers' network namespace; this process stands in for it.
        child = os.getpid()
        (self.runtime / "dockerd-rootless" / "child_pid").write_text(f"{child}\n")
        result = self.run_script("daemon")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.arguments(), ["--user", "--preserve-credentials", "--target", "4242",
                                            f"--net=/proc/{child}/ns/net", "--", "cargo", "test"])

    def test_daemon_without_a_detached_namespace_is_refused(self):
        result = self.run_script("daemon")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("detached network namespace", result.stderr)
        self.assertFalse(self.calls.exists(), "nothing runs")

    def test_daemon_with_a_malformed_child_is_refused(self):
        (self.runtime / "dockerd-rootless" / "child_pid").write_text("not-a-pid\n")
        result = self.run_script("daemon")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("detached network namespace", result.stderr)
        self.assertFalse(self.calls.exists(), "nothing runs")

    def test_any_other_value_is_refused(self):
        for value in ["host", "yes", "1"]:
            result = self.run_script(value)
            self.assertNotEqual(result.returncode, 0, value)
            self.assertIn("TQA_ROOTLESS_NETNS", result.stderr)
            self.assertFalse(self.calls.exists(), value)


if __name__ == "__main__":
    unittest.main()
