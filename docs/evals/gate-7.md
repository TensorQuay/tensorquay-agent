# Gate 7: Channels

**State:** **reached on release candidate c15**, by the technical lead's verdict (8 Oct 2026 06:12). **Every row
passed on c15 itself**: the
integration of C15 = dev `b5caac76` ran every row on the candidate's own images, its web image included, with nothing
failed. That closes c14b-rc's one open item, which was the page's evidence on the candidate's own web image (c14b-rc's
server came from 14b). The verdict and its limits are below.
c14b-rc's refresh, 13h's, 13g's and the history from 28 Sep follow.

## Scope
The TEST-PLAN §17 Gate 7 row: E2E-CHANNEL-016; PROTO-U-020; GW-U-065; GW-I-032/033, GW-I-101/102/103; TOOL-U-019;
SEC-015, SEC-035; WEB-E2E-012. Also the Phase 7 build list and demo (DEV-PLAN §4), PRD FR-31 and ADR-0014. Parking a run
that waits on a channel (GW-I-104) is deferred by ADR-0014 point 7 and is not part of this gate.

## Release candidate c15 (8 Oct 2026)
**Source:** the integration of C15 = dev `b5caac76` (8 Oct 2026, 01:23 to 04:24 +08:00, logs `int-b5caac76-*`), on
the candidate's images from its tag file `candidate15-tags.txt`:
- gateway `8fe72138b44c`, web `c21dae74e9d7`, runtime `489ab9525b4c`, Workstation `797499325f94`, fake model
  `03718db2e50f`;
- all built from `b5caac76` at 01:22, runtime protocol revision 13.

A row passes only if each of its test IDs ran and none failed. Each ID's Rust tests were found by the ID in a `TEST:`
line or comment above the test, or in the file's header, and each was looked up in the unit step (2,355 of 2,355
passed) and the database step (2,581 of 2,581 passed). Neither step has a failure. The counts are test functions,
counted another way than c14b-rc's cases.

| ID | c15 | Evidence |
|---|---|---|
| PROTO-U-020 | pass | 6 tests, all passed |
| GW-U-065 | pass | 7 tests, all passed |
| GW-I-032, GW-I-033 | pass | 13 and 5 tests, all passed |
| GW-I-101, GW-I-102, GW-I-103 | pass | 1, 5 and 4 tests, all passed |
| TOOL-U-019 | pass | 14 tests, all passed |
| SEC-015 | pass | 4 tests, all passed |
| SEC-035 | pass | 5 tests, all passed |
| E2E-CHANNEL-016 | pass | `tests/e2e/gate7.py --person api` (`gate7-api`) and `--person page` (`gate7`), both: 14 messages, 2 proposals accepted and 3 vetoed, 2 claims and 2 reports, Planner 9 and Builder 5 calls; the page saw all 14 |
| WEB-E2E-012 | pass | the `gate7` step's page run on c15's web image: `web/e2e/channel-stack.spec.ts` ("two agents and a person decide, veto and share a task on a channel, live"), 1 passed |

Beside the gate, on the same images:
- the `channel-members` stack suite passed;
- the fake-front browser suite passed: 546 passed, 25 skipped, none failed;
- the full web check passed (2,462 tests).

The channel rows of the c15 page pass are after the soak (no browser runs beside it).

### Evaluation on c15 (8 Oct 2026)
**Verdict: Gate 7 is reached on c15 (technical lead, 8 Oct 2026 06:12).** Every row of the TEST-PLAN §17 Gate 7 row passed on the candidate's
own images, the demo both over the API and from the page.

**Known limits**, as on c14b-rc:
- the demo is scripted on the fake model, so it proves the channel's mechanics, not a real model's judgement;
- parking a run that waits on a channel (GW-I-104) is deferred by ADR-0014 point 7;
- the first hour on c15 (8 Oct, 05:35 to 05:41) didn't touch channels: its findings (G1 to G3) are about web search
  and Internet access.

## Release candidate c14b-rc (5 to 6 Oct 2026)
**Source:** candidate 14b's integration on dev `16589e55` (5 Oct 2026, its unit step to its Gate 7 runs 00:02 to 01:18 +08:00,
logs `int-16589e55-*`; its tag file names
gateway `fde0b91bf88b`, runtime `347b895b203d`, Workstation `84ff9c47aa24`, fake model `272dfcee3718`, web
`488858e9b918`), each row checked against the logs: a row passes only if each of its test IDs ran and none failed.
c14b-rc is those images with the web image rebuilt from dev `101ffeca` (`83a8d1ad493f`). The channel code is the same
in both: none of the 46 files changed from `16589e55` to `101ffeca` is channel code. 13h's open
changes (`d1dae839`, `ae236763`) are in `16589e55`, so they are measured here.

| ID | c14b-rc | Evidence |
|---|---|---|
| PROTO-U-020 | pass | unit and database steps: 12 cases, none failed (unit 2,323 of 2,323, database 2,540 of 2,540) |
| GW-U-065 | pass | unit and database steps: 11 cases (the member and daily limits, the daily limit shared with agent-team logs since `d1dae839`) |
| GW-I-032, GW-I-033 | pass | the database step: 7 and 5 cases (the people's channel list without agent-team channels since `d1dae839`) |
| GW-I-101, GW-I-102, GW-I-103 | pass | the database step: 1, 2 and 3 cases |
| TOOL-U-019 | pass | unit and database steps: 22 cases |
| SEC-015 | pass | the database step (GW-I-103's test, its second tag) |
| SEC-035 | pass | unit and database steps: 2 cases |
| E2E-CHANNEL-016 | pass on 14b's images; **from the page, to run on c14b-rc's web** | `tests/e2e/gate7.py --person api` (`gate7-api` step) and `--person page` (`gate7` step), both: 14 messages, 2 proposals accepted and 3 vetoed, 2 claims and 2 reports, Planner 9 and Builder 5 calls; the page saw all 14 |
| WEB-E2E-012 | pass on 14b's web; **to run on c14b-rc's web** | the `gate7` step's page run: `web/e2e/channel-stack.spec.ts` ("two agents and a person decide, veto and share a task on a channel, live"), 1 passed |

Beside the gate, the `channel-members` stack suite passed on 14b's images (members added, a role changed, a person
removed; the channel ended, then deleted). The page pass on 14b's web passed the channel rows (a channel from its
purpose, a proposal, a vote and its decision, a channel task) with one P2: a channel named from its purpose showed the
purpose twice. The first hour on 14b found one channel P2 (F5): the panel's **Tasks** tab meant the channel's own work
items while the person had just added an agent task to the channel.

**What is missing before the gate can be reached, and why:**
1. **The page runs on c14b-rc's own web image** (`83a8d1ad493f`): E2E-CHANNEL-016 from the page, which is WEB-E2E-012,
   and the `channel-members` suite beside them. 14b's runs used 14b's web image; the channel page's code is unchanged,
   but the gate's evidence must be on the candidate itself. They run after the 24 h soak (no stack runs beside it), from
   a fresh export of `101ffeca`, so the specs expect c14b-rc's words: `tests/e2e/gate7.py --person page`, `--person
   api` for a complete record, and `deploy/tests/demo.py --suite channel-members`, with c14b-rc's image IDs.
2. **The technical lead's evaluation** on c14b-rc, recorded at the end of this page.

### Evaluation on c14b-rc (draft for the technical lead, 7 Oct 2026: to review, complete and sign after the soak)
**Proposed verdict: not reached yet.** Every row passes on c14b-rc's server and runtime; what is left is the page's
evidence on c14b-rc's own web image (item 1 above). If those runs pass, the proposed verdict is **Gate 7 is reached**,
with the known limits below.

Checked row by row against the integration's logs (`int-16589e55-*`). Each ID's tests were found by their `TEST:`
tag (or, for TOOL-U-019 and SEC-035, the file header that names the ID) in `16589e55`, and each was looked up as
`PASS` in the database step (`--run-ignored all`, 2,540 of 2,540 passed); neither that step nor the unit step
(2,323 of 2,323) has a failure. The counts below are test functions; the table above counts cases another way, and
no count disagrees with a pass.

| ID | Checked | Finding |
|---|---|---|
| PROTO-U-020 | 6 tagged tests | all passed |
| GW-U-065 | 7 tagged tests (member limit, majority and tie, veto, human-only, the daily message limit) | all passed |
| GW-I-032 | 7 tagged tests | all passed |
| GW-I-033 | 5 tagged tests | all passed |
| GW-I-101, GW-I-102, GW-I-103 | 1, 2 and 4 tagged tests | all passed |
| TOOL-U-019 | 14 tests in the two files the ID heads | all passed |
| SEC-015 | 1 tagged test (GW-I-103's, its second tag) | passed |
| SEC-035 | 5 tests in the three files the ID heads | all passed |
| E2E-CHANNEL-016 | `gate7-api` and `gate7` logs: gateway `fde0b91b` (c14b-rc's), Workstation `84ff9c47` | both passed: 14 messages, 2 accepted and 3 vetoed, 2 claims and 2 reports, Planner 9 and Builder 5 calls; the page saw all 14. **From the page on c14b-rc's web: to run** |
| WEB-E2E-012 | the `gate7` log | 1 passed on 14b's web (`channel-stack.spec.ts`, 1.1 s). **On c14b-rc's web: to run** |

- **To check before signing:** the three page runs of item 1 on c14b-rc's web image (`83a8d1ad493f`), with their
  image IDs and times; then this verdict.
- **Outside the gate rows, on c14b-rc:** a channel's panel calls its own work items **Tasks** while a person's added
  agent task is a member (first hour F5, P2), and a channel named from its purpose shows the purpose twice (page pass,
  P2). Both are fixed on dev for the next release (`a19bab38`, `f8391311`); neither changes a row.
- **Known limits, accepted at 28 Sep and still so:** no parking for a run waiting on a channel in v1.0 (ADR-0014 point
  7). Agent teams are in c14b-rc but off by default and not part of v1.0. *(to confirm with the lead: whether the
  28 Sep list's other limits, the `quorum` and `lead` rules, `agent_spawn` and the member-change stream hint, still
  stand as written)*

**Since c14b-rc, on dev** (none on a candidate yet; they change no row, but the next candidate's page run takes them):
| Change | Commit | Rows it touches |
|---|---|---|
| A channel's own work items are **to-dos** (the first tab, **Add a to-do…**, "Took on to-do #4"); agent tasks stay tasks (first hour F5) | `a19bab38` | WEB-E2E-012 (its spec's words) |
| A channel's purpose shows once under its title, carrying on from a cut title (page pass C2) | `f8391311` | the channel page; no row |

## Candidate 13h (1 Oct 2026)
**Source:** the technical lead's integration run on `5b79a932` (1 Oct 18:30–23:56 +08:00, logs `int-5b79a932-*`), each
row checked against the logs: a row passes only if each of its test IDs ran and none failed.

| ID | 13h | Evidence |
|---|---|---|
| PROTO-U-020 | pass | unit and ignored nextest steps |
| GW-U-065 | pass | unit and database steps |
| GW-I-032, GW-I-033, GW-I-101, GW-I-102, GW-I-103 | pass | the database step (`ignored`) |
| TOOL-U-019 | pass | unit and ignored nextest steps |
| SEC-015 | pass | the database step (it is the GW-I-103 test's second tag) |
| SEC-035 | pass | unit and ignored nextest steps |
| E2E-CHANNEL-016 | **pass** | `tests/e2e/gate7.py --person page` (`gate7` step): 14 messages with fixed seqs, 2 proposals accepted and 3 vetoed by the gateway's decisions, 2 claims and 2 reports on the board, Planner 9 and Builder 5 messages, and the page saw all 14 |
| WEB-E2E-012 | **pass** | the same run: the person acts from the channel page, `web/e2e/channel-stack.spec.ts` ("two agents and a person decide, veto and share a task on a channel, live"), 1 passed |

E2E-CHANNEL-016's row takes the person over the API or from the page; the integration runs the page, which is also
WEB-E2E-012. The `channel-members` stack suite passed too.

**What was left at 13h** (superseded by c14b-rc above): the technical lead's evaluation. 13h's open changes since
(`d1dae839`, the people's channel list and the shared daily limit; `ae236763`, the channel page's stack spec waiting for
a start) are measured on c14b-rc.

## Candidate 13g (1 Oct 2026)
**Source:** the technical lead's integration run on `86bdd6ee` (1 Oct 03:59–07:32 +08:00, logs `int-86bdd6ee-*`) and
the TEST-PLAN audit of it (1 Oct 2026), each row checked against the logs.

| ID | 13g | Evidence |
|---|---|---|
| PROTO-U-020 | pass | unit and ignored nextest steps |
| GW-U-065 | pass | unit and database steps (the member and daily limits included) |
| GW-I-032, GW-I-033, GW-I-101, GW-I-102, GW-I-103 | pass | the database step (`ignored`) |
| TOOL-U-019 | pass | unit and ignored nextest steps |
| SEC-015 | pass | the database step |
| SEC-035 | pass | unit and ignored nextest steps |
| WEB-E2E-012 | **not run** | `web/e2e/channel-stack.spec.ts`: the `channels` stack suite is not among the integration's demo suites |
| E2E-CHANNEL-016 | **not run** | `tests/e2e/gate7.py`: the Gate 7 demo is not an integration step |

The `channel-members` stack suite, a channel page flow outside this row, passed on 13g.

**What is missing before the gate can be reached:**
1. The `channels` stack suite (WEB-E2E-012) on the candidate's images.
2. The Gate 7 demo (E2E-CHANNEL-016) on the candidate's images, once over the API and once from the page.
3. Both added to the integration, so every candidate runs them (with the other stack suites it doesn't run yet: the TEST-PLAN audit's
   audit lists push, admin, approvals, connector, profile, ux-review and two key-use suites).
4. The technical lead's evaluation on that candidate, recorded at the end of this page.

## History (28 Sep 2026)

### How the status was established
- **The lead's full integration run on `a0bf5ef`** (2026-09-28 02:18–02:57), on images built from that commit (gateway
  `sha256:66494e4f0f32…`, Workstation runtime `sha256:2231f26c9fd4…`, protocol revision 8). It runs the row's unit and
  database tests, including GW-I-103's collision case against PostgreSQL. Every channel test passed.
- **The demo run twice on the same images** from an exact `a0bf5ef` tree: once with the person acting over the API
  (02:13–02:17) and once from the web page (02:17:31–02:17:55).
- **The demo run twice more on candidate 5**, from an exact `545c3d0` tree, on images built from that commit (gateway
  `sha256:47241c66aeb0…`, Workstation runtime `sha256:0146d8558f42…`, protocol revision 8). This script no longer gives
  the Builder its extra read (`10a50c7`), so the agents' posts fall on the same turns as in the run that found the key
  collision.
- **GW-U-065 on candidate 6** (`a14afc0`, which carries `c37cfe4` and `7a38f8d`): the lead's full integration run, its
  database step (2026-09-28, from 15:56; 1,878 tests run, 1,878 passed). Its three limit tests passed there: the 5,001st
  message of a day is refused with `429 channel_rate` and takes no seq, a decision still posts, and posting resumes once
  the window moves on; the 33rd member is refused with `409 channel_full` while a role change at the limit is not; and
  two adds racing for the last place leave 32 members and one `channel_full`. The row's evaluator tests passed in the
  unit step (from 15:51), and so did every other channel row's tests in the same run.

### Per ID
| ID | Status | Evidence |
|---|---|---|
| PROTO-U-020 Channel message types | pass | the message types, posts, roles, bounds and read filter in `tqa-types`, incl. a task's claim limit |
| GW-U-065 Decision evaluator | pass | majority, human and veto as a pure evaluator; the member limit (33rd refused, a race for the last place leaves 32) and the daily limit (ADR-0014: 5,000 messages a day, `429 channel_rate`), both enforced and tested since candidate 6 |
| GW-I-032 Channel log | pass | contiguous seqs under concurrent posts; a refused post takes no seq; the people API |
| GW-I-033 Claims and decisions | pass | 8 concurrent claims on limits 1, 3 and all: exactly 1, 3 and 8 win; a decision in the deciding vote's transaction; the timeout sweep |
| GW-I-101 Channel stream | pass | server-sent events with `Last-Event-ID` replay, then live |
| GW-I-102 Visibility, ending, deletion | pass | a stranger gets 404; an ended channel is read-only; deletion by the owner or an administrator, audited |
| GW-I-103 Runtime routes | pass | runtime tokens bound to the sandbox's address; membership checked on each call; the long poll; a run's post key scoped to its conversation (two of one person's agents no longer collide) |
| TOOL-U-019 Channel tools | pass | the four tools' schemas through the registry (optional arguments left out), a non-member refused, a cancelled wait ending within 200 ms |
| SEC-015 Author from the token | pass | a forged author in a post is dropped; a session or a garbage token on the runtime routes gets 401 |
| SEC-035 Channel content is untrusted | pass | each message reaches an agent inside an untrusted-content block the body cannot close |
| WEB-E2E-012 The person on the page | pass | the demo's person on the channel page: live tallies, a vote, a veto, a claim from the board and a report |
| E2E-CHANNEL-016 The demo | pass | both runs below |

**Totals:** 12 pass, 0 partial, 0 missing (on candidate 6; the evaluation completes on candidate 7).

### Demo
Two scripted agents (the Planner and the Builder, each a real run in its own sandbox with the fake model) share one
channel with a person. Both agents propose. The person accepts #2 by majority (2 of 3 voting members) and vetoes #3. The
Planner, whose proposal won, posts a task open to 2 claimants, claims it and reports. The person claims the second
place from the task board and reports. The driver checks the channel log (14 messages with fixed seqs and words), the
decisions, the task board, and each agent's tool calls (only channel tools, in the scripted order). The page run also
checks that every post reached the page live, once and in seq order.

| Run | Person | Time | Result |
|---|---|---|---|
| 1 | over the API | 02:13–02:17:30 | passed: 14 messages, #2 accepted, #3 vetoed, 2 claims, 2 reports, Planner 9 calls, Builder 6 |
| 2 | from the page | 02:17:31–02:17:55 | passed: the same, and 14 posts seen on the page |
| 3 | from the page, `545c3d0`, no extra read | 09:16:04–09:16:24 | passed: 14 messages, #2 accepted, #3 vetoed, 2 claims, 2 reports, Planner 9 calls, Builder 5, 14 posts seen on the page |
| 4 | over the API, `545c3d0`, no extra read | 09:16:44–09:17:10 | passed: 14 messages, #2 accepted, #3 vetoed, 2 claims, 2 reports, Planner 9 calls, Builder 5 |

Runs 1 and 2 used the `a0bf5ef` images above; runs 3 and 4 the candidate-5 images. The Builder makes 5 calls in runs 3
and 4 instead of 6 because its extra read is gone.

### What the demo found (fixed before these runs)
- A profile's tool allow-list could not name the channel tools (ADR-0014 point 4): fixed (`204a443`).
- A run's channel post key was scoped to the person, not the conversation, so two of one person's agents posting on
  the same turn collided (the second post was refused): fixed (`16279b5`, GW-I-103). The demo's script gave the
  Builder one harmless read, added while the fix was pending, so runs 1 and 2 did not exercise the collision
  themselves; GW-I-103's database test does. The read was then removed (`10a50c7`), and runs 3 and 4 on `545c3d0`
  passed without it: every post of both agents was stored, in order.
- The channel tools' schemas refused calls that leave optional arguments out once checked through the registry:
  fixed (`1932046`).

### Evaluation (technical lead, 2026-09-28)
**Correction (2026-09-28):** the first verdict below said every row passes. A later API-reference check found that
GW-U-065's daily message limit is neither enforced nor tested, so that row is partial and the gate is not reached until
the limit and its test are merged.

**Update (2026-09-28):** the limit and its tests are merged (`c37cfe4`, `7a38f8d`) and passed on candidate 6, so every
row passes. The evaluation completes on candidate 7's integration run, which the technical lead records here.

**First verdict (superseded): Gate 7 is reached.** Every row passes on `a0bf5ef`, and the demo passed both with the person over the API
and from the page.

- **Checked by the lead:** the integration run above and both demo records, with their images and times.
- **Confirmed on candidate 5 (`545c3d0`):** runs 3 and 4 passed without the Builder's extra read, so the demo itself now
  exercises the post-key fix. Candidate 5's full integration run (2026-09-28 08:05–08:57) passed every channel test.
- **Outside this gate, open in the same integration run:** GW-I-036 (a parked run resumes on the real stack) failed
  once: the session parked but the run's `run.parked` event did not reach the stream in time. It then passed 3 of 3
  alone. It is not a channel row, and it is being investigated with the engine and pressure diagnostics that run
  captured.
- **Known limits, accepted:** no parking for a run waiting on a channel in v1.0 (ADR-0014 point 7). Team runs, the
  `quorum` and `lead` rules and `agent_spawn` are v1.1 (FR-32). The member-change stream hint is not sent yet.
