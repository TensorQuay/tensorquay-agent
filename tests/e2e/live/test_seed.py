"""TK-U-017: the live suite's secret seed never shows a value. Synthetic values only.

Run: python3 -m unittest discover -s tests/e2e/live -p 'test_*.py' -v
"""
import os
from pathlib import Path
import stat
import subprocess
import sys
import tempfile
import tomllib
import unittest

import seed

SCRIPT = Path(__file__).resolve().parent / "seed.py"
ONE = "synthetic-live-value-one-7f3a"
TWO = "synthetic-live-value-two-91c2"
ENV = "# owner's file\nFIRST_KEY={}\nexport SECOND_KEY='{}'\nUNUSED_BASE_URL=http://proxy.invalid\n".format(ONE, TWO)


class SeedTest(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary.name)
        self.env = self.root / "keys.env"
        self.env.write_text(ENV)
        self.env.chmod(0o600)

    def tearDown(self):
        self.temporary.cleanup()

    def run_cli(self, *routes, env_file=None, out="seed"):
        arguments = ["--env-file", str(env_file or self.env), "--out-dir", str(self.root / out)]
        for route in routes:
            arguments += ["--route", route]
        return subprocess.run([sys.executable, str(SCRIPT), *arguments], capture_output=True, text=True, timeout=30,
                              check=False)

    def assert_no_value(self, *texts):
        for text in texts:
            for value in (ONE, TWO):
                self.assertNotIn(value, text)

    def test_tk_u_017_a_private_seed_with_one_entry_per_route(self):
        result = self.run_cli("p1-chat=FIRST_KEY", "p3-responses=SECOND_KEY")
        self.assertEqual(result.returncode, 0, result.stderr)
        path = Path(result.stdout.strip())
        self.assertEqual(path, self.root / "seed" / "secrets.toml")
        self.assertEqual(stat.S_IMODE(path.stat().st_mode), 0o600)
        self.assertEqual(stat.S_IMODE(path.parent.stat().st_mode), 0o700)
        self.assertEqual(tomllib.loads(path.read_text()),
                         {"secrets": {"secrets/live-p1-chat": ONE, "secrets/live-p3-responses": TWO}})
        self.assert_no_value(result.stdout, result.stderr)

    def test_tk_u_017_bad_input_is_refused_without_a_value(self):
        shared = self.root / "shared.env"
        shared.write_text(ENV)
        shared.chmod(0o644)
        bad_line = self.root / "bad.env"
        bad_line.write_text("FIRST_KEY {}\n".format(ONE))
        bad_line.chmod(0o600)
        control = self.root / "control.env"
        control.write_text("FIRST_KEY={}\x07\n".format(ONE))
        control.chmod(0o600)
        cases = [
            (("p1-chat=MISSING_KEY",), None, "the env file has no value for MISSING_KEY"),
            (("p1-chat=FIRST_KEY",), shared, "the env file must be this user's and mode 0600"),
            (("p1-chat=FIRST_KEY",), self.root / "absent.env", "the env file does not exist"),
            (("p1-chat=FIRST_KEY",), bad_line, "line 1 of the env file is not NAME=value"),
            (("p1-chat=FIRST_KEY",), control, "the value of FIRST_KEY contains a control character"),
            (("p1-chat=FIRST_KEY", "p1-chat=SECOND_KEY"), None, "a route is named twice"),
            (("P1=FIRST_KEY",), None, "a route is <name>=<VARIABLE>: lowercase name, uppercase variable"),
            ((), None, "at least one route is required"),
        ]
        for index, (routes, env_file, reason) in enumerate(cases):
            with self.subTest(reason):
                result = self.run_cli(*routes, env_file=env_file, out="seed-{}".format(index))
                self.assertEqual(result.returncode, 2)
                self.assertEqual(result.stderr, "seed: " + reason + "\n")
                self.assertEqual(result.stdout, "")
                self.assertFalse((self.root / "seed-{}".format(index)).exists())
                self.assert_no_value(result.stdout, result.stderr)

    def test_tk_u_017_an_existing_output_directory_is_refused(self):
        (self.root / "seed").mkdir()
        result = self.run_cli("p1-chat=FIRST_KEY")
        self.assertEqual((result.returncode, result.stderr), (2, "seed: the output directory must be new\n"))
        self.assertEqual(os.listdir(self.root / "seed"), [])


if __name__ == "__main__":
    unittest.main()
