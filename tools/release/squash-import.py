#!/usr/bin/env python3
"""Build a public main update as one squashed commit (a squashed import); it never pushes.

    python3 tools/release/squash-import.py --source <verified commit> --message-file <file> [--first-import]
        [--public-main origin/main] [--ref refs/heads/import/main]

The new commit's tree is exactly the source commit's tree. Its parent is the current public main (as last fetched),
or with --first-import the public main's root commit (the first public import), so that a replaced main carries none
of the history in between. Before anything is written, the tree and the message get the guard's checks (identity,
message format, personal data, IP addresses, secrets, internal references, file types and sizes) and the tree gets the
secrets scanner. A tree with the upgrade fixture (GW-I-016) must have it at the public main's last migration, the
release an installation upgrades from. The commit is created with the project identity and stored under a local ref;
nothing is fetched or pushed. The report says what a push of it would add or replace, and whether the upgrade fixture
was checked.
"""

import argparse
import importlib.util
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile

REPOSITORY = Path(__file__).resolve().parents[2]
TIMEOUT = 600
MIGRATIONS = "crates/tqa-gateway/migrations/"
UPGRADE_FIXTURE = "crates/tqa-gateway/tests/fixtures/upgrade-previous-release.sql"


def _load(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


guard = _load("git_guard", REPOSITORY / "tools" / "git-guard.py")


class Refused(Exception):
    """The import was not built; the message lists every problem."""


def git(*args, input_text=None, env=None):
    """The one way this script runs git: local commands only (it never pushes or fetches)."""
    if args and args[0] in ("push", "fetch", "pull", "remote"):
        raise AssertionError("squash-import never talks to a remote")
    result = subprocess.run(["git", *args], cwd=REPOSITORY, input=input_text, capture_output=True, text=True,
                            timeout=TIMEOUT, env=env, check=False)
    if result.returncode != 0:
        raise Refused("git {} failed: {}".format(args[0], result.stderr.strip()))
    return result.stdout


def resolve(ref):
    return git("rev-parse", "--verify", "--quiet", ref + "^{commit}").strip()


def roots(ref):
    return git("rev-list", "--max-parents=0", ref).split()


def tree_entries(commit):
    """(mode, type, object, size, path) for every entry of the commit's tree."""
    entries = []
    for record in filter(None, git("ls-tree", "-r", "-l", "-z", commit).split("\0")):
        meta, path = record.split("\t", 1)
        mode, kind, obj, size = meta.split()
        entries.append((mode, kind, obj, None if size == "-" else int(size), path))
    return entries


def check_message(message, deny, terms, allow):
    problems = []
    guard.check_message(message, "commit message", problems)
    for number, line in enumerate(message.splitlines(), 1):
        guard.check_line("commit message", number, line, deny, allow, problems)
        guard.check_internal("commit message", number, line, terms, allow, problems)
    return problems


def check_tree(commit, deny, terms, allow):
    """The guard's audit, applied to the commit's tree instead of the working copy."""
    problems = []
    for mode, kind, obj, size, path in tree_entries(commit):
        if kind != "blob":
            problems.append("{}: a {} entry cannot be published".format(path, kind))
            continue
        guard.check_path(path, size, allow["path"], problems)
        if mode == "120000" or size is None or size > guard.MAX_BYTES:
            continue
        data = subprocess.run(["git", "cat-file", "blob", obj], cwd=REPOSITORY, capture_output=True,
                              timeout=TIMEOUT, check=True).stdout
        if b"\0" in data[:8192]:
            continue
        for number, line in enumerate(data.decode("utf-8", "replace").splitlines(), 1):
            guard.check_line(path, number, line, deny, allow, problems)
            guard.check_internal(path, number, line, terms, allow, problems)
    return problems


def scan_secrets(commit):
    """The pinned secrets scanner over the commit's exported tree, with that tree's own configuration."""
    secrets = _load("check_secrets", REPOSITORY / "tools" / "check-secrets.py")
    secrets.require_gitleaks()
    with tempfile.TemporaryDirectory(prefix="tqa-import-") as directory:
        archive = subprocess.run(["git", "archive", "--format=tar", commit], cwd=REPOSITORY, capture_output=True,
                                 timeout=TIMEOUT, check=True).stdout
        subprocess.run(["tar", "-xf", "-", "-C", directory], input=archive, capture_output=True, timeout=TIMEOUT,
                       check=True)
        result = secrets.scan("dir", Path("."), Path(directory) / ".gitleaks.toml", cwd=directory)
    return [] if result.returncode == 0 else ["secrets scanner: " + line for line in result.stdout.splitlines()
                                              if line.strip()][:40] or ["secrets scanner failed"]


def highest_migration(commit):
    """The highest migration number in the commit's tree, or None when it has none."""
    names = git("ls-tree", "--name-only", commit, MIGRATIONS).split()
    found = (re.fullmatch(r".*/0*(\d+)_[^/]+\.sql", name) for name in names)
    return max((int(match.group(1)) for match in found if match), default=None)


def check_upgrade_fixture(source, public):
    """GW-I-016 upgrades a database from the fixture's level: it must be the public main's last migration. Returns
    (problems, the report line), and the line says when the check did not run, so a dropped fixture is visible."""
    released = highest_migration(public)
    present = subprocess.run(["git", "cat-file", "-e", source + ":" + UPGRADE_FIXTURE], cwd=REPOSITORY,
                             capture_output=True, timeout=TIMEOUT, check=False).returncode == 0
    if not present:
        return [], "upgrade fixture: not in this tree, not checked"
    if released is None:
        return [], "upgrade fixture: the public main has no migrations, not checked"
    first = git("show", source + ":" + UPGRADE_FIXTURE).partition("\n")[0]
    match = re.fullmatch(r"-- level: (\d+)", first.strip())
    if not match:
        return [UPGRADE_FIXTURE + ": the first line must be `-- level: <migration>`"], None
    if int(match.group(1)) != released:
        return ["{}: the upgrade fixture starts at migration {}, but the public main's last migration is {}; raise its "
                "level and add rows for the new tables (docs/release.md, The upgrade fixture)".format(
                    UPGRADE_FIXTURE, int(match.group(1)), released)], None
    return [], "upgrade fixture: at migration {}, the public main's last".format(released)


def build(source, message, public_main="origin/main", first_import=False, ref=None, secrets=True):
    """Checks, then creates the squashed commit; returns (commit, parent, report lines)."""
    source_commit = resolve(source)
    public = resolve(public_main)
    if not source_commit or not public:
        raise Refused("unknown commit: " + (source if not source_commit else public_main))
    if first_import:
        found = roots(public)
        if len(found) != 1:
            raise Refused("the public main has {} root commits, not one first import".format(len(found)))
        parent = found[0]
    else:
        parent = public
    if not message.endswith("\n"):
        message += "\n"
    listed = subprocess.run(["git", "cat-file", "-e", source_commit + ":.guard-allow"], cwd=REPOSITORY,
                            capture_output=True, timeout=TIMEOUT, check=False).returncode == 0
    allow = guard.allowlist(guard.parse_list(git("show", source_commit + ":.guard-allow")) if listed else [])
    deny, terms = guard.denylist(), guard.internal_terms()
    problems = check_message(message, deny, terms, allow) + check_tree(source_commit, deny, terms, allow)
    fixture_problems, fixture_line = check_upgrade_fixture(source_commit, public)
    problems += fixture_problems
    if secrets:
        problems += scan_secrets(source_commit)
    if problems:
        raise Refused("\n".join("  - " + problem for problem in dict.fromkeys(problems)))
    name, email = guard.IDENTITY
    env = dict(os.environ, GIT_AUTHOR_NAME=name, GIT_AUTHOR_EMAIL=email, GIT_COMMITTER_NAME=name,
               GIT_COMMITTER_EMAIL=email)
    tree = git("rev-parse", source_commit + "^{tree}").strip()
    commit = git("commit-tree", tree, "-p", parent, "-F", "-", input_text=message, env=env).strip()
    if git("rev-parse", commit + "^{tree}").strip() != tree or git("rev-parse", commit + "^").strip() != parent:
        raise Refused("the new commit does not have the source tree and the chosen parent")
    if ref:
        git("update-ref", ref, commit, "")  # an empty old value: never overwrites an existing ref
    return commit, parent, report(commit, parent, public, source_commit, first_import, ref, fixture_line)


def report(commit, parent, public, source, first_import, ref, fixture_line):
    short = lambda value: value[:12]
    lines = ["import {} (tree of {}, parent {})".format(short(commit), short(source), short(parent))]
    lines.append("changes against its parent: " + git("diff", "--shortstat", parent, commit).strip())
    lines.append(fixture_line)
    if ref:
        lines.append("stored as " + ref)
    if first_import and parent != public:
        dropped = int(git("rev-list", "--count", public, "^" + commit).strip())
        lines.append("a push replaces the public main {}: its {} commits after the first import leave main's history"
                     .format(short(public), dropped))
        lines.append("that is a force-push (the Product Owner's approval; TQ_GUARD_ALLOW_FORCE=1 for that one push), "
                     "for example:")
        lines.append("  TQ_GUARD_ALLOW_FORCE=1 git push --force-with-lease=main:{} origin {}:refs/heads/main"
                     .format(public, commit))
    else:
        lines.append("a push adds one commit on top of the public main {} (a fast-forward):".format(short(public)))
        lines.append("  git push origin {}:refs/heads/main".format(commit))
    lines.append("nothing was pushed")
    return lines


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--source", required=True, help="the verified commit whose tree is published")
    parser.add_argument("--message-file", required=True, type=Path, help="the public commit message")
    parser.add_argument("--public-main", default="origin/main", help="the public main as last fetched")
    parser.add_argument("--first-import", action="store_true",
                        help="parent the commit on the public main's first import instead of its tip")
    parser.add_argument("--ref", help="a new local ref to store the commit under (never overwritten)")
    args = parser.parse_args()
    try:
        _, _, lines = build(args.source, args.message_file.read_text(encoding="utf-8"), args.public_main,
                            args.first_import, args.ref)
    except Refused as error:
        print("squash-import refused:\n" + str(error), file=sys.stderr)
        return 1
    print("\n".join(lines))
    return 0


if __name__ == "__main__":
    sys.exit(main())
