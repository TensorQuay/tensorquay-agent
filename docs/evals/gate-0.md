# Gate 0: Foundation

**State:** **technical criteria met; open on the Product Owner's MS0**, by the technical lead's evaluation below (12 Oct 2026). Until then the page was a draft. Phase 0 was built before this folder had pages: the
repository's first import (`a50bac90`, 23 Sep 2026) already held the workspace, the guardrails and the protocol
types, and no Gate 0 report was written then. This page assesses the gate on the v1.0 release's evidence, 15b's full
integration (10 Oct 2026, 14:56 to 17:51; 15c, its server with a new web image, is on main as
`1586fd9f`). On that evidence the four test IDs and the CI gates pass. **One criterion has no record yet:** the product
owner's approval of the four documents (MS0). It was given on 12 Nov 2026 on a condition, that the technical lead had
reviewed the documents and found them aligned; the review and the revisions it led to are below ("The documents,
12 Nov 2026"), and the record follows them.

## Scope
The TEST-PLAN §17 Gate 0 row: PROTO-C-001/002, PROTO-U-003, PROTO-U-007 (3 P0, 1 P1), and the CI gates G-01…G-10.
DEV-PLAN Phase 0 adds two criteria: the skeleton protocol subset published, and the PO's approval of the documents
(milestone MS0).

## How it was checked
- **The references:** `tools/gate-evidence.py 0`, the offline listing (no test run), on the tree of 12 Oct 2026, which names
  each ID's tests.
- **The results:** each reference's result line in 15b's integration logs (`int-23112f6b-*.log`, the coverage logs
  left out): a nextest `PASS` line for each Rust test.
- **Not done:** a clause-by-clause reading of each test against its row's expected result. A test named for an ID
  counts as that ID's evidence.
- Nothing was built or run for this page.

## Result
| Criterion | Result | Evidence |
|---|---|---|
| PROTO-C-001 (P0) | **pass**: 35 of 35 tests | `int-23112f6b-unit.log` and the other step logs, `PASS` lines |
| PROTO-C-002 (P1), generated types in sync | **pass** | `int-23112f6b-webcheck.log` and the summary: `PASS protocol:check`; `int-23112f6b-check.log`: `PASS G-08`. The listing finds no test named for it: the row's check is gate G-08 and the web's `protocol:check` |
| PROTO-U-003 (P0) | **pass**: 24 of 24 | as PROTO-C-001 |
| PROTO-U-007 (P0) | **pass**: 15 of 15 | as PROTO-C-001 |
| CI gates G-01…G-10 | **pass** on 15b's tree: G-01, 02, 03, 04, 05 (with G-05-images), 06, 07, 08 (with G-08-decision), 09 (with G-09-catalog), G-10-audit, unit and secrets, "All gates passed" | `int-23112f6b-check.log` |
| Skeleton protocol subset published | **met**: `crates/tqa-types/schema/tqa-protocol-v1.json`, exported from the types and held to them by G-08 (pass, above); described in ARCHITECTURE §9 | the schema file; G-08 |
| PO approval of the documents (MS0) | **no record found** (12 Oct 2026; since then, see "The documents, 12 Nov 2026") | PRD's status line: "v0.2 (1 Oct 2026): v0.1 (22 Sep 2026) revised with the Product Owner's decisions to date, listed in §12". DEV-PLAN: v0.5 (8 Oct 2026). **ARCHITECTURE and TEST-PLAN** still say "draft v0.1 for Product Owner review (22 Sep 2026)". No approval of the set is recorded in the documents or the team's log |

## Phase 0's other deliverables (not gate criteria; checked for completeness)
- **Present:** the Cargo workspace and `web/`; `rust-toolchain.toml`, `deny.toml`; `AGENTS.md`, the hooks and
  `tools/git-guard.py`; `tqa-testkit` with `FakeProvider`, `FakeModelServer` (the fake OpenAI-compatible model
  server), `FakeGateway`, `FakeClock`, the protocol test client (`client`) and the workspace fixtures; ADR-0001, 0002,
  0003 and 0005; `THIRD-PARTY-NOTICES.md`; the admin guide (`docs/admin-guide/`).
- **The plan's fake names (updated 12 Oct 2026):** the plan said `FakeSandbox` and `FakeRelay`; the
  same fakes ship as `tqa-sandbox`'s `FakeSandboxBackend` and the testkit's `FakeGateway` (the runtime link and a
  streaming relay), and DEV-PLAN and DEV-GUIDELINES now use those names.
- **The protocol reference (updated 12 Oct 2026):** `docs/protocol-reference.md`, from the schema and
  ARCHITECTURE §9; G-09-complete fails if it misses a frame, event, error code or type of the schema.

## Limits of the acceptance
- **Retrospective.** The evidence is the v1.0 release's (10 Oct 2026). Nothing on disk shows the gates green on an
  empty tree; the first import was not empty.
- **By test name, not clause by clause** (How it was checked).
- **The references are dev's (`3940a08d`), the results 15b's (`23112f6b`).** Every reference for these four IDs has
  a result line in 15b's logs.

## For the product owner
- **MS0:** confirm that PRD, ARCHITECTURE, DEV-PLAN and TEST-PLAN are approved, so their status lines can say so; or
  say what is still to review.

## Evaluation (technical lead, 12 Oct 2026)
**Verdict: Gate 0's technical criteria are met; the gate closes when the product owner records MS0.**

- **Rechecked, not taken from the draft:** PROTO-C-001's 35 references each have a `PASS` line in 15b's logs, and no
  PROTO test has a `FAIL` line in any of them; `tools/gate-evidence.py 0` on `8fa48152` lists the same references.
  The listing marks PROTO-C-002 "no test": its check is gate G-08 and the web's `protocol:check`, both passing on
  15b, so it counts as passing.
- **MS0 is the product owner's.** Two of the four documents still say "draft v0.1 for Product Owner review". That
  is a record to make, not a test to run, and it is put to the owner.
- **`FakeSandbox`, `FakeRelay` and a separate protocol reference** are deliverables the plan names, not criteria.
  *Updated 12 Oct 2026:* the docs-completeness gate, G-09-complete, found them; the fakes ship as
  `FakeSandboxBackend` and `FakeGateway` and the plans now say so, and `docs/protocol-reference.md` is written and
  held to the schema by the gate.

## The documents, 12 Nov 2026
The product owner approved the four documents on 12 Nov 2026 on one condition: that the technical lead had reviewed
them and found them aligned. The review of that day found them not aligned on 16 points, each a place where a
document had fallen behind a decision already made or a release already out. None needed a new decision.

- **How they were reviewed:** the PRD and the DEV-PLAN's §1 to §4b read in full; ARCHITECTURE read to §4.7 and
  searched from there for wording about what is later, proposed, pending, deprecated or approved; TEST-PLAN's §17
  and §18 read against the PRD's requirements and the DEV-PLAN's gates, its test rows not reread.
- **What was found:** status lines that still said "draft for Product Owner review"; the PRD against itself
  (English only beside ten languages; object storage beside none); requirements that still spoke of approvals,
  a `parked` state and a Git relay that were removed; decisions listed as open that the build had settled;
  nothing on the releases since 17.0, or on agent teams and computer use being in them; the DEV-PLAN's stack
  table (the jailer, an image name) and its standing of 7 Nov; no traceability rows for FR-34 to FR-36.
- **The revisions:** PRD v0.3, DEV-PLAN v0.8, ARCHITECTURE v0.2 and TEST-PLAN v0.2, all of 12 Nov 2026.
- **Left open, and said so:** the PRD's §12 item 5 (the reference hardware for published numbers) has no decision;
  item 25's fourth check (SOAK-001's D4) takes its confirmation from this approval; NFR-2, NFR-3 and NFR-8 have
  no row in TEST-PLAN §18.
