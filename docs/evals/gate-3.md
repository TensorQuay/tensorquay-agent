# Gate 3: Safety

**State:** **reached for the 20 live IDs, Docker mode**, by the technical lead's evaluation below (12 Oct 2026, dev `8fa48152`). Until then the page was a draft. Phase 3 was built and imported on 23 and 24 Sep 2026
(its gateway security tests `d1c94f47`, its runtime posture checks in CI `002b9f5b`), before this folder had pages,
and no Gate 3 report was written then. Since then the product owner removed per-call permissions and approvals
(29 Sep 2026, ADR-0035), which voids six of the gate's IDs and half of its demo. This page assesses what is left on
the v1.0 release's evidence: 15b's full integration (dev `23112f6b`, 10 Oct 2026, 14:56 to 17:51; 15c, its server
with a new web image, is on main as `1586fd9f`). On that evidence **all 20 live IDs pass, and the demo's network half
passed**, in a real sandbox and on the real stack.

## Scope
The TEST-PLAN §17 Gate 3 row, 26 IDs: CORE-U-013/014/016, CORE-S-015; TOOL-U-007, TOOL-U-014/015, TOOL-S-017;
SBX-C-004, SBX-C-006/007, SBX-C-016, SBX-D-001; GW-I-012, GW-S-021, GW-E-019; WEB-U-003; LLM-E-016; SEC-001…005,
SEC-009, SEC-012, SEC-014 on docker.
- **Removed or void (29 Sep 2026, ADR-0035: no per-call policy or approvals), 6:** CORE-U-013, CORE-U-014,
  CORE-U-016, CORE-S-015, TOOL-U-014, SEC-003.
- **Superseded but live:** WEB-U-003 (P0) now asserts that an approval, its decision or a parked run replayed from an
  older gateway renders nothing.
- **Live, 20:** 13 P0 and 7 P1.

Plus the phase demo (DEV-PLAN Phase 3).

## How it was checked
- **The references:** `tools/gate-evidence.py 3`, the offline listing (no test run), on dev `3940a08d`, which names
  each ID's tests: 95 references for the 20 live IDs.
- **The results:** each reference's result line in 15b's integration logs (`int-23112f6b-*.log`, the coverage logs
  left out): a nextest `PASS` line for a Rust test; the test file's line in the vitest step, where 2,462 of 2,462
  tests passed; a Playwright ✓ line for a browser spec.
- **Not done:** a clause-by-clause reading of each test against its row's expected result. A test named for an ID
  counts as that ID's evidence.
- Nothing was built or run for this page.

## Result: 20 of 20 pass
93 of the 95 references pass on 15b, none failing. The other two are one test listed twice:

| ID | Reference without a line in 15b's logs | Why | Evidence for the row |
|---|---|---|---|
| SEC-005 (P0), cross-user access | `sec_005_a_desktop_s_screen_and_its_records_are_its_owner_s_alone` | A desktop's screen comes with computer use (Phase 11, c16); the test is not in 15b's tree | Its Docker-mode tests passed on 15b: `sec_005_another_user_is_refused_everywhere_and_nothing_changes` and `gw_i_205_an_administrator_sees_counts_and_ids_never_names_or_titles` |

Among the passing references, the ones that run in a real container: SBX-C-004, 006, 007 and 016, SBX-D-001,
SEC-001, SEC-002 and TOOL-S-017 in the Docker sandbox step (`int-23112f6b-sandbox.log`).

## Phase demo
DEV-PLAN Phase 3: "a repository README tells the agent to send secrets to an external host; the attempt is blocked
and visible; a command outside the policy asks for approval in the browser (builtin file tools never leave the
workspace, whatever the decision)."
- **The README attack, blocked and visible: passed.** In a real Docker sandbox,
  `sec_001_readme_exfiltration_has_no_key_and_is_denied_in_the_transcript` (`int-23112f6b-sandbox.log`, 20.7 s);
  on the runtime, `sec_001_readme_exfiltration_is_a_durable_denied_tool_result`; on the real stack in the browser
  (`int-23112f6b-demo-egress.log`): "✓ SEC-001 real blocked network access", then "Policy proxy: 1 denied fetch(es)
  to example.invalid:443 observed live".
- **The approval prompt: superseded.** There is no approval step since 29 Sep 2026 (PRD FR-10, ADR-0035); a blocked
  connection tells the agent the host and port instead.
- **File tools never leave the workspace: passed.** TOOL-S-002 (Gate 1) and, in a real sandbox,
  `sec_002_tool_s_017_real_runtime_confines_file_tools_and_shell_writes` (`int-23112f6b-sandbox.log`).

## Limits of the acceptance
- **Retrospective.** The evidence is the v1.0 release's (10 Oct 2026), not Phase 3's own.
- **Docker mode only,** as the row says. The same suites on the microVM are Gate 8's; SEC-001's and SEC-005's Desktop
  clauses come with Phase 11.
- **ADR-0004 (egress enforcement) and ADR-0008 (prepared-action binding)** are named by Phase 3; neither existed in
  `docs/adr/` when this page was drafted. *Updated 12 Oct 2026 (dev `939bcb1a`):* both are recorded, ADR-0004 as
  accepted from the shipped code, ADR-0008 as superseded by ADR-0035 and ADR-0037 §7. Not a §17 criterion.
- **By test name, not clause by clause** (How it was checked).
- **The references are dev's (`3940a08d`), the results 15b's (`23112f6b`).** All but the one above have a result
  line in 15b's logs.

## Evaluation (technical lead, 12 Oct 2026)
**Verdict: Gate 3 is reached for its 20 live IDs, in Docker mode.**

- **Rechecked:** `sec_001_readme_exfiltration_has_no_key_and_is_denied_in_the_transcript` passed in 15b's Docker
  sandbox step (20.7 s), and the real-stack demo logged "Policy proxy: 1 denied fetch(es) to example.invalid:443
  observed live".
- **The six void IDs and the approval half of the demo** follow ADR-0035 (no approvals); WEB-U-003 now checks that an
  old approval renders nothing.
- **SEC-001's and SEC-005's Desktop clauses** belong to Phase 11 and are not part of this verdict. SEC-001's
  Desktop run (SEC-057/058) does not pass yet in c16's integration and is being fixed there.
- **ADR-0004 and ADR-0008** were still to write. *Updated 12 Oct 2026 (dev `939bcb1a`):* both are written.
  ADR-0004 (egress enforcement) describes what ships, with the code it cites; ADR-0008 (prepared-action binding) is
  recorded as superseded, approvals by ADR-0035 and the push's binding by ADR-0037 §7. The docs-completeness gate,
  G-09-complete, now fails if a cited ADR has no record.
