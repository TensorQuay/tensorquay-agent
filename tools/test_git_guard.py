"""G-10 internal references: the tree audit refuses internal process references (team roles, internal note names, bug
labels, team scripts) and the terms of a local list kept outside the repository; history is not checked for them.
G-10 identity: a new commit is made as the one company identity; history keeps the earlier one, and GitHub as
the committer of a pull request merged there.

Run: python3 -m unittest tools/test_git_guard.py
"""

import importlib.util
import os
from pathlib import Path
import tempfile
import unittest
from unittest import mock

SPEC = importlib.util.spec_from_file_location("git_guard", Path(__file__).with_name("git-guard.py"))
guard = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(guard)

ALLOW = {"path": [], "ip": set(), "personal-ok": [], "secret-ok": [], "internal-ok": []}


def internal(text, terms=(), path="src/lib.rs", allow=None):
    problems = []
    guard.check_internal(path, 7, text, list(terms), allow or ALLOW, problems)
    return problems


class Patterns(unittest.TestCase):
    def test_internal_process_references_are_refused(self):
        for text in ("// the gateway refuses it (Dev 3).", "Merge branch 'dev2/web-084'", "a dev4 image",
                     "(design-mcp-client-dev1 §5a)", "(report-068 Attachment contract)", "(owner bug 3)",
                     "Owner bug (a)", "run it through ~/team/stack.sh", "(lead decision: fixed)", "a label (lead).",
                     "(lead, 2026-09-27)", "the lead's list", "the lead's review adds"):
            with self.subTest(text):
                problems = internal(text)
                self.assertEqual(len(problems), 1, problems)
                self.assertTrue(problems[0].startswith("src/lib.rs:7: internal reference"), problems)

    def test_ordinary_words_and_documented_roles_pass(self):
        for text in ("The technical lead's review adds the visual pass.", "a Product Owner decision", "leads to",
                     "misleading", "<div className=\"design-grid\">", "getByTestId(\"design-light\")", "a dev server",
                     "devices", "report-1234 is a number", "Development (dev) builds", "tqa-gateway:dev",
                     "let (lead, run) = keyed;", "fn place(lead: Uuid)", "(lead, member)"):
            with self.subTest(text):
                self.assertEqual(internal(text), [])


class LocalList(unittest.TestCase):
    def test_a_listed_term_is_refused_in_any_case_without_being_echoed(self):
        problems = internal("Run it on the Build-Box-7 host.", terms=["build-box-7"])
        self.assertEqual(problems, ["src/lib.rs:7: internal term from the local list"])

    def test_the_list_is_read_from_the_named_file_ignoring_comments(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "guard-internal"
            path.write_text("# internal vocabulary\nBuild-Box-7\n\nwrapper.sh\n", encoding="utf-8")
            with mock.patch.dict(os.environ, {"TQ_GUARD_INTERNAL": str(path)}):
                self.assertEqual(guard.internal_terms(), ["build-box-7", "wrapper.sh"])

    def test_a_missing_list_warns_unless_required(self):
        missing = {"TQ_GUARD_INTERNAL": "/nonexistent/guard-internal"}
        with mock.patch.dict(os.environ, missing), mock.patch("sys.stderr"):
            self.assertEqual(guard.internal_terms(), [])
        with mock.patch.dict(os.environ, dict(missing, TQ_GUARD_REQUIRE_INTERNAL="1")):
            with self.assertRaises(SystemExit):
                guard.internal_terms()


class Scope(unittest.TestCase):
    def test_reviewed_exceptions_and_the_guard_files_are_skipped(self):
        allow = dict(ALLOW, **{"internal-ok": ["migrations/*.sql"]})
        self.assertEqual(internal("-- (lead decision)", path="migrations/0037.sql", allow=allow), [])
        self.assertEqual(internal("r\"owner bug\"", path="tools/git-guard.py"), [])
        self.assertTrue(internal("-- (lead decision)", path="migrations/0037.sql"))

    def test_the_per_line_checks_used_for_history_do_not_check_internal_references(self):
        problems = []
        guard.check_line("src/lib.rs", 1, "// the gateway refuses it (Dev 3, owner bug 3).", [], ALLOW, problems)
        self.assertEqual(problems, [])


class FirstLineLength(unittest.TestCase):
    """G-10 commit messages: AGENTS.md limits the whole first line to 100 characters, type and scope included."""

    def problems(self, first):
        problems = []
        guard.check_message(first + "\n\nbody\n", "msg", problems)
        return problems

    def test_a_first_line_of_100_characters_passes(self):
        first = "feat(web): " + "a" * 89
        self.assertEqual(len(first), 100)
        self.assertEqual(self.problems(first), [])

    def test_a_first_line_over_100_characters_is_refused_even_with_a_short_description(self):
        first = "feat(web): " + "a" * 90
        self.assertEqual(len(first), 101)
        self.assertEqual(len(self.problems(first)), 1)

    def test_merges_and_history_reads_are_not_held_to_the_length(self):
        problems = []
        guard.check_message("Merge " + "b" * 120, "msg", problems)
        guard.check_message("feat(web): " + "a" * 120, "old", problems, require_format=False)
        self.assertEqual(problems, [])


if __name__ == "__main__":
    unittest.main()


class Identity(unittest.TestCase):
    def test_a_new_commit_is_made_as_the_dev_identity(self):
        problems = []
        guard.check_identity("TensorQuay", "dev@tensorquay.com", "author", problems)
        self.assertEqual(problems, [])
        guard.check_identity("TensorQuay", "admin@tensorquay.com", "author", problems)
        self.assertEqual(problems, ["author is 'TensorQuay <admin@tensorquay.com>', must be 'TensorQuay <dev@tensorquay.com>'"])

    def test_github_commits_a_merged_pull_request_and_authors_nothing(self):
        problems = []
        guard.check_identity("GitHub", "noreply@github.com", "1234abcd committer", problems, history=True,
                             committer=True)
        self.assertEqual(problems, [])
        guard.check_identity("GitHub", "noreply@github.com", "1234abcd author", problems, history=True)
        guard.check_identity("GitHub", "noreply@github.com", "committer", problems, committer=True)
        self.assertEqual(len(problems), 2)

    def test_history_keeps_the_earlier_identity_and_nothing_else(self):
        problems = []
        for email in ("dev@tensorquay.com", "admin@tensorquay.com"):
            guard.check_identity("TensorQuay", email, "1234abcd author", problems, history=True)
        self.assertEqual(problems, [])
        guard.check_identity("Someone", "someone@example.com", "1234abcd author", problems, history=True)
        self.assertEqual(len(problems), 1)
