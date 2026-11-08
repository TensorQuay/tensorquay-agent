"""SBX-F-012: host-setup.sh --check against fake host roots, and refusal of root actions."""

import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

SCRIPT = Path(__file__).resolve().parent / "host-setup.sh"
USER, UID, SUBUID_START, SUBGID_START = "svc", 1500, 200000, 300000
# Container uid and gid 10000 map to the subordinate ranges' starts plus 9999 under rootless Docker.
VMM_HOST_UID, VMM_HOST_GID = SUBUID_START + 9999, SUBGID_START + 9999
UNIT_DIR = "etc/systemd/system"
# The oracle for what --apply installs: the reviewed units, with neutral descriptions. logind resets device ACLs at
# seat setup, after tmpfiles and udev, so the grant runs after logind and is re-applied by a timer.
SERVICE = f"""[Unit]
Description=TensorQuay Agent: grant /dev/kvm to the Firecracker VMM uid {VMM_HOST_UID} (ADR-0006)
After=systemd-logind.service display-manager.service systemd-udev-settle.service
ConditionPathExists=/dev/kvm

[Service]
Type=oneshot
ExecStart=/usr/bin/setfacl -m u:{VMM_HOST_UID}:rw,g:{VMM_HOST_GID}:rw /dev/kvm
"""
TIMER = f"""[Unit]
Description=TensorQuay Agent: re-apply the /dev/kvm ACL for uid {VMM_HOST_UID} after a logind reset

[Timer]
OnBootSec=1min
OnUnitActiveSec=30s

[Install]
WantedBy=timers.target
"""


def write(root, relative, text):
    path = root / relative
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text)
    return path


def provisioned(root, kernel="6.18.54-generic"):
    """A fake host with every requirement met."""
    write(root, "etc/passwd", f"{USER}:x:{UID}:{UID}::/nonexistent:/usr/sbin/nologin\n")
    write(root, "etc/subuid", f"other:100000:65536\n{USER}:{SUBUID_START}:65536\n")
    write(root, "etc/subgid", f"other:100000:65536\n{USER}:{SUBGID_START}:65536\n")
    write(root, "proc/cpuinfo", "flags\t\t: fpu vme vmx sse\n")
    write(root, "proc/sys/kernel/osrelease", kernel + "\n")
    write(root, "dev/net/tun", "")
    kvm = write(root, "dev/kvm", "")
    subprocess.run(["setfacl", "-m", f"u:{VMM_HOST_UID}:rw,g:{VMM_HOST_GID}:rw", str(kvm)], check=True)
    write(root, f"{UNIT_DIR}/tqa-kvm-acl.service", SERVICE)
    write(root, f"{UNIT_DIR}/tqa-kvm-acl.timer", TIMER)
    wants = root / UNIT_DIR / "timers.target.wants"
    wants.mkdir()
    (wants / "tqa-kvm-acl.timer").symlink_to("../tqa-kvm-acl.timer")
    service = f"sys/fs/cgroup/user.slice/user-{UID}.slice/user@{UID}.service/cgroup.controllers"
    write(root, service, "cpu memory pids\n")


class HostSetupCheck(unittest.TestCase):
    def setUp(self):
        self.root = Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, self.root)

    def run_script(self, *args, fake_root=True):
        env = {"PATH": os.environ["PATH"], "LC_ALL": "C"}
        if fake_root:
            env["TQA_HOST_ROOT"] = str(self.root)
        return subprocess.run(
            ["bash", str(SCRIPT), *args], env=env, capture_output=True, text=True, timeout=30, check=False
        )

    def test_a_user_grant_alone_is_not_enough_because_logind_flushes_user_entries(self):
        # logind's uaccess reset deletes every ACL_USER entry on each change event (one per VM created or destroyed);
        # a VM starting in the gap before the re-grant failed with EACCES. The group entry survives the reset.
        provisioned(self.root)
        subprocess.run(["setfacl", "-x", f"g:{VMM_HOST_GID}", str(self.root / "dev/kvm")], check=True)
        result = self.run_script("--check", "--user", USER)
        self.assertEqual(result.returncode, 1)
        self.assertIn(f"MISSING /dev/kvm group access for gid {VMM_HOST_GID}", result.stdout)
        self.assertIn("--apply --user " + USER, result.stdout)

    def test_a_missing_subordinate_gid_range_is_named(self):
        provisioned(self.root)
        (self.root / "etc/subgid").write_text("other:100000:65536\n")
        result = self.run_script("--check", "--user", USER)
        self.assertIn("MISSING subordinate gid range", result.stdout)

    def test_provisioned_host_passes(self):
        provisioned(self.root)
        result = self.run_script("--check", "--user", USER)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertNotIn("MISSING", result.stdout)
        self.assertIn(f"u:{VMM_HOST_UID}:rw", result.stdout)

    def test_check_is_the_default_mode(self):
        provisioned(self.root)
        self.assertEqual(self.run_script("--user", USER).returncode, 0)

    def test_each_missing_item_is_named_with_its_remedy(self):
        provisioned(self.root)
        cases = {
            "proc/cpuinfo": "hardware virtualisation",
            "dev/kvm": "/dev/kvm",
            "dev/net/tun": "/dev/net/tun",
            f"{UNIT_DIR}/tqa-kvm-acl.service": "tqa-kvm-acl.service",
            f"{UNIT_DIR}/tqa-kvm-acl.timer": "tqa-kvm-acl.timer",
        }
        for relative, item in cases.items():
            with self.subTest(relative):
                backup = (self.root / relative).read_bytes()
                (self.root / relative).unlink()
                result = self.run_script("--check", "--user", USER)
                self.assertEqual(result.returncode, 1, result.stdout)
                missing = [line for line in result.stdout.splitlines() if line.startswith("MISSING")]
                # A missing /dev/kvm also misses both of its grants.
                self.assertEqual(len(missing), 1 + 2 * (relative == "dev/kvm"), result.stdout)
                self.assertIn(item, missing[0])
                self.assertIn("remedy:", missing[0])
                (self.root / relative).write_bytes(backup)
                if relative == "dev/kvm":
                    subprocess.run(["setfacl", "-m", f"u:{VMM_HOST_UID}:rw,g:{VMM_HOST_GID}:rw",
                                    str(self.root / relative)], check=True)

    def test_missing_acl_names_the_vmm_uid_and_apply(self):
        provisioned(self.root)
        subprocess.run(["setfacl", "-b", str(self.root / "dev/kvm")], check=True)
        result = self.run_script("--check", "--user", USER)
        self.assertEqual(result.returncode, 1)
        self.assertRegex(result.stdout, rf"MISSING /dev/kvm access for uid {VMM_HOST_UID}.*remedy:.*--apply")

    def test_acl_for_another_uid_does_not_count(self):
        provisioned(self.root)
        kvm = str(self.root / "dev/kvm")
        subprocess.run(["setfacl", "-b", kvm], check=True)
        subprocess.run(["setfacl", "-m", f"u:{VMM_HOST_UID + 1}:rw", kvm], check=True)
        self.assertEqual(self.run_script("--check", "--user", USER).returncode, 1)

    def test_undelegated_controllers_are_named(self):
        provisioned(self.root, kernel="6.1.100")
        service = f"sys/fs/cgroup/user.slice/user-{UID}.slice/user@{UID}.service/cgroup.controllers"
        write(self.root, service, "cpu pids\n")
        result = self.run_script("--check", "--user", USER)
        self.assertEqual(result.returncode, 1)
        self.assertRegex(result.stdout, r"MISSING cgroup controller memory .*remedy:")

    def test_short_or_absent_subordinate_range_is_named(self):
        provisioned(self.root)
        for content in ("", f"{USER}:{SUBUID_START}:9999\n"):
            with self.subTest(content=content):
                write(self.root, "etc/subuid", content)
                result = self.run_script("--check", "--user", USER)
                self.assertEqual(result.returncode, 1)
                self.assertRegex(result.stdout, r"MISSING subordinate uid range .*remedy:")

    def test_provisioned_host_has_no_warning_but_the_kernel(self):
        provisioned(self.root)
        self.assertNotIn("WARN", self.run_script("--check", "--user", USER).stdout)

    def test_drifted_units_are_named_with_apply_as_remedy(self):
        for name, text in (("tqa-kvm-acl.service", SERVICE), ("tqa-kvm-acl.timer", TIMER)):
            with self.subTest(name):
                provisioned(self.root)
                write(self.root, f"{UNIT_DIR}/{name}", text.replace("TensorQuay Agent", "Local team"))
                result = self.run_script("--check", "--user", USER)
                self.assertEqual(result.returncode, 1)
                self.assertRegex(result.stdout, rf"MISSING {name} .*remedy:.*--apply")
                shutil.rmtree(self.root)
                self.root.mkdir()

    def test_disabled_timer_is_named(self):
        provisioned(self.root)
        (self.root / UNIT_DIR / "timers.target.wants" / "tqa-kvm-acl.timer").unlink()
        result = self.run_script("--check", "--user", USER)
        self.assertEqual(result.returncode, 1)
        self.assertRegex(result.stdout, r"MISSING tqa-kvm-acl.timer enabled.*remedy:.*--apply")

    def test_legacy_tmpfiles_grant_warns_without_failing(self):
        provisioned(self.root)
        write(self.root, "etc/tmpfiles.d/tqa-kvm.conf", f"a+ /dev/kvm - - - - u:{VMM_HOST_UID}:rw\n")
        result = self.run_script("--check", "--user", USER)
        self.assertEqual(result.returncode, 0, result.stdout)
        self.assertRegex(result.stdout, r"WARN /etc/tmpfiles.d/tqa-kvm.conf .*--apply removes it")

    def test_untested_host_kernel_warns_without_failing(self):
        provisioned(self.root, kernel="7.0.0-34-generic")
        result = self.run_script("--check", "--user", USER)
        self.assertEqual(result.returncode, 0, result.stdout)
        self.assertRegex(result.stdout, r"WARN host kernel 7\.0 is outside")

    def test_unknown_user_is_refused(self):
        provisioned(self.root)
        result = self.run_script("--check", "--user", "nobody-here")
        self.assertEqual(result.returncode, 2)
        self.assertIn("unknown user", result.stderr)

    def test_usage_errors(self):
        for args in (["--bogus"], ["--user"], ["--check", "--apply"]):
            with self.subTest(args=args):
                self.assertEqual(self.run_script(*args).returncode, 2)

    @unittest.skipIf(os.geteuid() == 0, "refusal is observable only as a non-root user")
    def test_root_actions_refuse_without_root_and_change_nothing(self):
        for mode in ("--apply", "--revert"):
            with self.subTest(mode=mode):
                result = self.run_script(mode, "--user", USER, fake_root=False)
                self.assertEqual(result.returncode, 2)
                self.assertIn("must run as root", result.stderr)
                self.assertEqual(result.stdout, "")

    def test_root_actions_need_an_explicit_user(self):
        for mode in ("--apply", "--revert"):
            with self.subTest(mode=mode):
                result = self.run_script(mode, fake_root=False)
                self.assertEqual(result.returncode, 2)
                self.assertIn("needs --user", result.stderr)

    def test_root_actions_refuse_a_fake_root(self):
        provisioned(self.root)
        result = self.run_script("--apply", "--user", USER)
        self.assertEqual(result.returncode, 2)
        self.assertIn("--check only", result.stderr)


if __name__ == "__main__":
    unittest.main()
