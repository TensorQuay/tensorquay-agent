"""The squashed public import: one commit with exactly the source tree, on the public main or its first import, built
only when the guard's checks pass on the tree and the message, and never pushed.

Run: python3 -m unittest discover -s tools/release -p 'test_*.py' -v
"""

import importlib.util
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest import mock

SPEC = importlib.util.spec_from_file_location("squash_import", Path(__file__).with_name("squash-import.py"))
squash = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(squash)

# Internal references are assembled, so this file itself passes the guard audit it tests.
LEAD_LABEL = "(lead " + "decision)"
TEAM_SCRIPT = "~/" + "team/stack" + ".sh"
HOME_PATH = "/" + "home/someone/x"
MESSAGE = "feat: public import of the connectors and profiles work\n\nAgent profiles and MCP connectors.\n"


def run(repo, *args, env=None):
    return subprocess.run(["git", *args], cwd=repo, capture_output=True, text=True, check=True,
                          env=dict(os.environ, **(env or {}))).stdout.strip()


class Import(unittest.TestCase):
    """A repository whose public main is a first import plus one commit with internal history, and a source commit."""

    def setUp(self):
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        self.repo = Path(directory.name) / "repo"
        self.repo.mkdir()
        run(self.repo, "init", "-q", "-b", "main")
        identity = {"GIT_AUTHOR_NAME": "TensorQuay", "GIT_AUTHOR_EMAIL": "admin@tensorquay.com",
                    "GIT_COMMITTER_NAME": "TensorQuay", "GIT_COMMITTER_EMAIL": "admin@tensorquay.com"}
        self.commit = lambda message: (run(self.repo, "add", "-A"),
                                       run(self.repo, "commit", "-q", "--no-verify", "-m", message, env=identity),
                                       run(self.repo, "rev-parse", "HEAD"))[2]
        self.write("README.md", "An agent harness.\n")
        self.first = self.commit("feat: first import")
        self.write("src/lib.rs", "pub fn one() {}\n")
        self.public = self.commit("Merge a work branch into the integration branch")
        run(self.repo, "update-ref", "refs/remotes/origin/main", self.public)
        self.write("src/lib.rs", "pub fn one() {}\npub fn two() {}\n")
        self.source = self.commit("feat: two")
        lists = Path(directory.name)
        (lists / "deny").write_text("zqxperson\n")
        (lists / "internal").write_text("build-box-7\n")
        patches = [mock.patch.object(squash, "REPOSITORY", self.repo),
                   mock.patch.dict(os.environ, {"TQ_GUARD_DENYLIST": str(lists / "deny"),
                                                "TQ_GUARD_INTERNAL": str(lists / "internal")})]
        for patch in patches:
            patch.start()
            self.addCleanup(patch.stop)

    def write(self, relative, text):
        path = self.repo / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text)

    def build(self, **options):
        return squash.build(self.source, options.pop("message", MESSAGE), secrets=False, **options)

    def test_one_commit_with_the_source_tree_on_the_public_main(self):
        commit, parent, lines = self.build(ref="refs/heads/import/main")
        self.assertEqual(parent, self.public)
        self.assertEqual(run(self.repo, "rev-parse", commit + "^{tree}"),
                         run(self.repo, "rev-parse", self.source + "^{tree}"))
        self.assertEqual(run(self.repo, "log", "-1", "--format=%an <%ae>|%cn <%ce>|%B", commit),
                         "TensorQuay <admin@tensorquay.com>|TensorQuay <admin@tensorquay.com>|" + MESSAGE.strip())
        self.assertEqual(run(self.repo, "rev-parse", "refs/heads/import/main"), commit)
        self.assertIn("a fast-forward", "\n".join(lines))
        self.assertEqual(lines[-1], "nothing was pushed")

    def test_the_first_import_mode_parents_on_the_public_root_and_names_what_a_push_replaces(self):
        commit, parent, lines = self.build(first_import=True)
        self.assertEqual(parent, self.first)
        self.assertEqual(run(self.repo, "rev-list", "--count", commit), "2")
        text = "\n".join(lines)
        self.assertIn("replaces the public main {}: its 1 commits".format(self.public[:12]), text)
        self.assertIn("--force-with-lease=main:" + self.public, text)

    def test_a_message_with_internal_references_or_the_wrong_form_is_refused(self):
        for message in ("feat: import " + LEAD_LABEL + "\n", "Import of today's work\n", "feat: import\n\nzqxperson\n",
                        "feat: import\n\nbuilt on build-box-7\n"):
            with self.subTest(message):
                with self.assertRaises(squash.Refused):
                    self.build(message=message, ref="refs/heads/import/refused")
                self.assertEqual(run(self.repo, "for-each-ref", "refs/heads/import/"), "")

    def test_a_tree_the_guard_refuses_is_not_imported(self):
        for relative, text in (("docs/run.md", "Run it through " + TEAM_SCRIPT + ".\n"),
                               ("docs/host.md", "Measured on build-box-7.\n"), ("src/path.rs", "// " + HOME_PATH + "\n")):
            with self.subTest(relative):
                self.write(relative, text)
                self.source = self.commit("feat: more")
                with self.assertRaises(squash.Refused) as refused:
                    self.build()
                self.assertIn(relative, str(refused.exception))
                (self.repo / relative).unlink()
                self.source = self.commit("fix: less")

    def release(self, level, *added):
        """A public main with two migrations, then a source that adds `added`, with the fixture at `level` (or none)."""
        for name in ("0001_users.sql", "0002_tasks.sql"):
            self.write(squash.MIGRATIONS + name, "SELECT 1;\n")
        self.public = self.commit("feat: the previous release")
        run(self.repo, "update-ref", "refs/remotes/origin/main", self.public)
        for name in added:
            self.write(squash.MIGRATIONS + name, "SELECT 1;\n")
        if level is not None:
            self.write(squash.UPGRADE_FIXTURE, level + "\nINSERT INTO users VALUES (1);\n")
        self.source = self.commit("feat: the next release")

    def test_an_upgrade_fixture_behind_the_public_main_or_without_its_level_is_refused(self):
        self.release("-- level: 1", "0003_teams.sql")
        with self.assertRaises(squash.Refused) as refused:
            self.build()
        self.assertIn("starts at migration 1, but the public main's last migration is 2",
                      str(refused.exception))
        self.write(squash.UPGRADE_FIXTURE, "INSERT INTO users VALUES (1);\n")
        self.source = self.commit("fix: the fixture without its level")
        with self.assertRaises(squash.Refused) as refused:
            self.build()
        self.assertIn("the first line must be", str(refused.exception))

    def test_an_upgrade_fixture_at_the_public_mains_last_migration_is_imported(self):
        self.release("-- level: 2", "0003_teams.sql")
        _, parent, lines = self.build()
        self.assertEqual(parent, self.public)
        self.assertIn("upgrade fixture: at migration 2, the public main's last", lines)

    def test_a_tree_without_the_upgrade_fixture_is_not_checked(self):
        self.release(None, "0003_teams.sql")
        _, parent, lines = self.build()
        self.assertEqual(parent, self.public)
        self.assertIn("upgrade fixture: not in this tree, not checked", lines)

    def test_an_upgrade_fixture_on_a_public_main_without_migrations_is_not_checked(self):
        self.write(squash.UPGRADE_FIXTURE, "-- level: 7\nINSERT INTO users VALUES (1);\n")
        self.source = self.commit("feat: the fixture first")
        _, parent, lines = self.build()
        self.assertEqual(parent, self.public)
        self.assertIn("upgrade fixture: the public main has no migrations, not checked", lines)

    def test_an_existing_ref_is_never_overwritten(self):
        self.build(ref="refs/heads/import/main")
        with self.assertRaises(squash.Refused):
            self.build(ref="refs/heads/import/main")

    def test_no_remote_command_is_ever_run(self):
        calls = []
        real = subprocess.run
        recorded = lambda args, **options: calls.append(args) or real(args, **options)
        with mock.patch.object(squash.subprocess, "run", side_effect=recorded):
            self.build(first_import=True, ref="refs/heads/import/main")
        self.assertFalse([args for args in calls if set(args) & {"push", "fetch", "pull", "remote", "ls-remote"}])
        with self.assertRaises(AssertionError):
            squash.git("push", "origin", "main")


if __name__ == "__main__":
    unittest.main()
