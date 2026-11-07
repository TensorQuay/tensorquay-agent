# Gate 1: Walking skeleton

**State:** **reached**, by the technical lead's evaluation below (12 Oct 2026, dev `8fa48152`). Until then the page was a draft. Phase 1 was built and imported on 23 and 24 Sep 2026
(the first import `a50bac90`), before this folder had pages, and no Gate 1 report was written then. This page
assesses the gate on the v1.0 release's evidence: 15b's full integration (dev `23112f6b`, 10 Oct 2026, 14:56 to
17:51) and, for one real-stack spec, 15c's web run (15b's server with the web image `28c0f621…`; 15c is on main as
`1586fd9f`). On that evidence **all 36 IDs pass, PERF-001 is recorded well inside its target, and the phase demo
passed**, with one demo step not automated (Phase demo).

## Scope
The TEST-PLAN §17 Gate 1 row, 36 IDs, all P0: CORE-U-001/002/005, CORE-U-017/018, CORE-U-026; LLM-C-001/002,
LLM-E-006, LLM-I-014, LLM-U-010, LLM-C-019; TOOL-U-001/003/006, TOOL-S-002, TOOL-U-011; RT-I-001/002;
SBX-C-001/002/005; GW-I-001/004/005/006, GW-I-008 (basic relay), GW-I-041/042; WEB-U-001, WEB-U-013, WEB-U-014/015,
WEB-E2E-004 and WEB-E2E-018 with the fake model server (the web app also works over plain HTTP on a LAN address);
PERF-001 baseline. Plus the phase demo (DEV-PLAN Phase 1) and milestone MS1.

## How it was checked
- **The references:** `tools/gate-evidence.py 1`, the offline listing (no test run), on dev `3940a08d`, which names
  each ID's tests: 281 references for the 36 IDs.
- **The results:** each reference's result line in 15b's integration logs (`int-23112f6b-*.log`, the coverage logs
  left out): a nextest `PASS` line for a Rust test; the test file's line in the vitest step, where 2,462 of 2,462
  tests passed; a Playwright ✓ line for a browser spec, in the fake-front suite (546 passed, 0 failed) or a
  real-stack suite.
- **Not done:** a clause-by-clause reading of each test against its row's expected result. A test named for an ID
  counts as that ID's evidence.
- Nothing was built or run for this page.

## Result: 36 of 36 pass
278 of the 281 references have a passing result line in 15b's logs, and none has a failing one. The other three:

| ID | Reference without a line in 15b's logs | Why | Evidence instead |
|---|---|---|---|
| LLM-I-014 | `llm_i_014_live_tool_round_trip_with_usage` | The live variant against a real local server: opt-in and ignored by default, as the row says | Its other test, `llm_i_014_tool_round_trip_with_final_usage` (the fake server), passed |
| WEB-U-014, WEB-U-015 | The real-stack spec "WEB-U-014/015 on the real stack: a long answer streams, its code copies, and Jump to latest brings it back" (`web/e2e/task-page-stack.spec.ts`) | 15b's task-page suite never started: its fixture lacked a field the fake model requires (a test fault, fixed in `cdb2f3ff`) | **Passed on 15c's run**, the same server images (`c15c-run/demo-task-page.log`: ✓, 12.2 s; the suite 9 passed). Their other 9 and 6 references (unit and fake-front) passed on 15b |

## Phase demo
DEV-PLAN Phase 1: "sign in, start a conversation, ask the agent to create a small project and then change it, watch
the tool cards, see the files in the workspace directory on the host. The web pages reach the browser through the
gateway."
- **WEB-E2E-004 on 15b's real stack** (`int-23112f6b-demo-basic.log`): "✓ WEB-E2E-004: create a project, inspect
  tools and replay the real transcript (6.7s)"; the driver then checked the host: "Host workspace:
  hello/README.md exists with the expected contents". The transcript after a reload matched the one before it.
- **WEB-E2E-018 on 15b's real stack** (`int-23112f6b-demo-ux.log`), opened over plain HTTP from the host's
  private LAN address (the log's "Gateway URL" line): 6 of 6, at 1024, 1440 and 1920 px, light and dark. (It had failed at 1440 light in
  13g's integration, found by the TEST-PLAN audit of 1 Oct; it passes on 15b.)
- **Not automated:** "and then change it". The scripted demo answers one request with a file write and a shell
  command (`crates/tqa-testkit/scenarios/demo.json`); a second request that changes the project is not part of it.

## Measurements (environment: one Linux workstation, Intel Core Ultra 7 270K Plus, 24 logical cores, 62 GiB, Linux 7.0.0-34, rustc 1.97.1, release build, clean tree)
| ID | Result | Target |
|---|---|---|
| PERF-001 harness overhead per step, 10-step scripted run, 50 samples after 5 warm-ups | in-memory journal: p50 **0.089 ms**, p99 0.092 ms; journal on disk: p50 **0.81 ms**, p99 1.19 ms | p50 ≤ 50 ms, p99 ≤ 200 ms (PRD M2) |

Source: `int-23112f6b-perf-release.log` (10 Oct 2026 17:48, the load 6.14 before and after) and its result file
`PERF-001-2026-10-10T09-48-47Z.json`. This is the record on the release; an earlier Phase 1 baseline was not found.

## Limits of the acceptance
- **Retrospective.** The evidence is the v1.0 release's (10 Oct 2026), not Phase 1's own.
- **By test name, not clause by clause** (How it was checked).
- **The references are dev's (`3940a08d`), the results 15b's (`23112f6b`).** All but the three above have a result
  line in 15b's logs.
- **The demo's change step** is not automated (Phase demo).
- **Single environment** for the measurement.

## Evaluation (technical lead, 12 Oct 2026)
**Verdict: Gate 1 is reached.**

- **Rechecked:** the WEB-U-014/015 real-stack spec passed on 15c's run (`c15c-run/demo-task-page.log`: ✓, 12.2 s,
  the suite 9 passed). PERF-001's result file names commit `23112f6b`, a release build, a clean tree and no failures,
  at p50 0.81 ms with the journal on disk against the 50 ms target.
- **Accepted:** LLM-I-014's live variant is opt-in by its row; its fake-server test passed.
- **The demo's "then change it" step** isn't automated: the scripted scenario answers one request. It is a gap in the
  demo's script, not a criterion (every §17 ID passes). The scenario gets a second, changing request in the next
  candidate's work.
