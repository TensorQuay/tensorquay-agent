#!/usr/bin/env python3
"""TensorQuay git guardrails. Rules and rationale: AGENTS.md ("Git guardrails").

Subcommands (wired by .githooks/*):
  pre-commit            staged paths, sizes and added lines; the identity (never a denylisted name; for a
                        maintainer, the company's)
  commit-msg <file>     message format and banned words
  pre-push <remote> <url>   (ref updates on stdin) every outgoing commit; force-pushes (except to a contributor's
                        own branches); the remote (for a maintainer, a TensorQuay one)
  audit [--contributed <base>]   whole tree + full history; exit 1 on any violation. With --contributed, the
                        commits in <base>..HEAD are a pull request's own: any identity, everything else checked.

The identity and remote rules are the maintainers': they apply in a clone set up with
`tools/install-guardrails.sh --maintainer` (git config tq.guard.maintainer), in a clone that commits as the company
identity, and in CI's audits (TQ_GUARD_IDENTITY=1). A contributor commits as themselves, pushes to their own fork and
may force-push their own branches there; GitHub's squash merge of their pull request keeps them as its author.
Content and messages are checked for everyone, and no commit, whoever made it, has a personal identifier of the
denylist as its author or committer.

Personal identifiers are never stored in the repository. They are read from a local denylist file
(default ~/.config/tensorquay/guard-denylist, or $TQ_GUARD_DENYLIST), one case-insensitive literal per line.
The tree audit also refuses internal process references: the patterns below, and the terms of a second local list
(default ~/.config/tensorquay/guard-internal, or $TQ_GUARD_INTERNAL) such as machine, team and script names.
"""
import fnmatch
import os
import re
import subprocess
import sys
from pathlib import Path

IDENTITY = ("TensorQuay", "dev@tensorquay.com")  # every new commit, from 7 Nov 2026
EARLIER_IDENTITIES = (("TensorQuay", "admin@tensorquay.com"),)  # history made before then; never a new commit
MERGER = ("GitHub", "noreply@github.com")  # the committer of a pull request merged on GitHub; never an author
REMOTE_OK = re.compile(r"github\.com[:/]TensorQuay/", re.I)
MAX_BYTES = 2 * 1024 * 1024
ZERO = "0" * 40
PROTECTED_BRANCHES = {"refs/heads/main", "refs/heads/master"}

# Commit messages must never name AI tools/vendors or carry attribution trailers.
MSG_BANNED = re.compile(
    r"claude|anthropic|codex|openai|chatgpt|copilot|gemini|\baider\b|co-authored-by|generated (with|by)|\U0001F916",
    re.I,
)
# File contents must never carry AI attribution markers (model names in research text are allowed).
FILE_ATTRIBUTION = re.compile(
    r"co-authored-by:|generated (with|by) \[?(claude|codex|chatgpt|copilot|gemini|an? ai)"
    r"|noreply@anthropic\.com|\U0001F916 generated",
    re.I,
)
CONVENTIONAL = re.compile(
    r"^(feat|fix|perf|test|docs|refactor|build|ci|chore|style|revert)(\([a-z0-9._/-]+\))?!?: \S.{0,98}$"
)
SECRETS = [
    re.compile(p)
    for p in (
        r"hf_[A-Za-z0-9]{30,}",
        r"gh[pousr]_[A-Za-z0-9]{36,}",
        r"github_pat_[A-Za-z0-9_]{50,}",
        r"sk-(ant-)?[A-Za-z0-9_-]{32,}",
        r"AKIA[0-9A-Z]{16}",
        r"xox[abprs]-[A-Za-z0-9-]{10,}",
        r"-----BEGIN [A-Z ]*PRIVATE KEY-----",
    )
]
# Heuristic: a quoted value of 12+ chars assigned to a secret-like name. Skipped when the value is plainly a test value.
GENERIC_SECRET = re.compile(r"(?i)(api[_-]?key|secret|passw(or)?d|token)[\"']?\s*[:=]\s*[\"']([^\"'\s]{12,})[\"']")
FAKE_VALUE = re.compile(r"(?i)fixture|test|fake|dummy|example|placeholder|changeme|redacted")
PERSONAL = [
    re.compile(p, re.I)
    for p in (
        r"(?<![\w/.-])/Users/[A-Za-z0-9._-]+",
        r"(?<![\w/.-])/home/(?!runner\b)[A-Za-z0-9._-]+",
        r"(?<![\w/.-])/private/(tmp|var)/",
        r"C:\\+Users\\+",
        r"[A-Za-z0-9._%+-]+@(gmail|googlemail|qq|163|126|hotmail|outlook|live|icloud|yahoo)\.com",
    )
]
# Internal process references a stranger cannot resolve: team roles, internal note names, bug labels, team scripts.
# The documented roles stay allowed ("the technical lead", "the Product Owner"). Checked on the tree only (audit).
INTERNAL = [
    re.compile(p)
    for p in (
        r"\bDev [1-9]\b",
        r"\bdev[1-9]\b",
        r"design-[a-z0-9-]+-dev[0-9]",
        r"\breport-[0-9]{3}\b",
        r"[Oo]wner bug",
        r"~/team/",
        r"lead decision",
        r"\(lead(\)|, [0-9])",  # "(lead)", "(lead, 2026-09-27)"; agent teams' "(lead, run)" is product code
        r"(?<!technical )\blead's (review|list)",
    )
]
IPV4 = re.compile(r"(?<![\d.])(?:\d{1,3}\.){3}\d{1,3}(?![\d.])")
IP_OK = {"127.0.0.1", "0.0.0.0", "169.254.169.254"}  # loopback, any, cloud metadata (used in SSRF tests)
IP_DOC_PREFIXES = ("192.0.2.", "198.51.100.", "203.0.113.")  # RFC 5737 documentation ranges
FORBIDDEN_PATHS = (
    ".env", ".env.*", "*.pem", "*.key", "*.p12", "id_rsa*", "id_ed25519*",
    ".claude/*", "*/.claude/*", "CLAUDE.local.md", ".codex/*", ".cursor/*", ".aider*",
    ".DS_Store", "*/.DS_Store", "target/*", "node_modules/*", "*/node_modules/*", "*__pycache__/*",
    "*.safetensors", "*.gguf", "*.bin", "*.pt", "*.pth", "*.ckpt", "*.onnx",
    "*.zip", "*.tar", "*.tar.gz", "*.tgz", "*.7z", "*.pdf", "*.mp4", "*.mov",
)
# These files must spell out the banned AI words to enforce them; all other checks still apply.
GUARD_FILES = ("tools/git-guard.py", "tools/test_git_guard.py", "AGENTS.md", "CLAUDE.md", ".githooks/*")


def git(*args, check=True):
    out = subprocess.run(["git", *args], capture_output=True, check=check)
    return out.stdout.decode("utf-8", "replace")


def root():
    return Path(git("rev-parse", "--show-toplevel").strip())


def parse_list(text):
    return [ln.strip() for ln in text.splitlines() if ln.strip() and not ln.lstrip().startswith("#")]


def load_list(path):
    if not path.exists():
        return []
    return parse_list(path.read_text(encoding="utf-8"))


def denylist():
    path = Path(os.environ.get("TQ_GUARD_DENYLIST", "~/.config/tensorquay/guard-denylist")).expanduser()
    words = [w.lower() for w in load_list(path)]
    if not words:
        msg = f"guard: personal denylist {path} missing or empty"
        if os.environ.get("TQ_GUARD_REQUIRE_DENYLIST") == "1":
            sys.exit(msg)
        print(f"warning: {msg}; personal-name checks limited", file=sys.stderr)
    return words


def internal_terms():
    """The local list of internal vocabulary (machine, team and script names), kept out of the repository."""
    path = Path(os.environ.get("TQ_GUARD_INTERNAL", "~/.config/tensorquay/guard-internal")).expanduser()
    words = [w.lower() for w in load_list(path)]
    if not words:
        msg = f"guard: internal-term list {path} missing or empty"
        if os.environ.get("TQ_GUARD_REQUIRE_INTERNAL") == "1":
            sys.exit(msg)
        print(f"warning: {msg}; internal-term checks limited to the built-in patterns", file=sys.stderr)
    return words


def allowlist(entries=None):
    """.guard-allow entries (reviewed in PRs): `<glob>` or `path:<glob>` allows a file type/size;
    `ip:<address>`; `personal-ok:<glob>` skips personal-data and IP checks (private internal docs only);
    `secret-ok:<glob>` skips the generic key=value heuristic (real token formats are always checked);
    `internal-ok:<glob>` skips the internal-reference check (files that cannot change, such as applied migrations);
    `identity-ok:<email>` accepts an author or committer whose name or address only resembles a denylist term."""
    allow = {"path": [], "ip": set(), "personal-ok": [], "secret-ok": [], "internal-ok": [], "identity-ok": []}
    for entry in load_list(root() / ".guard-allow") if entries is None else entries:
        kind, sep, value = entry.partition(":")
        if sep and kind in allow:
            allow[kind].add(value) if kind == "ip" else allow[kind].append(value)
        else:
            allow["path"].append(entry)
    return allow


def matches(path, patterns):
    return any(fnmatch.fnmatch(path, p) for p in patterns)


def check_path(path, size, allow_globs, problems):
    if matches(path, allow_globs):
        return
    if matches(path, FORBIDDEN_PATHS) and path != ".env.example":
        problems.append(f"{path}: forbidden file type or location")
    if size is not None and size > MAX_BYTES:
        problems.append(f"{path}: {size / 1e6:.1f} MB exceeds {MAX_BYTES / 1e6:.0f} MB (use .guard-allow if intended)")


def check_line(path, lineno, text, deny, allow, problems):
    where = f"{path}:{lineno}"
    if not matches(path, GUARD_FILES) and FILE_ATTRIBUTION.search(text):
        problems.append(f"{where}: AI attribution marker")
    for rx in SECRETS:
        if rx.search(text):
            problems.append(f"{where}: possible secret ({rx.pattern[:24]}…)")
    if not matches(path, allow["secret-ok"]):
        for m in GENERIC_SECRET.finditer(text):
            if not FAKE_VALUE.search(m.group(3)):
                problems.append(f"{where}: possible secret (quoted value assigned to '{m.group(1)}')")
    if matches(path, allow["personal-ok"]):
        return
    for rx in PERSONAL:
        if rx.search(text):
            problems.append(f"{where}: personal path or e-mail")
    low = text.lower()
    for word in deny:
        if word in low:
            problems.append(f"{where}: personal identifier from the denylist")
    for ip in IPV4.findall(text):
        if ip in IP_OK or ip in allow["ip"] or ip.startswith(IP_DOC_PREFIXES):
            continue
        if all(int(p) <= 255 for p in ip.split(".")):
            problems.append(f"{where}: IP address {ip}")


def check_internal(path, lineno, text, terms, allow, problems):
    """Internal process references in a tree line: the built-in patterns, then the local list's terms (never echoed)."""
    if matches(path, GUARD_FILES) or matches(path, allow["internal-ok"]):
        return
    where = f"{path}:{lineno}"
    for rx in INTERNAL:
        m = rx.search(text)
        if m:
            problems.append(f"{where}: internal reference '{m.group(0)}' (cite the public record: an ADR or test ID)")
            return
    low = text.lower()
    if any(term in low for term in terms):
        problems.append(f"{where}: internal term from the local list")


def scan_patch(patch, deny, allow, problems):
    """Check the added lines of a unified diff (-U0)."""
    path, lineno = None, 0
    for line in patch.splitlines():
        if line.startswith("+++ "):
            path = line[6:] if line.startswith("+++ b/") else None
        elif line.startswith("@@"):
            m = re.search(r"\+(\d+)", line)
            lineno = int(m.group(1)) if m else 0
        elif line.startswith("+") and path:
            check_line(path, lineno, line[1:], deny, allow, problems)
            lineno += 1


def maintainer():
    """Whether the identity and remote rules apply: a maintainer's clone, a clone that commits as the company identity
    (one set up before --maintainer existed, or with it forgotten), or a run that asks for them (CI)."""
    if os.environ.get("TQ_GUARD_IDENTITY") == "1":
        return True
    if git("config", "--get", "--bool", "tq.guard.maintainer", check=False).strip() == "true":
        return True
    return git("config", "--get", "user.email", check=False).strip().lower() == IDENTITY[1]


def check_identity_terms(name, email, what, deny, allow, problems):
    """No author or committer, whoever made the commit, has a personal identifier of the denylist (never echoed). A
    reviewed `identity-ok:<email>` accepts one whose name or address only resembles a listed term."""
    if email.lower() in (e.lower() for e in allow.get("identity-ok", ())):
        return
    if any(word in f"{name} <{email}>".lower() for word in deny):
        problems.append(f"{what} has a personal identifier from the denylist")


def check_identity(name, email, what, problems, history=False, committer=False):
    """A new commit is made as IDENTITY; an existing one (pushed or audited) may also carry an earlier identity, and a
    pull request merged on GitHub has GitHub as its committer."""
    allowed = {IDENTITY}
    if history:
        allowed.update(EARLIER_IDENTITIES)
        if committer:
            allowed.add(MERGER)
    if (name, email) not in allowed:
        problems.append(f"{what} is '{name} <{email}>', must be '{IDENTITY[0]} <{IDENTITY[1]}>'")


def check_history_identity(sha, author_name, author_email, committer_name, committer_email, problems):
    """An existing commit: one GitHub made by merging a pull request keeps that pull request's author, whoever it is
    (GitHub never authors one); any other commit is a maintainer's."""
    if (committer_name, committer_email) == MERGER:
        if (author_name, author_email) == MERGER:
            problems.append(f"{sha} author is '{MERGER[0]} <{MERGER[1]}>', which never authors a commit")
        return
    check_identity(author_name, author_email, f"{sha} author", problems, history=True)
    check_identity(committer_name, committer_email, f"{sha} committer", problems, history=True, committer=True)


def check_message(msg, what, problems, require_format=True):
    body = "\n".join(ln for ln in msg.splitlines() if not ln.startswith("#")).strip()
    if MSG_BANNED.search(body):
        problems.append(f"{what}: message names an AI tool/vendor or carries an attribution trailer")
    first = body.splitlines()[0] if body else ""
    conventional = CONVENTIONAL.match(first) and len(first) <= 100  # AGENTS.md: the whole first line, type included
    if require_format and not (conventional or first.startswith(("Merge ", "Revert ", "fixup! ", "squash! "))):
        problems.append(f"{what}: first line must follow Conventional Commits ('type(scope): subject', ≤ 100 chars)")


def report(problems, stage):
    if problems:
        print(f"\n✖ git guard ({stage}) blocked this:", file=sys.stderr)
        for p in dict.fromkeys(problems):
            print(f"  - {p}", file=sys.stderr)
        print("Rules: AGENTS.md → Git guardrails. Fix the cause; never bypass with --no-verify.\n", file=sys.stderr)
        sys.exit(1)


def pre_commit():
    problems = []
    deny, allow, identity = denylist(), allowlist(), maintainer()
    for var in ("GIT_AUTHOR_IDENT", "GIT_COMMITTER_IDENT"):
        m = re.match(r"(.*) <(.*)>", git("var", var).strip())
        what = var.split("_")[1].lower()
        if identity:
            check_identity(m.group(1), m.group(2), what, problems)
        check_identity_terms(m.group(1), m.group(2), what, deny, allow, problems)
    for entry in filter(None, git("diff", "--cached", "--name-only", "--diff-filter=ACMR", "-z").split("\0")):
        size = int(git("cat-file", "-s", f":{entry}").strip())
        check_path(entry, size, allow["path"], problems)
    scan_patch(git("diff", "--cached", "-U0", "--no-color", "--no-ext-diff"), deny, allow, problems)
    report(problems, "pre-commit")


def commit_msg(path):
    problems = []
    check_message(Path(path).read_text(encoding="utf-8"), "commit message", problems)
    report(problems, "commit-msg")


def check_commit(sha, deny, allow, problems, identity=True):
    short = sha[:8]
    an, ae, cn, ce = git("show", "-s", "--format=%an%x00%ae%x00%cn%x00%ce", sha).strip().split("\0")
    if identity:
        check_history_identity(short, an, ae, cn, ce, problems)
    check_identity_terms(an, ae, f"{short} author", deny, allow, problems)
    check_identity_terms(cn, ce, f"{short} committer", deny, allow, problems)
    check_message(git("show", "-s", "--format=%B", sha), short, problems)
    for row in git("diff-tree", "-r", "--root", "--no-commit-id", "--diff-filter=ACMR", sha).splitlines():
        meta, path = row.split("\t", 1)
        blob = meta.split()[3]
        check_path(path, int(git("cat-file", "-s", blob).strip()), allow["path"], problems)
    scan_patch(git("show", "--format=", "-U0", "--no-color", "--no-ext-diff", sha), deny, allow, problems)


def tensorquay_remotes():
    """The remotes that are TensorQuay's GitHub repositories: what they hold was checked when it got there."""
    return [name for name in git("remote").split() if REMOTE_OK.search(git("remote", "get-url", name, check=False))]


def outgoing(local_sha, remote_sha, remote):
    """The commits a push sends: those not on the remote's branch, nor on the pushed remote or a TensorQuay remote
    (so a contributor's first push to a fork checks their own commits, not the whole of upstream's history)."""
    known = remote_sha != ZERO and subprocess.run(
        ["git", "cat-file", "-e", f"{remote_sha}^{{commit}}"], capture_output=True).returncode == 0
    exclude = [f"--remotes={name}" for name in sorted({remote, *tensorquay_remotes()})]
    return git("rev-list", local_sha, "--not", *exclude, *([remote_sha] if known else [])).split()


def pre_push(remote, url):
    problems = []
    identity = maintainer()
    if identity and not REMOTE_OK.search(url):
        problems.append(f"remote '{remote}' ({url}) is not a TensorQuay GitHub repository")
    deny, allow = denylist(), allowlist()
    for line in sys.stdin.read().splitlines():
        local_ref, local_sha, remote_ref, remote_sha = line.split()
        if local_sha == ZERO:
            if remote_ref in PROTECTED_BRANCHES:
                problems.append(f"deleting {remote_ref} is not allowed")
            continue
        if remote_sha != ZERO:
            ancestor = subprocess.run(["git", "merge-base", "--is-ancestor", remote_sha, local_sha],
                                      capture_output=True).returncode == 0
            # A contributor rewrites their own pull request's branch in their fork; main, and every branch of a
            # maintainer's, keep their history.
            guarded = identity or remote_ref in PROTECTED_BRANCHES
            if not ancestor and guarded and os.environ.get("TQ_GUARD_ALLOW_FORCE") != "1":
                problems.append(f"{remote_ref}: force-push (history rewrite) needs PM approval; see AGENTS.md")
        for sha in outgoing(local_sha, remote_sha, remote):
            check_commit(sha, deny, allow, problems, identity)
    report(problems, "pre-push")


def audit(contributed=None):
    problems = []
    deny, allow, terms = denylist(), allowlist(), internal_terms()
    identity = maintainer()
    if contributed and subprocess.run(["git", "rev-parse", "--verify", "--quiet", f"{contributed}^{{commit}}"],
                                      capture_output=True).returncode != 0:
        report([f"audit --contributed: the base '{contributed}' is not a commit here (fetch it first)"], "audit")
    own = set(git("rev-list", f"{contributed}..HEAD").split()) if contributed else set()
    for row in git("log", "--all", "--format=%H%x00%an%x00%ae%x00%cn%x00%ce").splitlines():
        full, an, ae, cn, ce = row.split("\0")
        sha = full[:10]
        if identity and full not in own:
            check_history_identity(sha, an, ae, cn, ce, problems)
        check_identity_terms(an, ae, f"{sha} author", deny, allow, problems)
        check_identity_terms(cn, ce, f"{sha} committer", deny, allow, problems)
        check_message(git("show", "-s", "--format=%B", full), sha, problems, require_format=False)
    for path in filter(None, git("ls-files", "-z").split("\0")):
        full = root() / path
        size = full.stat().st_size if full.exists() else None
        check_path(path, size, allow["path"], problems)
        if size and size <= MAX_BYTES and b"\0" not in full.read_bytes()[:8192]:
            for n, text in enumerate(full.read_text(encoding="utf-8", errors="replace").splitlines(), 1):
                check_line(path, n, text, deny, allow, problems)
                check_internal(path, n, text, terms, allow, problems)
    report(problems, "audit")
    print("✔ git guard audit: clean")


def main(argv):
    cmd = argv[1] if len(argv) > 1 else ""
    if cmd == "pre-commit":
        pre_commit()
    elif cmd == "commit-msg" and len(argv) == 3:
        commit_msg(argv[2])
    elif cmd == "pre-push" and len(argv) == 4:
        pre_push(argv[2], argv[3])
    elif cmd == "audit" and len(argv) == 2:
        audit()
    elif cmd == "audit" and len(argv) == 4 and argv[2] == "--contributed":
        audit(argv[3])
    else:
        sys.exit(__doc__)


if __name__ == "__main__":
    main(sys.argv)
