"""SBX-F-019: kvm-no-uaccess.sh --check against fake host roots and a fake udevadm, and refusal of root actions (owner
decision (b)). On systemd 255 the seat's uaccess builtin still runs for /dev/kvm (TAG== matches the sticky TAGS), so the
check also requires the re-grant rule and that udev schedules it after the builtin."""

import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

SCRIPT = Path(__file__).resolve().parent / "kvm-no-uaccess.sh"
RULE_FILE = "etc/udev/rules.d/72-tqa-kvm-no-uaccess.rules"
RULE = """# TensorQuay Agent (ADR-0006): logind must not manage /dev/kvm's ACL, so the Firecracker VMM's grant persists.
SUBSYSTEM=="misc", KERNEL=="kvm", TAG-="uaccess"
"""
GRANT = "/usr/bin/setfacl -m u:175535:rw,g:175535:rw /dev/kvm"
# The grant before the group entry; a host between the two updates still reads its unit.
USER_ONLY_GRANT = "/usr/bin/setfacl -m u:175535:rw /dev/kvm"
REGRANT_FILE = "etc/udev/rules.d/74-tqa-kvm-acl.rules"
REGRANT = f"""# TensorQuay Agent (ADR-0006): re-grant the Firecracker VMM after the seat's uaccess builtin, in the same event.
SUBSYSTEM=="misc", KERNEL=="kvm", ACTION!="remove", RUN+="{GRANT}"
"""
UNIT_FILE = "etc/systemd/system/tqa-kvm-acl.service"
UNIT = f"[Unit]\nDescription=grant\n\n[Service]\nType=oneshot\nExecStart={GRANT}\n"


class Check(unittest.TestCase):
    def setUp(self):
        self.root = Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, self.root)
        self.fake = self.root / "udevadm"

    def run_script(self, *args, fake_root=True, udevadm=None):
        env = {"PATH": os.environ["PATH"], "LC_ALL": "C"}
        if fake_root:
            env["TQA_HOST_ROOT"] = str(self.root)
        if udevadm is not None:
            env["TQA_UDEVADM"] = str(udevadm)
        return subprocess.run(["bash", str(SCRIPT), *args], env=env, capture_output=True, text=True, timeout=30,
                              check=False)

    def write(self, name, text):
        path = self.root / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text)

    def write_host(self):
        for name, text in ((RULE_FILE, RULE), (REGRANT_FILE, REGRANT), (UNIT_FILE, UNIT)):
            self.write(name, text)

    def write_udevadm(self, current=":seat:", runs=("uaccess", GRANT)):
        """A fake udevadm: `info` prints the device's tags, `test` the programs udev would run, in order."""
        self.write("udevadm-info", f"DEVNAME=/dev/kvm\nTAGS=:uaccess:seat:\nCURRENT_TAGS={current}\n")
        listed = "".join(f"run: '{run}'\n" for run in runs)
        self.write("udevadm-test", f"Reading rules file: /{REGRANT_FILE}\n{listed}")
        self.fake.write_text(f'#!/bin/sh\ncat "{self.root}/udevadm-$1"\n')
        self.fake.chmod(0o755)

    def test_installed_rules_pass(self):
        self.write_host()
        result = self.run_script("--check")
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertRegex(result.stdout, r"ok +/etc/udev/rules.d/72-tqa-kvm-no-uaccess.rules installed")
        self.assertRegex(result.stdout, r"ok +/etc/udev/rules.d/74-tqa-kvm-acl.rules installed")

    def test_missing_or_different_rules_are_named_with_apply(self):
        cases = ((RULE_FILE, None), (RULE_FILE, RULE.replace("TAG-=", "TAG+=")),
                 (REGRANT_FILE, None), (REGRANT_FILE, REGRANT.replace("175535", "175536")))
        for name, text in cases:
            with self.subTest(name=name, text=text):
                self.write_host()
                if text is None:
                    (self.root / name).unlink()
                else:
                    self.write(name, text)
                result = self.run_script("--check")
                self.assertEqual(result.returncode, 1)
                self.assertRegex(result.stdout, rf"MISSING /{name} missing or different; remedy:.*--apply")

    def test_the_regrant_follows_the_host_setup_unit(self):
        """The rule runs the unit's own grant, so the VMM uid is defined once, by host-setup.sh."""
        self.write_host()
        (self.root / UNIT_FILE).unlink()
        result = self.run_script("--check")
        self.assertEqual(result.returncode, 1)
        self.assertRegex(result.stdout, r"MISSING tqa-kvm-acl.service.*remedy:.*host-setup.sh --apply")
        self.write(UNIT_FILE, UNIT.replace("175535", "175536"))
        result = self.run_script("--check")
        self.assertEqual(result.returncode, 1)
        self.assertRegex(result.stdout, r"MISSING /etc/udev/rules.d/74-tqa-kvm-acl.rules missing or different")

    def test_a_unit_with_the_older_user_only_grant_is_still_read(self):
        self.write_host()
        self.write(UNIT_FILE, UNIT.replace(GRANT, USER_ONLY_GRANT))
        self.write(REGRANT_FILE, REGRANT.replace(GRANT, USER_ONLY_GRANT))
        self.write_udevadm(runs=("uaccess", USER_ONLY_GRANT))
        result = self.run_script("--check")
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)

    def test_udev_must_schedule_the_regrant_after_the_uaccess_builtin(self):
        self.write_host()
        cases = ((("uaccess", GRANT), 0), ((GRANT,), 0), (("uaccess",), 1), ((GRANT, "uaccess"), 1), ((), 1))
        for runs, expected in cases:
            with self.subTest(runs=runs):
                self.write_udevadm(runs=runs)
                result = self.run_script("--check", udevadm=self.fake)
                self.assertEqual(result.returncode, expected, result.stdout + result.stderr)
                pattern = r"ok +udev re-grants /dev/kvm after" if expected == 0 else r"MISSING udev does not re-grant"
                self.assertRegex(result.stdout, pattern)

    def test_only_the_current_tags_decide_the_uaccess_check(self):
        """TAGS keeps every tag a device ever had; only CURRENT_TAGS shows what udev applies now."""
        self.write_host()
        for current, expected in ((":seat:", 0), (":uaccess:seat:", 1)):
            with self.subTest(current=current):
                self.write_udevadm(current=current)
                result = self.run_script("--check", udevadm=self.fake)
                self.assertEqual(result.returncode, expected, result.stdout + result.stderr)
                pattern = r"ok +/dev/kvm carries no uaccess tag" if expected == 0 else r"MISSING /dev/kvm still carries"
                self.assertRegex(result.stdout, pattern)

    def test_usage_errors(self):
        for args in (["--bogus"], ["--check", "--apply"]):
            with self.subTest(args=args):
                self.assertEqual(self.run_script(*args).returncode, 2)

    @unittest.skipIf(os.geteuid() == 0, "refusal is observable only as a non-root user")
    def test_root_actions_refuse_without_root_and_change_nothing(self):
        for mode in ("--apply", "--revert"):
            with self.subTest(mode=mode):
                result = self.run_script(mode, fake_root=False)
                self.assertEqual(result.returncode, 2)
                self.assertIn("must run as root", result.stderr)
                self.assertEqual(result.stdout, "")

    def test_root_actions_refuse_a_fake_root(self):
        result = self.run_script("--apply")
        self.assertEqual(result.returncode, 2)
        self.assertIn("--check only", result.stderr)


if __name__ == "__main__":
    unittest.main()
