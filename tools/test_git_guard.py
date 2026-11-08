"""G-10 internal references: the tree audit refuses internal process references (team roles, internal note names, bug
labels, team scripts) and the terms of a local list kept outside the repository; history is not checked for them.
G-10 identity: a maintainer's new commit is made as the one company identity; history keeps the earlier one, and a
pull request GitHub merged keeps its contributor as author, GitHub as committer. A contributor's clone checks
content and messages but no identity, pushes to the contributor's fork and force-pushes the contributor's own
branches. No commit, whoever made it, has a personal identifier of the denylist as its author or committer.

Run: python3 -m unittest tools/test_git_guard.py
"""

import contextlib
import importlib.util
import io
import os
from pathlib import Path
import shutil
import subprocess
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


JANE = ("Jane Doe", "jane@example.com")
PAT = ("Pat Quinlan", "pat.quinlan@example.org")  # a name the scratch denylist below holds
TOOLS = Path(__file__).resolve().parent


def git(*args):
    """Runs git in the working directory without any hook or signing; returns its output."""
    out = subprocess.run(["git", "-c", "core.hooksPath=/dev/null", "-c", "commit.gpgsign=false", *args],
                         check=True, capture_output=True, text=True)
    return out.stdout.strip()


def commit(author, committer=None, message="docs: a note"):
    """Commits one more line of notes.md as `author` (and `committer`, the author when not given)."""
    note = Path("notes.md")
    note.write_text((note.read_text() if note.exists() else "") + "a note\n")
    git("add", "notes.md")
    committer = committer or author
    with mock.patch.dict(os.environ, {"GIT_AUTHOR_NAME": author[0], "GIT_AUTHOR_EMAIL": author[1],
                                      "GIT_COMMITTER_NAME": committer[0], "GIT_COMMITTER_EMAIL": committer[1]}):
        git("commit", "-q", "-m", message)


@contextlib.contextmanager
def scratch_repository(settings=None):
    """A new repository on `main`, entered (the guard runs git in the working directory), with empty local lists and
    no identity rule unless `settings` asks for it."""
    previous = os.getcwd()
    with tempfile.TemporaryDirectory() as directory:
        lists, work = Path(directory) / "lists", Path(directory) / "work"
        lists.mkdir()
        work.mkdir()
        (lists / "deny").write_text("")
        (lists / "internal").write_text("")
        environment = {"TQ_GUARD_DENYLIST": str(lists / "deny"), "TQ_GUARD_INTERNAL": str(lists / "internal"),
                       "HOME": directory, **(settings or {})}
        with mock.patch.dict(os.environ, environment):
            os.environ.pop("TQ_GUARD_IDENTITY", None) if "TQ_GUARD_IDENTITY" not in (settings or {}) else None
            os.chdir(work)
            try:
                git("init", "-q", "-b", "main")
                yield work
            finally:
                os.chdir(previous)


def as_person(person):
    """The environment that makes git author and commit as `person`."""
    return mock.patch.dict(os.environ, {"GIT_AUTHOR_NAME": person[0], "GIT_AUTHOR_EMAIL": person[1],
                                        "GIT_COMMITTER_NAME": person[0], "GIT_COMMITTER_EMAIL": person[1]})


def contributor():
    """Makes the scratch repository a contributor's clone, as the install script without --maintainer does."""
    git("config", "tq.guard.maintainer", "false")


def deny(*terms):
    """Writes the scratch repository's personal denylist (case-insensitive literals)."""
    (Path.cwd().parent / "lists" / "deny").write_text("".join(f"{term}\n" for term in terms))


def push(remote, url, local, remote_sha=None, ref="refs/heads/fix"):
    """Runs the pre-push check for one ref update; returns its problems."""
    line = f"{ref} {local} {ref} {remote_sha or guard.ZERO}\n"
    with mock.patch.object(guard.sys, "stdin", io.StringIO(line)):
        return quiet(guard.pre_push, remote, url)


def quiet(check, *arguments):
    """Runs one guard check; returns the problems it reported ([] when it passed)."""
    said = io.StringIO()
    with contextlib.redirect_stderr(said), contextlib.redirect_stdout(io.StringIO()):
        try:
            check(*arguments)
        except SystemExit:
            return [line for line in said.getvalue().splitlines() if line.startswith("  - ")]
    return []


class Contributors(unittest.TestCase):
    """An outside contribution: the contributor commits as themselves, and GitHub squash-merges the pull request,
    keeping them as its author. The identity rule is the maintainers' (tools/install-guardrails.sh --maintainer)
    and CI's (TQ_GUARD_IDENTITY=1)."""

    def test_github_merging_a_pull_request_keeps_its_contributor_as_author(self):
        problems = []
        guard.check_history_identity("1234abcd", *JANE, *guard.MERGER, problems)
        self.assertEqual(problems, [])
        guard.check_history_identity("1234abcd", *guard.MERGER, *guard.MERGER, problems)
        self.assertEqual(len(problems), 1, problems)

    def test_a_commit_github_did_not_merge_is_held_to_the_identity(self):
        problems = []
        guard.check_history_identity("1234abcd", *JANE, *JANE, problems)
        self.assertEqual(len(problems), 2, problems)
        problems = []
        guard.check_history_identity("1234abcd", *guard.IDENTITY, *guard.IDENTITY, problems)
        guard.check_history_identity("1234abcd", *guard.EARLIER_IDENTITIES[0], *guard.MERGER, problems)
        self.assertEqual(problems, [])

    def test_ci_holds_history_to_the_identity_and_not_a_pull_requests_own_commits(self):
        with scratch_repository({"TQ_GUARD_IDENTITY": "1"}):
            commit(guard.IDENTITY)
            commit(JANE, guard.MERGER, "fix: a contribution GitHub merged")
            git("switch", "-q", "-c", "contribution")
            commit(JANE, message="fix: the pull request's own commit")
            self.assertEqual(len(quiet(guard.audit)), 2)
            self.assertEqual(quiet(guard.audit, "main"), [])
            commit(JANE, message="fix: a change\n\nCo-Authored-By: a helper <helper@example.com>")
            self.assertEqual(len(quiet(guard.audit, "main")), 1, "the messages are still checked")

    def test_a_commit_on_main_that_github_did_not_merge_stays_refused(self):
        with scratch_repository({"TQ_GUARD_IDENTITY": "1"}):
            commit(guard.IDENTITY)
            commit(JANE, message="fix: pushed straight to main")
            git("switch", "-q", "-c", "contribution")
            commit(JANE, message="fix: the pull request's own commit")
            self.assertEqual(len(quiet(guard.audit, "main")), 2)

    def test_a_contributors_clone_holds_no_identity_and_a_maintainers_does(self):
        with scratch_repository():
            contributor()
            commit(JANE, message="fix: a contributor's change")
            self.assertEqual(quiet(guard.audit), [])
            git("config", "tq.guard.maintainer", "true")
            self.assertEqual(len(quiet(guard.audit)), 2)

    def test_pre_commit_holds_only_a_maintainers_commit_to_the_identity(self):
        with scratch_repository():
            contributor()
            Path("notes.md").write_text("a note\n")
            git("add", "notes.md")
            with mock.patch.dict(os.environ, {"GIT_AUTHOR_NAME": JANE[0], "GIT_AUTHOR_EMAIL": JANE[1],
                                              "GIT_COMMITTER_NAME": JANE[0], "GIT_COMMITTER_EMAIL": JANE[1]}):
                self.assertEqual(quiet(guard.pre_commit), [])
                git("config", "tq.guard.maintainer", "true")
                self.assertEqual(len(quiet(guard.pre_commit)), 2)

    def test_a_contributor_pushes_to_their_fork_and_a_maintainer_only_to_tensorquay(self):
        with scratch_repository():
            contributor()
            commit(JANE, message="fix: a contributor's change")
            refs = f"refs/heads/fix {git('rev-parse', 'HEAD')} refs/heads/fix {guard.ZERO}\n"
            fork = "https://github.com/jane/tensorquay-agent.git"
            with mock.patch.object(guard.sys, "stdin", io.StringIO(refs)):
                self.assertEqual(quiet(guard.pre_push, "origin", fork), [])
            git("config", "tq.guard.maintainer", "true")
            with mock.patch.object(guard.sys, "stdin", io.StringIO(refs)):
                problems = quiet(guard.pre_push, "origin", fork)
            self.assertTrue(any("not a TensorQuay GitHub repository" in p for p in problems), problems)

    def test_the_install_sets_the_company_identity_only_for_a_maintainer_and_marks_a_contributors_clone(self):
        for arguments, maintainer in (([], False), (["--maintainer"], True)):
            with self.subTest(arguments=arguments), scratch_repository():
                Path("tools").mkdir()
                for name in ("install-guardrails.sh", "git-guard.py"):
                    shutil.copy(TOOLS / name, "tools")
                shutil.copytree(TOOLS.parent / ".githooks", ".githooks")
                commit(guard.IDENTITY)
                result = subprocess.run(["sh", "tools/install-guardrails.sh", *arguments], capture_output=True,
                                        text=True, check=False)
                self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
                local = subprocess.run(["git", "config", "--local", "--get-regexp", r"^(user\.|tq\.)"],
                                       capture_output=True, text=True, check=False).stdout.split("\n")
                expected = ["tq.guard.maintainer true", f"user.email {guard.IDENTITY[1]}",
                            f"user.name {guard.IDENTITY[0]}"] if maintainer else ["tq.guard.maintainer false"]
                self.assertEqual(sorted(filter(None, local)), expected)
                self.assertEqual(git("config", "--local", "--get", "core.hooksPath"), ".githooks")

    def test_an_unknown_install_argument_is_refused(self):
        with scratch_repository():
            Path("tools").mkdir()
            shutil.copy(TOOLS / "install-guardrails.sh", "tools")
            result = subprocess.run(["sh", "tools/install-guardrails.sh", "--maintainr"], capture_output=True,
                                    text=True, check=False)
            self.assertEqual(result.returncode, 2, result.stdout + result.stderr)
            self.assertIn("usage:", result.stderr)


class ContributorLimits(unittest.TestCase):
    """What a contributor's freedom does not loosen: no commit carries a personal identifier of the denylist as its
    author or committer, whoever made it; a clone that commits as the company keeps the maintainers' rules; main is
    never force-pushed. And what it needs: a contributor force-pushes their own branch, and a first push to their
    fork checks only the commits TensorQuay's repositories do not already hold."""

    def test_no_commit_carries_a_denylisted_identity_whoever_made_it(self):
        for settings in ({}, {"TQ_GUARD_IDENTITY": "1"}):
            with self.subTest(settings=settings), scratch_repository(settings):
                if not settings:
                    contributor()
                deny("quinlan")
                commit(guard.IDENTITY)
                commit(PAT, guard.MERGER, "fix: a pull request GitHub merged")
                git("switch", "-q", "-c", "contribution")
                commit(PAT, message="fix: the pull request's own commit")
                problems = quiet(guard.audit, "main")
                self.assertEqual(len(problems), 3, problems)
                self.assertTrue(all("personal identifier from the denylist" in p for p in problems), problems)
                self.assertFalse(any("quinlan" in p.lower() for p in problems), "the term is never echoed")
                Path(".guard-allow").write_text(f"# a contributor whose name resembles a listed term\n"
                                                f"identity-ok:{PAT[1]}\n")
                self.assertEqual(quiet(guard.audit, "main"), [])

    def test_a_denylisted_identity_is_refused_at_commit_and_push_in_any_clone(self):
        with scratch_repository():
            contributor()
            deny("quinlan")
            commit(JANE, message="fix: a contributor's change")
            Path("notes.md").write_text("another note\n")
            git("add", "notes.md")
            with mock.patch.dict(os.environ, {"GIT_AUTHOR_NAME": PAT[0], "GIT_AUTHOR_EMAIL": PAT[1],
                                              "GIT_COMMITTER_NAME": PAT[0], "GIT_COMMITTER_EMAIL": PAT[1]}):
                self.assertEqual(len(quiet(guard.pre_commit)), 2)
            commit(PAT, message="fix: a change made under a listed name")
            problems = push("origin", "https://github.com/pat/tensorquay-agent.git", git("rev-parse", "HEAD"))
            self.assertEqual(len(problems), 2, problems)

    def test_a_contributor_force_pushes_their_own_branch_but_never_main(self):
        with scratch_repository():
            contributor()
            commit(JANE, message="fix: a contributor's change")
            base = git("rev-parse", "HEAD")
            commit(JANE, message="fix: the first version")
            old = git("rev-parse", "HEAD")
            git("reset", "-q", "--hard", base)
            commit(JANE, message="fix: the version after review")
            new = git("rev-parse", "HEAD")
            fork = "https://github.com/jane/tensorquay-agent.git"
            for ref, refused in (("refs/heads/fix", False), ("refs/heads/main", True)):
                with self.subTest(ref=ref):
                    problems = push("origin", fork, new, old, ref)
                    self.assertEqual(any("force-push" in p for p in problems), refused, problems)
            git("config", "tq.guard.maintainer", "true")
            problems = push("origin", "https://github.com/TensorQuay/tensorquay-agent.git", new, old)
            self.assertTrue(any("force-push" in p for p in problems), problems)

    def test_a_clone_that_commits_as_the_company_keeps_the_maintainers_rules(self):
        with scratch_repository():
            contributor()
            git("config", "user.name", guard.IDENTITY[0])
            git("config", "user.email", guard.IDENTITY[1])
            commit(guard.IDENTITY)
            problems = push("origin", "https://github.com/someone/tensorquay-agent.git", git("rev-parse", "HEAD"))
            self.assertTrue(any("not a TensorQuay GitHub repository" in p for p in problems), problems)

    def test_a_first_push_to_a_fork_checks_only_what_tensorquay_does_not_hold(self):
        with scratch_repository():
            contributor()
            commit(guard.IDENTITY, message="Bump a dependency from 1.0 to 1.1")
            git("remote", "add", "upstream", "https://github.com/TensorQuay/tensorquay-agent.git")
            git("update-ref", "refs/remotes/upstream/main", "HEAD")
            git("switch", "-q", "-c", "fix")
            commit(JANE, message="fix: a contributor's change")
            fork = "https://github.com/jane/tensorquay-agent.git"
            self.assertEqual(push("fork", fork, git("rev-parse", "HEAD")), [])
            commit(JANE, message="a change without a type")
            self.assertEqual(len(push("fork", fork, git("rev-parse", "HEAD"))), 1, "the new commits are checked")

    def test_the_audit_names_a_base_it_cannot_find(self):
        with scratch_repository({"TQ_GUARD_IDENTITY": "1"}):
            commit(guard.IDENTITY)
            problems = quiet(guard.audit, "origin/main")
            self.assertEqual(len(problems), 1, problems)
            self.assertIn("origin/main", problems[0])

    def test_a_clone_not_marked_as_a_contributors_keeps_the_maintainers_rules(self):
        # A clone set up before contributors could commit has no mark: it stays a maintainer's, as does any audit
        # outside a set-up clone.
        with scratch_repository():
            commit(JANE, message="fix: a change")
            self.assertEqual(len(quiet(guard.audit)), 2)
            problems = push("origin", "https://github.com/jane/tensorquay-agent.git", git("rev-parse", "HEAD"))
            self.assertTrue(any("not a TensorQuay GitHub repository" in p for p in problems), problems)
            contributor()
            self.assertEqual(quiet(guard.audit), [])

    def test_a_maintainers_push_holds_each_outgoing_commit_to_the_identity(self):
        with scratch_repository():
            git("config", "tq.guard.maintainer", "true")
            commit(guard.IDENTITY)
            commit(JANE, guard.MERGER, "fix: a contribution GitHub merged")
            tensorquay = "https://github.com/TensorQuay/tensorquay-agent.git"
            self.assertEqual(push("origin", tensorquay, git("rev-parse", "HEAD")), [])
            commit(JANE, message="fix: made under another name")
            problems = push("origin", tensorquay, git("rev-parse", "HEAD"))
            self.assertTrue(any("author is 'Jane Doe" in p for p in problems), problems)

    def test_a_contributors_commits_are_checked_for_secrets_personal_data_and_attribution(self):
        token = "ghp_" + "x1Y2z3" * 6  # built here, so this file holds no token
        home = "/" + "home/jane/notes.txt"
        with scratch_repository():
            contributor()
            commit(JANE, message="fix: a contributor's change")
            Path("config.md").write_text(f"token {token}\nsee {home}\n")
            git("add", "config.md")
            with as_person(JANE):
                problems = quiet(guard.pre_commit)
                git("commit", "-q", "-m", "fix: a change\n\nCo-Authored-By: a helper <helper@example.com>")
            self.assertTrue(any("possible secret" in p for p in problems), problems)
            self.assertTrue(any("personal path" in p for p in problems), problems)
            problems = push("fork", "https://github.com/jane/tensorquay-agent.git", git("rev-parse", "HEAD"))
            for kind in ("possible secret", "personal path", "attribution"):
                self.assertTrue(any(kind in p.lower() for p in problems), (kind, problems))
            problems = quiet(guard.audit)
            for kind in ("possible secret", "personal path", "attribution"):
                self.assertTrue(any(kind in p.lower() for p in problems), (kind, problems))

    def test_a_push_checks_what_another_tensorquay_repository_holds(self):
        # Only this project's own repository counts as already checked: another TensorQuay repository's history
        # (private, unguarded) is checked like any other commit before it can reach this one.
        with scratch_repository():
            git("config", "tq.guard.maintainer", "true")
            commit(guard.IDENTITY)
            git("switch", "-q", "-c", "sibling")
            commit(JANE, message="a commit of another repository")
            git("remote", "add", "engine", "https://github.com/TensorQuay/another-project.git")
            git("update-ref", "refs/remotes/engine/main", "HEAD")
            git("switch", "-q", "main")
            with as_person(guard.IDENTITY):
                git("merge", "-q", "--no-ff", "-m", "fix: brings in the other history", "sibling")
            problems = push("origin", "https://github.com/TensorQuay/tensorquay-agent.git", git("rev-parse", "HEAD"))
            self.assertTrue(any("author is 'Jane Doe" in p for p in problems), problems)

    def test_a_maintainer_pushes_only_to_github_itself(self):
        with scratch_repository():
            git("config", "tq.guard.maintainer", "true")
            commit(guard.IDENTITY)
            head = git("rev-parse", "HEAD")
            for url, refused in (("https://github.com/TensorQuay/tensorquay-agent.git", False),
                                 ("git@github.com:TensorQuay/tensorquay-agent.git", False),
                                 ("ssh://git@github.com/TensorQuay/tensorquay-agent.git", False),
                                 ("https://mirror.example/https://github.com/TensorQuay/tensorquay-agent.git", True),
                                 ("https://github.com.example/TensorQuay/tensorquay-agent.git", True)):
                with self.subTest(url=url):
                    problems = push("origin", url, head)
                    self.assertEqual(any("not a TensorQuay GitHub repository" in p for p in problems), refused,
                                     problems)


if __name__ == "__main__":
    unittest.main()
