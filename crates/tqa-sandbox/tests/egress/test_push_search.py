"""SEC-019 on Docker: the in-sandbox and host-storage searches look for every form of the credential (no Docker).

Run from the repository root: PYTHONPATH=crates/tqa-sandbox/tests python3 -m unittest egress.test_push_search
"""
import base64
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

from egress import git_push, push_checks
from egress.web_connector import forms

SECRET = "synthetic-push-credential-" + "5a" * 24


def basic(user):
    return "Authorization: Basic " + base64.b64encode((user + ":" + SECRET).encode()).decode()


def run(program):
    done = subprocess.run([sys.executable, "-c", program], input=json.dumps(forms(SECRET)), capture_output=True,
                          text=True, timeout=60, check=True)
    return done.stdout


class InSandbox(unittest.TestCase):
    def tree(self):
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        root = Path(directory.name)
        for name in ("workspace", "proc/7"):
            (root / name).mkdir(parents=True)
        (root / "proc/7/environ").write_bytes(b"PATH=/usr/bin\0")
        return root

    def search(self, root):
        program = git_push.search_program(roots=[str(root / "workspace")], proc=str(root / "proc"))
        return [line for line in run(program).splitlines() if line]

    def test_a_raw_or_encoded_copy_is_found_where_it_is(self):
        root = self.tree()
        (root / "workspace/.git").mkdir()
        (root / "workspace/.git/config").write_text("[http]\n\textraHeader = " + basic("x-access-token") + "\n")
        (root / "workspace/raw.txt").write_text("token " + SECRET)
        (root / "proc/7/cmdline").write_bytes(b"git\0-c\0http.extraHeader=" + basic("git").encode() + b"\0")
        self.assertEqual(sorted(self.search(root)), sorted([str(root / "workspace/.git/config"),
                                                            str(root / "workspace/raw.txt"),
                                                            str(root / "proc/7/cmdline")]))

    def test_nothing_planted_finds_nothing(self):
        root = self.tree()
        (root / "workspace/near.txt").write_text(SECRET[:-1] + "X")
        self.assertEqual(self.search(root), [])

    def test_the_program_holds_no_credential(self):
        self.assertNotIn(SECRET[:12], git_push.search_program())
        self.assertIn("'/workspace'", git_push.search_program())


class Storage(unittest.TestCase):
    def test_every_form_is_counted_once_per_file(self):
        with tempfile.TemporaryDirectory() as root:
            (Path(root) / "clean").write_text("nothing here")
            self.assertEqual(run(push_checks.SEARCH_ROOT.format(root=root)).split(), ["searched", "1", "files;",
                                                                                      "hits", "0"])
            (Path(root) / "journal").write_text("x" + basic("oauth2") + "y")
            (Path(root) / "both").write_text(SECRET + " and " + basic("u"))
            self.assertEqual(run(push_checks.SEARCH_ROOT.format(root=root)).split(), ["searched", "3", "files;",
                                                                                      "hits", "2"])


if __name__ == "__main__":
    unittest.main()
