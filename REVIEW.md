# Code review

How a change to this repository is reviewed before it merges. It adds to `docs/DEV-GUIDELINES.md` §3 and §7 and
`docs/ENGINEERING-GUARDRAILS.md` §9, and doesn't repeat them. People and automated reviewers both read this file first.

A review exists to stop real failures reaching users: a wrong result, a lost row, a secret in a log, a sandbox that
reaches what it shouldn't, a release that can't be rebuilt. It is not the place for taste.

## 1. The layers

Every change passes these in order. A later layer never replaces an earlier one.

| # | Layer | Who | Blocks the merge when |
|---|---|---|---|
| 1 | **Author's checks** | The author | The tier-1 gates (DEV-GUIDELINES §6) that the change touches don't pass on the head being handed in |
| 2 | **Fresh-context review** | A reviewer that didn't write the change and starts with only the diff, this file and the documents the diff names | It finds a P1 or P2 the author hasn't fixed or answered |
| 3 | **Automated review**: `/code-review`, then `/security-review` | The author runs both on the final head and fixes every finding that survives verification | A verified finding is still open |
| 4 | **CI gates** | CI | Any required gate is red |
| 5 | **Human review by risk tier** (§2) | The technical lead, plus a second reviewer on the high tier | A reviewer hasn't approved |
| 6 | **After the merge** | The maintainers | A merged change breaks the integration branch: it is reverted first and fixed after (§6) |

The hand-in names the commit each check ran on. A result from an earlier commit doesn't count for a later one.

**Fresh context** means the reviewer hasn't seen the author's reasoning, notes or earlier drafts. It reads the change
the way a stranger will. An automated reviewer in a new session, or a person who wasn't part of the work, both qualify.
The author's own session, continued, doesn't.

**A verified finding** is one that holds up when someone tries to disprove it: the failure can be shown from the code
(the input, the path, the wrong result), or reproduced. Findings that are only style, a preference or a "consider" are
dropped before they reach the author (§4).

## 2. Risk tiers

A change takes the tier of its riskiest file. When in doubt, take the higher tier.

| Tier | What it touches | Human review |
|---|---|---|
| **High** | Authentication, sessions, tokens, roles and rate limits; the secrets store and every path that reads a secret; sandbox isolation, egress, the proxy, network rules, runtime images and the Desktop browser policy; migrations and `.sqlx`; release, build and integration scripts; `deny.toml`, ignored advisories and dependency changes | The technical lead, line by line, **and** a second reviewer: a team member who didn't write the change. A red test that fails for the stated reason comes first. Every claim in the hand-in carries its evidence (command, exit code, counts) |
| **Medium** | Gateway handlers and runtime logic outside the above; web pages and the core message catalogue; protocol types and schemas; tests, test fixtures and test plans; copy outside the core catalogue; code comments | The technical lead, line by line. A test-only change shows the test can fail (a negative control) |
| **Low** | Documentation prose only: Markdown and other documents that no build, test or script reads | A light read by the technical lead, after layers 1 to 4 are green |

A change that alters what an existing test asserts is reviewed as a behaviour change, at the tier of the code under
test. A low-tier change still follows the public-content rules in `AGENTS.md`.

**A light read** checks that the prose is correct, says what the change claims, links what it names and follows the
public-content rules. It is for documentation prose only. Test code and product code, including a one-line change,
always get the full line-by-line review of their tier.

**The second reviewer** on the high tier is a team member, never the change's author, who reads it line by line like
the technical lead. The project owner reviews no code. The owner's approval is needed only for a release, for
publishing or announcing anything, and for anything that handles credentials; it comes on top of the tier's review and
never replaces it.

## 3. What failure looks like, per area

### Authentication and secrets
- No secret value reaches a log, an error body, a `Debug` output, an event or a test snapshot. Check the derives on any
  new type that holds one.
- A task or route uses a secret only when it is bound to it; an unbound use stays `409 secret_not_allowed`. A new path
  that reads the store needs its own denial test.
- Role checks happen on the server. An administrator route is tested with a member's token and expects 403, not only
  with an administrator's.
- The login limiter is part of the contract. A suite that signs in many times plans for it instead of raising it.
- Internal envelopes (such as the signed-in-person wrapper) never reach text a model or a person sees.

### Sandbox egress
- A change that loosens egress (a new allow, a `NO_PROXY` entry, a resolver or network change) is high tier even when
  it is one line. The PR says what newly becomes reachable, and a test shows that something just outside it is still
  refused.
- Loopback is the task's own sandbox and nothing else: not the gateway, the model network or another sandbox.
- The Desktop browser saves nothing and phones nowhere (REL-I-004). A new policy key is covered by the image check.
- A harness that stops running fails loudly. A suite that quietly ran nothing is a finding.

### Migrations and `.sqlx`
- A migration is numbered above the highest one on the integration branch at merge time and is never applied out of
  order. Two branches that take the same number are renumbered at merge, before either lands.
- Migrations are forward only and tested from the previous release's schema with its data, not only on an empty
  database (GW-I-016). Existing rows survive and new constraints are shown to refuse what they should.
- `.sqlx` files come from `cargo sqlx prepare` only. Evidence: the prepare run, an offline build, and a stale query file
  shown to fail the check. A hand-edited query file fails review.

### Web page budgets
- Every route has a first-load JavaScript budget (WEB-U-069). A change that adds code or core-catalogue text to a route
  states that route's bytes before and after, measured on a production build.
- The core catalogue loads on every page. Prefer an existing string or a page catalogue over a new core key.
- A route left with under 1 % headroom is called out in the PR.

### Release, build and integration scripts
- Nothing deletes a directory that can hold results (`rm -rf`, a forced worktree removal) before its evidence is copied
  out.
- User text (release notes, task text) is never passed to a shell unquoted. A script refuses input it can't quote.
- A failed step prints its error and exits non-zero. A failure hidden behind `|| …`, or a summary that reads a file that
  isn't there, is a finding.
- A script that checks or tags images covers every image the tests use and compares image IDs, not tag names.
- Ref updates name the expected old value. A port is reserved on every address the service binds, loopback included.

### Dependencies
- Advisories can appear with no code change. A new ignore needs a decision record, proof the affected function isn't
  called, a guard test that fails when the ignore goes stale, and the condition for removing it.

### Tests and test IDs (every tier)
- Red first: the hand-in names the commit where the new test fails, and why.
- A test ID is claimed before use. A duplicate is fixed before the merge.
- A flaky test is "known" only after it passes alone on the same commit and images. It is never retried quietly inside
  a suite.
- Fix the bug class, not the instance: when a finding is a pattern, the author searches for its other occurrences and
  fixes or lists them in the same branch.

## 4. What reviewers skip

People and tools don't comment on:
- formatting or lint that `rustfmt`, Clippy, ESLint or the guard already checks;
- naming or style that follows the surrounding code;
- whole-file reformatting (never run a formatter over `docs/TEST-PLAN.md`);
- tests the change didn't touch, unless it breaks what they assume;
- generated files (`web/src/protocol/generated/`, `.sqlx/`) except to check they were regenerated, not edited;
- "consider adding" ideas with no failure scenario.

At most three P3 notes per review; the rest are dropped.

## 5. Findings, verdicts and rounds

A finding states the file and line, the input or state, what goes wrong, and a severity:
- **P1**: data loss, a security hole, a wrong result users rely on, or a broken release;
- **P2**: a real failure in a less common path, or a missing test for a P0/P1 row;
- **P3**: a small defect worth fixing, never a blocker.

A review hands back one list, worst first, and one verdict: merge, merge after the listed fixes, or don't merge. An
empty list is a valid result.

**Three rounds at most.** Round 1 is the full review. Rounds 2 and 3 check only the fixes and what they touched. A new
P3 in a later round becomes a follow-up, never a new round. If round 3 still has an open P1 or P2, the review stops and
the author and the technical lead step back: reconsider the approach, split the branch, or merge with a recorded
follow-up. A gate failure after approval (for example a route a few bytes over its budget) goes back with the
technical lead's chosen fix and is not a new round.

## 6. Reverts teach the reviewers

A change reverted after it merged, or a defect found later that a review should have caught, gets a short record: what
broke, which layer should have stopped it, and the rule or check that would have. The rule is added to §3 or to a
check, and the change is kept as a test case for the reviewers. The maintainers track rounds per branch, the human
review time, and reverts, and revisit this file when the numbers move.

## 7. Trial period

This process applies from 28 October 2026, to every branch handed in from then on. It is measured for two weeks, until
11 November 2026, on three numbers:
- review rounds per branch (§5);
- the technical lead's review time per branch;
- reverts and defects found after the merge (§6).

At the end of the two weeks the maintainers compare the numbers with the branches before the trial and keep, change or
drop the tiers.
