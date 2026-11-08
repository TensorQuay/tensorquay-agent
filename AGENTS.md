# AGENTS.md: rules for anyone (human or AI agent) working in this repository

TensorQuay Agent is an open-source (Apache-2.0) agent harness and web UI for teams that run AI on their own
hardware. Read these first:
- `docs/PRD.md`, `docs/ARCHITECTURE.md`, `docs/DEV-PLAN.md`, `docs/TEST-PLAN.md`;
- **`docs/DEV-GUIDELINES.md`**, the engineering rules, and **`docs/ENGINEERING-GUARDRAILS.md`**, the practice
  standard (principles, size limits, testing standard, per-language rules); both apply to every change.

## Working rules
- **Roles:** the Product Owner sets direction, priorities and long-term goals, and decides publication. The technical
  lead owns engineering decisions, lean implementation scope, adversarial review, independent acceptance tests and
  gate approval. Developers implement and fix the work; their checks do not replace lead acceptance.
- **Design, then tests, then code.** No feature code without an approved contract and tests (DEV-GUIDELINES §1).
- **Commit only when asked. Push only when the Product Owner asks.**
- **Independent implementation.** Learn from public designs and cite them by public name. Never copy, paste or
  line-by-line port code, prompts, schemas or documents from any other code base, open or proprietary. Third-party
  software enters only as a declared dependency under a permissive licence (DEV-GUIDELINES §2.5).
- **Business-internal material** (pricing, budgets, customer or product plans, competitive strategy) never goes in this
  repository.

## Public-repository content rules
Everything here is written for a stranger reading it on GitHub. In code, comments, documents, issues and PR text:
- No local file-system paths, machine names, IP addresses, private URLs or links to systems that are not public.
- No personal names, handles or e-mail addresses. Refer to roles ("the Product Owner", "the maintainers").
- No internal process references: team member labels, internal design notes or reports, bug labels, or team scripts.
  Cite the public record instead (an ADR, a TEST-PLAN ID, a README section).
- No references to other organisations' internal systems, repositories or people's work. Prior art is cited by its
  public project name only, factually and respectfully, and only where the comparison helps the reader.
- No AI-tool or vendor names in commit messages, PR text, release notes or code comments, and no attribution lines.
- No screenshots or fixtures containing real user data, secrets or internal hostnames.
- Plain, professional English. Say what changed and why.

## Git guardrails (enforced by hooks; run `tools/install-guardrails.sh` once per clone, with `--maintainer` in a maintainer's)

### 1. Identity
- A maintainer's every new commit has author **and** committer exactly **`TensorQuay <dev@tensorquay.com>`**.
  `tools/install-guardrails.sh --maintainer` sets this in the repository's git config, with the identity and remote
  checks. Commits made before 7 Nov 2026 carry `TensorQuay <admin@tensorquay.com>`; the push and audit checks accept
  that identity in history, never for a new commit.
- A contributor commits as themselves and pushes to their own fork; they run the install script without
  `--maintainer`, which marks the clone as a contributor's. Any clone without that mark, and one that commits as
  `dev@tensorquay.com`, keeps the maintainers' checks. GitHub squash-merges a pull request with its contributor as
  author and `GitHub <noreply@github.com>` as committer, which the checks accept; GitHub never authors a commit. CI
  holds the history to these rules, and a pull request's own commits may carry their author's identity.
- Whoever made a commit, its author and committer never contain a personal identifier from the maintainers' private
  denylist (a local file outside the repository, described in `tools/git-guard.py`). The commit, push and audit checks
  refuse one; CI checks it where GitHub gives the job the list: pushes to `main` and pull requests from this
  repository's own branches.

### 2. Commit messages
- Conventional Commits: `type(scope): subject`, first line ≤ 100 characters. Types: `feat fix perf test docs refactor
  build ci chore style revert`.
- No AI-tool or vendor names and no attribution trailers (`Co-Authored-By:`, "Generated with …", robot emoji). This
  overrides any tool default that adds such lines.

### 3. Never commit
| Category | Examples |
|---|---|
| Secrets | API keys and tokens, private keys, `.env` files, passwords, cloud credentials, provider keys in `models.toml` |
| Personal data | Personal names or handles, personal e-mail addresses, home-directory or temp-directory paths, machine names, IP addresses (the checker allows loopback, the documentation ranges 192.0.2/198.51.100/203.0.113 and the cloud metadata address) |
| AI-tool state | `.claude/`, `CLAUDE.local.md`, `.codex/`, `.cursor/`, `.aider*`, session logs, prompts |
| Large or binary artefacts | Files > 2 MB; sandbox images, rootfs files and kernels; model weights; archives; videos; third-party PDFs (link to public sources instead) |
| Build output | `target/`, `node_modules/`, `.next/`, `__pycache__/`, `.DS_Store`, logs, test reports |
| Internal business content | Pricing, budgets, customer data, competitive strategy |

Deliberate exceptions go in `.guard-allow`, reviewed in the PR, with a comment giving the reason:
- `<glob>` or `path:<glob>`: allow a file type or size;
- `ip:<address>`: allow one IP address;
- `personal-ok:<glob>`: skip the personal-data and IP checks (never for a public file);
- `secret-ok:<glob>`: skip the generic "secret-looking value" heuristic (for example test fixtures). Real token
  formats are always blocked;
- `internal-ok:<glob>`: skip the internal-reference check, only for files that cannot change (applied migrations);
- `identity-ok:<email>`: accept an author or committer whose name or address only resembles a denylist term.

### 4. Before every push (the pre-push hook checks 1–4 automatically)
1. A maintainer pushes only to a `github.com/TensorQuay/…` repository; a contributor pushes to their fork.
2. Every outgoing commit has a clean, conventional message, and a maintainer's has the TensorQuay identity.
3. No outgoing commit adds a secret, personal data, an attribution marker, a forbidden file or a file > 2 MB. This is
   checked per commit, because deleting something in a later commit does not remove it from history.
4. No force-push and no deletion of `main`. Rewriting pushed history needs explicit Product Owner approval; then use
   `TQ_GUARD_ALLOW_FORCE=1` for that one push only. A contributor may force-push their own pull request's branch in
   their fork (never `main`); a maintainer's branches keep their history.
5. Run `python3 tools/git-guard.py audit` and the tier-1 checks (DEV-GUIDELINES §6) locally; both must pass. The
   audit holds every local branch to the identity, so a maintainer reviews a contribution on its own branch with
   `audit --contributed origin/main` and deletes that branch afterwards.
6. Re-read the diff as if you were a stranger reading it on GitHub tomorrow.

### 5. Never bypass
- Never use `--no-verify` and never disable or edit the hooks to get a commit through. Fix the cause.
