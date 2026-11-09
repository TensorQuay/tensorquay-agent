# Gate 10: Agent teams

**State:** **draft, not reached** (21 Oct 2026, for the technical lead). The Evaluation below is the technical lead's
to write. The release candidate is 16y. In the 16y integration (20 Oct), every row in this gate's
scope passed. That includes the agents suite on the real stack, 17 of 17 (16j's 12 and the five added since), and
SEC-046 with its team case.

After 16j, a real-world campaign on 16k (17 Oct) gave agent teams goals a person would set. The defects it found,
and those the live checks on the real model found after, were fixed in 16m to 16x and checked again live on 16o, 16p,
16q and 16u (below).

**The primary evidence for 16y's team behaviour** (decided on 21 Oct 2026):
- 16k's real-world team cases;
- the live checks on 16o and 16p;
- 16u's live checks L2, L2b and L3.

Since 16u, 16y changes no team code except WEB-U-415 and WEB-U-416, which the integration's E2E-AGENTS-032 covers.
16y itself is 16x plus TCP keepalive on the gateway's accepted sockets (GW-U-205, GW-I-314). The phase demo on 16j
(15 Oct) passed and is supporting evidence.

What isn't done yet:
- **a soak with agent teams in its mix: open.** Every c16 soak so far, 16y's included, has run with agent teams off.
  Off by default is ADR-0034's install switch (accepted 2 Oct 2026); keeping agent teams off for this release is still
  the Product Owner's open decision. While they stay off, this doesn't block 16y, but it isn't closed;
- the evaluation's result: "keep experimental" is the bars' reading and the technical lead's recommendation, and the
  Product Owner decides (`agent-teams.md` §3);
- the Evaluation below.

## Scope
The TEST-PLAN §17 Gate 10 row (set on 12 Oct 2026; the rows added since, 21 Oct) holds:
- every P0 and P1 row on agent teams: those that name ADR-0034, and the agent-team features and the Product Owner's edge
  cases of 7 Oct that don't;
- the phase demo.

The evaluation is the gate's measurement.
- **P0 (8):** CORE-U-078; GW-I-214, GW-I-252; RT-I-039; SEC-037, SEC-038, SEC-046 (with its team case); WEB-U-228.
- **P1 (91):** CORE-U-079, CORE-U-080; E2E-AGENTS-021…030, E2E-AGENTS-033…035; GW-I-106…111, GW-I-195…198,
  GW-I-202…207, GW-I-210…212, GW-I-215, GW-I-248…251, GW-I-253…256, GW-I-258, GW-I-260, GW-I-275, GW-I-277, GW-I-302,
  GW-I-311; GW-U-069, GW-U-125…128, GW-U-130…136, GW-U-139, GW-U-142, GW-U-145, GW-U-168, GW-U-186, GW-U-201;
  LLM-U-054; PROTO-U-023, PROTO-U-034, PROTO-U-035; RT-I-042, RT-I-044; SEC-039; TK-U-042; TOOL-U-036, TOOL-U-037,
  TOOL-U-056, TOOL-U-058; WEB-E2E-062; WEB-U-220…227, WEB-U-229, WEB-U-308, WEB-U-326.
- **The phase demo** (DEV-PLAN Phase 10, slice 7): an agent team end to end, as E2E-AGENTS-020 scripts it: a person
  asks a task to lead, the lead starts helpers, they report and hand files back, and the team ends with its end note.
  It runs on the real model in the technical lead's live session.
- **The measurement:** the evaluation (`agent-teams.md` §2, proposal J), a single agent against an agent team at the
  same budget. It doesn't pass or fail the gate. It decides the guides' advice and whether agent teams stay
  experimental. Agent teams stay behind `[agent_teams] enabled`, off by default, either way.

**The rows added on 21 Oct** are the P1 rows on agent teams written since 12 Oct. Each has its
tests, and each passed in the 16y integration:
- a team's budget counts a call that was cut off or reported no usage: LLM-U-054, GW-U-186, GW-I-275 (the
  evaluation's night 2);
- `read_helper` says where the helper is now: TOOL-U-058, GW-I-277 (night 2);
- a turn the runtime took, with its run not yet started, is in flight: GW-U-201 (16o's live check: a `my_files`
  helper was never started);
- Stop team stops the lead too: GW-I-302 (16k's B6b);
- the gateway hands back the work of a helper stopped before it reported: GW-I-311 (16o's W1);
- on the real stack: two helpers on the lead's files take turns, said as such (E2E-AGENTS-033); a team's ends in words
  (E2E-AGENTS-034); a stopped helper's hand-back (E2E-AGENTS-035);
- the page that started a team shows its strip live, with no reload: WEB-U-326 (c16's live check LC-01; written on
  12 Oct, and added to the row on 21 Oct). Its tests, in `team-page.test.tsx`, passed in 16y's web unit run (27 of 27).

**Rows that touch agent teams but stay out:**
- GW-U-078 (P1): the row is about messages held behind a run in any task, and its team case (a team member's first
  turn with a message behind it) is one of its several inputs. It passed in 16y's unit run.
- GW-I-219 (P1): Take over is a computer-use feature in Gate 11's scope, and an agent team's wake queued meanwhile is
  one of its several inputs. It passed in the 16y integration.

**Other evidence, not criteria:**
- The P2 rows E2E-AGENTS-020, WEB-E2E-024, WEB-U-230 and WEB-U-327, as before.
- The P2 rows added for 16k's and the live checks' findings, which all passed in the 16y integration:
  - E2E-AGENTS-031 and E2E-AGENTS-032;
  - GW-I-298…301, GW-I-303, GW-I-304, GW-I-306, GW-I-307, GW-I-312;
  - the team rows among WEB-U-387…416.

## The 16y integration (20 Oct 2026, its summary from 20:08 to 23:16 +08:00)
**Source:** the integration's summary `int-5a59ad1d5-summary.log`, with each step's log in `int-5a59ad1d5-*.log`.
Failed gates 0, cut 0; one failed step, release-mode performance (below).

- **Unit, DB and web unit:**
  - unit 2,643 of 2,643 (`check`);
  - DB and the other ignored suites 2,929 of 2,929 (`ignored`);
  - vitest 2,823 of 2,823.
- **The agents suite** (`int-5a59ad1d5-demo-agents.log`): **17 passed** in 6.9 min:
  - E2E-AGENTS-020, end to end and again across a gateway restart;
  - E2E-AGENTS-021…030, the Product Owner's edge cases;
  - WEB-E2E-062;
  - added since 16j: E2E-AGENTS-031 (a message to a leading lead), 032 (a lead that ends its turns without `done`),
    033 (two helpers on the lead's files), 034 (a team's ends in words) and 035 (a stopped helper's hand-back).
- **SEC-046 with its team case** (`int-5a59ad1d5-host-keys.log`) passed:
  - on a shared workspace, the teammate's run held none of the owner's keys;
  - in a team, the owner's helper held the owner's keys as its lead did, and its call with the lead's placeholder was
    refused `foreign_sandbox` with nothing sent;
  - the teammate's helper on the shared files held none of the owner's keys.
- **The mocked web e2e:** 592 passed, 0 failed, including `teams.spec.ts` and `team-pending.spec.ts`.
- **Two failures outside this gate,** both performance tests in code 16y doesn't change. Both passed in the 16w and 16x
  integrations, and both will rerun on 16y in a quiet session after its soak (Thursday 22 Oct, about 00:30 to 05:30):
  - the gateway's event pipeline throughput (PERF-002, release mode): the 10,000 events/s row rate wasn't offered.
    Six bursts went out at 2,882 to 8,495 events/s beside other jobs' load, and lag p99 was 6.2 s against 100 ms
    (p50 2.5 ms). Nothing was lost: 550,000 events, all stored;
  - the microVM's workspace flush (`perf_013`): p95 91 ms against 50 ms, p50 2.5 ms.
- **Everything else** passed, including the Desktop suite, the Firecracker suites (107 of 108, the flush above), the
  real-stack browser suites and the gate demos.

## The real-world campaign (16k, 17 Oct 2026)
The Product Owner asked for real-world end-to-end use: give a team a goal and see whether it works together to
finish it.
- **Setup:** a fresh 16k install with agent teams on, the real local model (27B, with images), and a short allow-list
  of public sites. Every case was a task typed in the browser as a person would. Each was judged on its screens, its
  files and the run's own records.
- **The team cases:**

  | Case | Goal | Verdict |
  |---|---|---|
  | B1 | Three helpers each research one database on Wikipedia; the lead writes a comparison with a pick | PASS: three helpers at once on their own copies; the comparison picks DuckDB |
  | B2 | Helpers build a small todo CLI (storage, CLI, tests); done when the tests pass | PARTIAL: it built and its 29 tests pass, but the lead never called `done`, so the finish check never ran |
  | B3 | Two helpers total sales and costs from two uploaded CSVs; a profit chart and summary | PASS: the totals match the known answer to the cent |
  | B4 | One helper reads a headline with the Desktop, the other writes a script that prints it | PASS |
  | B5 | B1, with "Add MySQL as a fourth database" sent mid-run | PARTIAL: the message waited until the team ended |
  | B6, B6b | Stop team mid-run | PASS: the helpers stopped and the files were kept, but the lead worked on alone |
  | B7 | B1 with two helpers per lead and a small budget | PASS: the limit held, and the budget end was said in words |
  | B8 | Waiting and wake, live | PASS |
  | B9 | An open trip-planning goal, no split asked for | By design: the lead worked alone (helpers are used when asked) |
  | B9b | B9, asking for helpers, on the default 2M budget | FAIL: the budget ran out and nothing came back |
  | B9c | B9b with a 6M budget | PASS: two helpers in parallel; one coherent plan built from both reports |
  | B10 | An open goal: a café web page, checked in a real browser | FAIL: the page's own local server was blocked by the egress proxy |
  | B10b | B10, asking for helpers | PASS: two helpers took turns on the lead's files; the team's test passes |

- **Its agent-team defects, all fixed by 16x:**
  - a task's own loopback server went to the egress proxy (TOOL-U-060, SBX-U-018, SBX-C-023; 16m);
  - a helper that reported read Stopped when the lead ended the team (GW-I-301); a `my_files` lead said "No files
    changed" above the files handed back (WEB-U-389); a raw heredoc as a step label (WEB-U-390); all 16m;
  - a lead could end its turn without `done` and read Done while its team ran (GW-I-306, WEB-U-398; 16n and 16o);
  - a team out of budget handed back nothing (GW-I-304, WEB-U-394; 16n). The gateway now hands back a stopped
    helper's work itself (GW-I-311, WEB-U-403; 16p and 16q);
  - Stop team didn't stop the lead (GW-I-302, WEB-U-391; 16n);
  - a person's message waited for the whole team (GW-I-307, WEB-U-402, TOOL-U-065; 16n);
  - a wait for the lead's files read "Waiting for a free slot" (GW-I-303, GW-U-199, WEB-U-393; 16n);
  - a failed start, a traceback, durations and the end note's Markdown were shown raw (WEB-U-395, WEB-U-396,
    WEB-U-399; 16n);
  - a lead whose last step kept failing read red Failed while its helpers worked (WEB-U-401; 16n).
- **Model mistakes, not product defects:**
  - in B2 the lead never called `done`;
  - in B5 and B7 the lead named a profile that doesn't exist, then retried without it;
  - in B9b the lead ignored the 80 % notice and waited, instead of asking the helpers for what they had.

## The live checks on the real model (16o to 16x, 18 and 19 Oct 2026)
Each fix was checked on a fresh install of its candidate with the real model. Every verdict rests on the run's own
records or the screen, never on the model's words.

| Candidate | Agent-team cases | Result |
|---|---|---|
| 16o (18 Oct) | Helpers that reported read Done; step labels; the loopback server; a person steers a running team; a lead can't quietly end its turn; a wait on the lead's files said as such; Stop team stops the lead; the words of a team's ends | PASS |
| 16o | A team out of budget hands back its work | FAIL: helpers didn't act on the 80 % ask before they were stopped. Fixed by the gateway's own hand-back (GW-I-311, 16p) |
| 16o | No "No files changed" above the files handed back | Not judged: a `my_files` helper was never started. Fixed in 16p (GW-U-201) |
| 16p (18 Oct) | That `my_files` team again; steering; a lead's turn; a failed lead waits for its helpers (live for the first time) | PASS |
| 16p | The gateway's hand-back | It works, but it listed the runtime's own files as too large. Fixed in 16q |
| 16q (18 Oct) | The gateway's hand-back, with real files | Correct: each helper's hand-back said truly that it changed no files. Attaching files is proven by E2E-AGENTS-035 |
| 16u (18 Oct) | A team's pages opened mid-team never read Done early (WEB-U-404…406); Stop team with turns still waiting (GW-I-312) | PASS |
| 16v to 16x (19 Oct) | Computer-use cases only (Gate 11) | — |

16w and 16x add two more fixes to when a lead's page may read Done (WEB-U-415, WEB-U-416). They are proven in the
integration by E2E-AGENTS-032. On 16x, its `header-samples.json` holds 39 samples of the header through a team's run,
none of them Done; it passed again on 16y.

**Not exercised live:** the reminder to a lead that ends its turn without `done`. The leads in 16o's and 16p's runs
never needed one. E2E-AGENTS-032 covers it on the real stack.

## The phase demo (16j, 15 Oct 2026, the technical lead's live session): supporting evidence
**PASS** on the real model, as E2E-AGENTS-020 scripts it:
- a lead started four helpers, each with its own brief;
- each helper ran its own tests and reported, and the lead wrote the package's last file;
- the full suite passed (27 tests), and the lead called `done` with the finish check set, so it went through only on
  the check's success;
- the end note showed the summary, each member's share, each helper's report and the files handed back. It used
  287,731 of 2,000,000 tokens in about 10 minutes.

One helper caught a wrong example in the lead's brief and built the rule the brief stated.

**Its findings:**
- **Fixed in 16k:** the lead's wake showed as the person's own waiting message, with Cancel (GW-I-298…300,
  WEB-U-387, WEB-U-388).
- **Fixed in 16n:** a wait for the lead's files read "Waiting for a free slot" (WEB-U-393).
- **Open for c17:** the lead gave all four helpers `my_files`, so they took turns on its workspace instead of working
  at once. That is by design (ADR-0034 point 6), but the parts were independent. `start_member`'s guidance should
  steer independent parts to `own_copy`; then measure again.

The team code changed after 16j, so the demo is supporting evidence only (decided on 21 Oct 2026). The primary
evidence for 16y is listed at the top of this page. On those runs, real-model teams ended by `done` on 16k (B1, B3,
B4, B9c, B10b) and in the live checks on 16o and 16p, and E2E-AGENTS-020 passed on 16y.

## Earlier integrations (16d to 16j, 12 to 14 Oct 2026)
In each, every row then in this gate's scope passed:

| Candidate | Dev | Agents suite | SEC-046 team case | Notes |
|---|---|---|---|---|
| 16d | `3aa5a170` | 12 of 12, its first run on a c16 build | not yet written | GW-I-198's acceptance check on `done` built and passed |
| 16g | `bac51ffc2` | 12 of 12 | passed, its first integration | |
| 16i | `a94681cf4` | 12 of 12 | passed | the page pass's web fixes; none an agent-teams row |
| 16j | `c4cfb80c0` | 12 of 12 | passed | two Desktop fixes; two Desktop test flakes, outside this gate |

**Before 16d,** the technical lead's live check (c16 `298fddf1`, 11 Oct) found the feature working end to end with
the real model. Its three findings were fixed before 16d.

**The page pass** (16f, retaken on 16h and 16i, 14 Oct) checked a team lead's page and the task list with a team, at
1280 and 390, light and dark, in English and Simplified Chinese. All were ok, and none of its findings was in agent
teams.

## What's still open
1. **The soak with agent teams in its mix.** Every c16 soak has run with agent teams off: 16e's (passed, 12 to 13 Oct),
   16j's (passed, 14 to 15 Oct), 16u's (stopped at 9 h 33 min on 18 Oct, once 16u could no longer
   be the candidate), 16x's (passed by the growth rule, with an idle-connection leak found and fixed as 16y) and 16y's
   (running, to Thu 22 Oct). ADR-0034 adds resources that build up over time: helpers' sandboxes and copies, the team
   log, wakes, and budget reservations (GW-I-252). No long run has had a team in it. Adding teams to the soak's mix is a
   harness change (the soak has no setting for teams) and a new 24-hour run. Agent teams are off by default (ADR-0034's
   install switch), and keeping them off for this release is the Product Owner's open decision. While they stay off,
   this doesn't block 16y, but it stays open.
2. **The evaluation's result** (`agent-teams.md` §3, now filled from nights 1 to 3 and the blind judging):
   - no difference between a team and a single agent is shown either way, on 7 and 13 tasks;
   - teams formed in only 4 of 20 team attempts with the brief line;
   - against the bars accepted in §2, that reads as **keep experimental**. That is also the technical lead's
     recommendation; the Product Owner decides.

**Closed since 14 Oct:**
- the phase demo on 16j (above), as supporting evidence;
- the rows written since 12 Oct, added to the scope (21 Oct);
- the evaluation's judging and write-up;
- the 16k campaign's and the live checks' agent-team findings, all fixed and checked.

**Release evidence beside this gate, not its criteria:**
- **Gate 5 on 16x** (20 Oct). Every run passed its row with 0 harness failures:
  - E2E-CMP-003, 50 of 50 against the reference's 49 of 50;
  - E2E-WAVE-001, 58 of 60;
  - the compaction demo, 1 of 1;
  - E2E-WAVE-002 at 8 at once, 60 of 60;
  - E2E-FALLBACK-004, 17 of 30. That was a drop from 16j's 27 of 30. A rerun on 16y gave 26 of 30, and replaying the
    same requests ruled out the binaries.
- **Gate 9:** on 16x (20 Oct), PERF-001, PERF-010 and PERF-020 show no regression, and REL-I-001…003 passed. The
  phases that a busy host disturbed or that didn't run (PERF-011's run failed on the harness) will run on 16y in a
  quiet session after its soak (Thursday 22 Oct, about 00:30 to 05:30).

## Known limits (v1.1)
- **Off by default.** `[agent_teams] enabled` in `gateway.toml` is off, and off leaves no trace (WEB-U-228).
  Administrators turn it on per install.
- **Depth 1.** Helpers never get `start_member`, and a lead can't become a helper of another team (ADR-0034, GW-U-069).
  Swarms (Phase 12) link teams; they don't nest them.
- **Not on CPU.** On a computer without a suitable GPU, each model call took 1½ to 5½ minutes in 16d's first hour on
  the README's default small model. A team multiplies those calls, so teams are impractical there. The user guide's
  Running without a GPU section says so.
- **A model has to choose to lead.** On night 1 the model never started a helper without the brief line. With it, a
  helper was used in 2 of 7 attempts on night 2 and 2 of 13 on night 3. On 16k's open goal (B9), the lead worked alone
  until the task asked for helpers.
- **Helpers on the lead's files take turns** (ADR-0034 point 6). A lead that gives independent parts `my_files` gets
  no parallel work (the phase demo).
- **A team's budget.** On 16k a two-helper research team spent the default 2M tokens in 7 minutes (B9b), and the same
  goal finished in 3.73M (B9c). Out of budget, a team now hands back what its helpers had (GW-I-304, GW-I-311).

## Evaluation (the technical lead)
*Left to the technical lead.*
