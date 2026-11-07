# Gate 4: Team

**State:** the gate is **reached**, by the technical lead's evaluation below (2026-09-28). The page was
assembled from the gate audit of 2026-09-26 (with rechecks on `fe8cb2e`) and the follow-up tests merged
since.

## Scope
Every ID in the TEST-PLAN §17 Gate 4 row, 28 IDs: GW-I-002/003, GW-I-013…015, GW-I-023, GW-I-028, GW-I-030/031,
GW-U-022, GW-U-027; SBX-C-002, SBX-C-009…011, SBX-C-015, SBX-D-004; WEB-U-007/008, WEB-E-009, WEB-E2E-005/006/011;
E2E-TEAM-011; TK-U-001; PERF-010, PERF-012, PERF-020. Plus the phase demo (E2E-TEAM-011).

## What was independently verified
- **Re-run by the auditor** (no containers): the gateway database tests against a private test database, the web
  unit, API and protocol checks, and the test-client tests. Each ID was checked clause by clause against its expected
  result, not only for the existence of a test.
- **Taken from recorded runs** (container-backed): the sandbox suites, the stack web tests (WEB-E2E-005/006/011), the
  team demo (E2E-TEAM-011, 3/3) and the performance runs.

## Result at the audit (2026-09-26 17:20)
- **20 pass, 6 partial, 0 missing.** The partial rows had tests that did not assert every clause:
  - GW-I-013: tree page bounds, preview truncation, download size limit;
  - SBX-C-002: ownership and permissions after re-creation;
  - SBX-C-010: no leftover volume or process;
  - SBX-C-011: unique addresses and no cross-talk;
  - SBX-C-015: the typed `workspace.full` event;
  - SBX-D-004: the logged warning when a tag moves.
- **Every P0 passed** except SBX-C-002, which was partial.

## Since the audit
Tests for each partial clause are now on dev. They have **not** been re-run for this page.
- GW-I-013: `gw_i_013_a_preview_is_cut_at_preview_bytes_and_marked_truncated`,
  `gw_i_013_a_download_above_download_bytes_is_413`, and the tree `limit` paging.
- SBX-C-002: modes and ownership asserted in `sbx_c_002_sbx_c_015_workspace_persists_for_bind_and_volume`.
- SBX-C-010: `…_removes_partial_container_network_volume_and_process`,
  `…_unknown_digest_leaves_no_container_network_volume_or_process`.
- SBX-C-011: `…_sixteen_concurrent_creates_have_unique_handles_and_addresses`, `…_one_sandbox_cannot_reach_another`.
- SBX-C-015: the typed event landed (`workspace.full`, TOOL-U-026, CORE-U-049, PROTO-U-015, WEB-U-041). The row was
  restated: the in-run out-of-space clause applies on Firecracker; on Docker the quota is enforced between runs
  (GW-I-030).
- SBX-D-004: `sbx_d_004_a_moved_tag_does_not_change_the_pinned_image_and_logs_a_warning`.

## Measurements (environment: one Linux workstation, rootless container engine)
| ID | Result | Target |
|---|---|---|
| PERF-010 Docker sandbox start | p50 413 ms, p99 694 ms | p50 ≤ 3 s |
| PERF-012 Workspace I/O | ratios 1.14–1.25 (partly interpreter overhead) | ≥ 0.80 |
| PERF-020 Concurrency | 16 conversations, 0 dropped events, CPU p95 24.2 %, RSS 12 MB | CPU < 70 %, RSS < 1.5 GB |
| WEB-E-009 Long transcript | TTI p50 789 ms over 50 cold loads; an 8× CPU-throttled control fails as it should | TTI ≤ 2 s |

## Limits of the acceptance
- The container-backed IDs passed on images older than the final ones; a fresh run on the final images is needed
  before sign-off.
- The follow-up tests listed above need one green run on dev before the partial rows count as passing.

## Evaluation (technical lead, 2026-09-28)
**Verdict: Gate 4 is reached.** Both limits above are closed.

- **The follow-up tests for the six partial rows passed** in the lead's full integration run on `deef3f2` (2026-09-28
  00:21–01:07), which was green in every step: `gw_i_013_a_preview_is_cut_at_preview_bytes_and_marked_truncated`,
  `gw_i_013_a_download_above_download_bytes_is_413`, `sbx_c_002_sbx_c_015_workspace_persists_for_bind_and_volume`, the
  two SBX-C-010 no-leftover tests, the two SBX-C-011 tests and `sbx_d_004_…_logs_a_warning`. The same tests passed again
  in the run on `a0bf5ef`. So the 6 partial rows count as passing: the audit's 26 assessed IDs are **26 pass, 0 partial,
  0 missing**, and the 2 measured IDs meet their targets (Measurements above).
- **The container-backed IDs were re-run on the final images** (built from `a0bf5ef`: gateway `sha256:66494e4f0f32…`,
  Workstation runtime `sha256:2231f26c9fd4…`), from an exact `a0bf5ef` tree:
  - the team demo E2E-TEAM-011, 3 of 3 (03:40–03:41): two users, a share, a lease wait, a fork run in parallel and two
    pushes; evidence in `tests/e2e/results/gate4-20260927T1940*Z`;
  - WEB-E2E-005 (push, 03:42), WEB-E2E-006 (administration, 03:43) and WEB-E2E-011 (workspaces, 03:44), each passing;
  - the sandbox suites (SBX-C-002, SBX-C-009…011, SBX-C-015, SBX-D-004) in both integration runs above.
- **Known limits, accepted:** SBX-C-015's in-run out-of-space clause applies on Firecracker; on Docker the quota is
  enforced between runs (GW-I-030), as restated in the row. The measurements are single-environment.
