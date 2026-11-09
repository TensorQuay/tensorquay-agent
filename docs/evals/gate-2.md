# Gate 2: Robustness

**State:** **reached, on one condition**: SOAK-002 closes when c16's soak completes every end-of-run check (the technical lead's evaluation below, 12 Oct 2026). Until then the page was a draft. Phase 2 was built and imported on 23 and 24 Sep 2026,
before this folder had pages; its short soak (SOAK-002) ran on 24 Sep (`3781de25`), and no Gate 2 report was
written then. This page assesses the gate on the v1.0 release's evidence: 15b's full integration (10 Oct 2026,
14:56 to 17:51), 15c's web run (15b's server with the web image `28c0f621…`; 15c is on main as
`1586fd9f`) and 15b's 24-hour soak. On that evidence **49 of the 50 live IDs pass on their tests; SOAK-002 passes
with the gap the product owner accepted for 15b's soak**, and each part of the phase demo passed in its own test.

## Scope
The TEST-PLAN §17 Gate 2 row, 51 IDs: CORE-E-007…012, CORE-E-029/030, CORE-E-037, CORE-U-019…021, CORE-U-036;
CTX-U-007, CTX-E-011; LLM-E-007/008, LLM-E-018; RT-I-003…005, RT-I-009, RT-I-011, RT-E-006/010; PROTO-U-004…006;
GW-I-007, GW-I-010/011, GW-I-016…018, GW-E-020, GW-I-034…036; SBX-C-008/013, SBX-D-003; WEB-U-002; E2E-RESUME-005
on docker; SBX-C-017, RT-I-013, GW-I-043, WEB-U-018, E2E-ENV-010 (environments); GW-I-044, RT-I-014 (conversation
bar); SOAK-002 (short run). **GW-I-036 is void** (29 Sep 2026: parking for an approval went with approvals,
ADR-0035), so 50 IDs are live: 28 P0 and 22 P1. Plus the phase demo (DEV-PLAN Phase 2).

## How it was checked
- **The references:** `tools/gate-evidence.py 2`, the offline listing (no test run), on the tree of 12 Oct 2026, which names
  each ID's tests: 213 references for the 49 IDs that have tests. SOAK-002 is a harness run, not a named test.
- **The results:** each reference's result line in 15b's integration logs (`int-23112f6b-*.log`, the coverage logs
  left out): a nextest `PASS` line for a Rust test; the test file's line in the vitest step, where 2,462 of 2,462
  tests passed; a Playwright ✓ line for a browser spec.
- **Not done:** a clause-by-clause reading of each test against its row's expected result. A test named for an ID
  counts as that ID's evidence.
- Nothing was built or run for this page.

## Result
- **49 IDs, 213 of 213 references pass** on 15b, none failing. Among them the container-backed ones: E2E-RESUME-005,
  five tasks each killed mid-model and mid-tool and resumed (`int-23112f6b-recovery.log`); GW-E-020, a real
  PostgreSQL outage replayed from the runtime's log, and GW-I-018, `SIGTERM` with a live stream and runtime link
  (`int-23112f6b-deploy.log`); SBX-D-003 and SBX-C-013; E2E-ENV-010 on the real stack
  (`int-23112f6b-demo-environment.log`: "✓ E2E-ENV-010 retained file, pytest and headless browser after a switch").
- **SOAK-002 (P0), reconnect chaos: passes with the accepted gap.**

| Run | What it shows | Evidence |
|---|---|---|
| 24 Sep 2026, 2 h, the Gate 2 shape (`3781de25`) | The commit records it passed: "108 runs completed, all eight receipt logs equal stored history" | **No result file was kept** |
| 15b's 24 h soak, from 9 Oct 12:48 (SOAK-001…004) | Passed by the Product Owner's rule (PRD §12 item 25), **with the gap the Product Owner accepted**: at hour 24 the harness's sign-in had expired (HTTP 401), so "the end-of-run receipt and run checks of the last 8 of its 72 conversations didn't run" (`gate-9.md`) | `gate-9.md`, M5 row; the soak's `result.json` says `failed` for that reason |
| 11 Oct 2026, 2 h, the sign-in renewal rehearsal (15c's images) | Every end-of-run check ran and passed. **Not gate evidence** by design: it shortens the session to 60 s to exercise the renewal | the rehearsal's result (`gate_evidence` false) |

## Phase demo
DEV-PLAN Phase 2: "kill the sandbox mid-run, close the browser mid-run, restart the gateway; the run finishes and the
transcript is complete." No single test does all three in one run; each part passed on its own:
- **The sandbox killed mid-run:** E2E-RESUME-005 (above), and on the real stack WEB-E2E-042, the task's own container
  killed twice mid-run, on 15c (`c15c-run/demo-task-page.log`: "✓ WEB-E2E-042 the environment recovers after a
  failure, twice mid-run, and the run answers once").
- **The browser gone mid-run:** GW-I-007 (the client stream dropped at 20 points, each reconnect by `Last-Event-ID`,
  exactly once) and WEB-U-002 (the stream closed 5 times), both passing on 15b; and the soak's client stream drops
  (SOAK-002, above).
- **The gateway restarted mid-run:** GW-I-018 (above), and on the real stack WEB-E2E-055 on 15c ("✓ WEB-E2E-055 a
  gateway restart mid-step reconnects, starts the step over and says so").

## Limits of the acceptance
- **Retrospective.** The evidence is the v1.0 release's (10 Oct 2026), not Phase 2's own.
- **SOAK-002's exactly-once check has no complete gate-evidence run on disk:** the 24 Sep run kept nothing, and
  15b's soak missed its last 8 end-of-run checks (accepted by the Product Owner for 15b). **c16's soak must complete
  every end-of-run check**; that closes it.
- **The demo is shown in parts,** not in one run.
- **By test name, not clause by clause** (How it was checked).
- **The references are dev's (`3940a08d`), the results 15b's (`23112f6b`).** Every reference has a result line.

## Evaluation (technical lead, 12 Oct 2026)
**Verdict: Gate 2 is reached, on one condition:** c16's soak must complete every end-of-run check. That closes
SOAK-002's exactly-once check with a full gate-evidence run.

- **Rechecked:** E2E-ENV-010 (`int-23112f6b-demo-environment.log`), WEB-E2E-042 and WEB-E2E-055 (15c's
  `demo-task-page` suites) each have their ✓ line.
- **One row read clause by clause, GW-I-007:** `crates/tqa-gateway/tests/runtime_stream.rs` sends 1000 frames with
  20 seeded disconnects and checks each sequence once and in order; with `io_timeout` 2 s a 4 s store stall closes no
  link and a 9 s stall causes exactly one outage, after which every frame arrives once. All three passed in 15b's
  ignored step (`int-23112f6b-ignored.log`). The client's `Last-Event-ID` half is the SSE tests and WEB-U-002.
- **The demo in parts** is accepted: each of its three disruptions has a real-stack test on 15c's images.
