"""`tools/check.sh` runs every gate and ends with the list of those that failed: one failing gate (a stale coverage
report, say) never hides the gates after it. A fake `cargo` fails the format check and the pure-crate coverage run and
passes everything else; the repository's own Python gates run for real. `--member` leaves out only the
whole-workspace coverage report."""

import os
import subprocess
import tempfile
import unittest
from pathlib import Path

SCRIPT = Path(__file__).resolve().parent / "check.sh"

FAKE_CARGO = """#!/usr/bin/env bash
[[ -n ${CARGO_LOG:-} ]] && printf '%s\\n' "$*" >> "$CARGO_LOG"
case " $* " in
    *" fmt "*) exit 1 ;;
    *" llvm-cov nextest -p "*) exit 1 ;;
    *) exit 0 ;;
esac
"""


def check(*arguments, extra=None):
    """Runs check.sh with the fake cargo first on PATH (and `extra` in its environment); returns the result and every
    cargo command line it ran."""
    with tempfile.TemporaryDirectory() as directory:
        cargo = Path(directory) / "cargo"
        cargo.write_text(FAKE_CARGO)
        cargo.chmod(0o755)
        log = Path(directory) / "cargo.log"
        # Never the caller's low-disk setting: under it, a fake run would remove the real build.
        env = {name: value for name, value in os.environ.items() if name != "TQA_CHECK_LOW_DISK"}
        env.update(PATH=f"{directory}:{os.environ['PATH']}", CARGO_LOG=str(log), **(extra or {}))
        result = subprocess.run(
            ["bash", str(SCRIPT), *arguments], env=env, capture_output=True, text=True, timeout=600, check=False)
        return result, (log.read_text().splitlines() if log.exists() else [])


class AllGatesTests(unittest.TestCase):
    def test_every_gate_runs_and_every_failure_is_listed(self):
        result, _ = check("--skip-audit")
        self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
        self.assertIn("FAIL G-01", result.stderr)
        self.assertIn("FAIL G-04\n", result.stderr)
        # The gates after a failure still ran.
        for later in ["PASS G-02", "PASS G-03", "PASS G-04-clean", "PASS G-04-report", "PASS G-05"]:
            self.assertIn(later, result.stdout, result.stdout)
        last = result.stderr.strip().splitlines()[-1]
        self.assertTrue(last.startswith("FAILED "), last)
        self.assertIn("G-01", last)
        self.assertIn("G-04", last)

    def test_a_member_run_leaves_out_only_the_workspace_coverage_report(self):
        result, commands = check("--member", "--skip-audit")
        self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
        # The pure crates' threshold still runs (and fails here); the workspace report never starts.
        self.assertIn("FAIL G-04\n", result.stderr)
        self.assertTrue(any("llvm-cov nextest -p tqa-types" in line for line in commands), commands)
        self.assertFalse(any("llvm-cov nextest --workspace" in line for line in commands), commands)
        self.assertEqual(sum("llvm-cov clean" in line for line in commands), 1, commands)
        self.assertIn("SKIP G-04-report-clean, G-04-report (--member", result.stdout)
        self.assertNotIn("G-04-report\n", result.stdout + result.stderr)
        for later in ["PASS G-02", "PASS G-03", "PASS G-04-clean", "PASS G-05", "PASS G-10-secrets"]:
            self.assertIn(later, result.stdout, result.stdout)
        self.assertIn("SKIP G-10-audit", result.stdout)

    def test_a_low_disk_run_removes_the_test_build_once_the_tests_are_done(self):
        # A runner whose disk holds one workspace build at a time (CI's) sets TQA_CHECK_LOW_DISK=1: G-03's build goes
        # before the coverage gates, which build into their own directory; without it nothing is removed.
        for setting, kept in (("1", False), ("", True), ("0", True)):
            with self.subTest(setting=setting), tempfile.TemporaryDirectory() as target:
                (Path(target) / "debug" / "deps").mkdir(parents=True)
                (Path(target) / "llvm-cov-target").mkdir()
                result, _ = check("--skip-audit", extra={"CARGO_TARGET_DIR": target, "TQA_CHECK_LOW_DISK": setting})
                self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
                self.assertEqual((Path(target) / "debug").exists(), kept, result.stdout)
                self.assertTrue((Path(target) / "llvm-cov-target").exists(), "the coverage build stays")
                freed = f"FREED {target}/debug (TQA_CHECK_LOW_DISK=1)"
                if kept:
                    self.assertNotIn("FREED", result.stdout)
                else:
                    out = result.stdout
                    self.assertLess(out.index("PASS G-03"), out.index(freed), out)
                    self.assertLess(out.index(freed), out.index("PASS G-04-clean"), out)

    def test_unknown_or_mixed_arguments_are_refused_with_the_usage(self):
        for arguments in (["--kvm", "--member"], ["--member", "--kvm"], ["--fast"], ["--member", "extra"]):
            with self.subTest(arguments=arguments):
                result, commands = check(*arguments)
                self.assertEqual(result.returncode, 2, result.stdout + result.stderr)
                self.assertIn("Usage:", result.stderr)
                self.assertIn("[--skip-audit] [--member] | --kvm", result.stderr)
                self.assertEqual(commands, [], "nothing runs")


if __name__ == "__main__":
    unittest.main()
