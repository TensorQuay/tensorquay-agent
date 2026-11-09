# Gate 9: PRD metrics M1–M8

**State:** **reached on release candidate 15b**, by the technical lead's verdict (11 Oct 2026), **with the Product
Owner's waiver of PERF-012's microVM half** (11 Oct). Every metric M1–M8 is met on 15b (the summary below; the runs
under "Release candidate 15b"): the 24 h soak passed by the Product Owner's rule of 9 Oct (PRD §12 item 25), with a gap
the Product Owner accepted; Gate 9's rerun passed; PERF-021, REL-I-001…003 and PERF-012's Docker half passed; PERF-012's
microVM half failed and was waived. The v1.0 candidate became **15c** (15b's server with a new web image) and is on main
as `1586fd9f` (11 Oct). **9 Oct 2026:
c15 was respun as 15b**:
c15's soak showed the gateway's memory growing (two leaks, both fixed in 15b), so the product owner chose a new
candidate. 15b's 24 h soak runs from 9 Oct 12:48, and its Gate 9 runs follow it; see "Release candidate 15b". Before
that, **8 Oct 2026: release candidate c15** was in its 24 h soak; its Gate 9 reruns were to
follow on a quiet host. The integration's
values and the release dry run are recorded below, under "Release candidate c15". Refreshed 6 Oct 2026 with release
candidate c14b-rc's Gate 9 runs, and 7 Oct with its GPU rows. Gate 9 was not reached then: M1 was met on c14b-rc (60/60 and 59/60, the one miss the
model's, 7 Oct); M5's soak 4 on c14b-rc (6 Oct 20:13) failed on the harness's own fixed 60 s wait, shorter than the gateway's start
bounds, not on the product (fixed in `df6bb17b` and `a2d7202b`); M6's
E2E-CMP-003 is pending a GPU slot. Refreshed 4 Oct 2026 with candidate 13h's values. Gate 9 needs
every PRD metric (PRD §7, M1–M8) **met or explicitly waived by the product owner**. v1.0 ships on the **next**
candidate, so every verdict is taken on that candidate's runs (decided on 4 Oct 2026); candidate
13h's values below are **indicative**, kept with their raw data and commands, and decide nothing. On 13h, M1–M4 measured
inside their bounds, M5 (the soak) and M6 (the comparison) have no passing run, M7 was met on candidate 7, and M8's
coverage is met. No waiver is asked; the Product Owner's open questions and the next candidate's runs are at the end.

**The rule this page applies:** a metric is *met* only with a measured result on a **named release candidate**, dated,
with its configuration. A result measured only on an ordinary dev commit, or asserted by a passing test that keeps no
value, is *not yet* until it is measured again on a candidate. A stretch target, or a target the evidence can't reach
by method, needs an *owner waiver* or an owner decision, never a quiet pass.

Candidates named here: candidate 5 `545c3d0` (2026-09-28 08:05), candidate 6 `a14afc0` (2026-09-28 15:50, images gateway
`c580e6b4b246`, runtime `47fea18da874`, Workstation `8ccb9b88a291`). Candidate 13h: `5b79a932` (integration 1 Oct 2026
18:30–23:56 +08:00; quiet reruns 2 Oct from `ae236763`, the same images).

## Summary
| Metric | Verdict (taken on the next candidate) | Candidate 13h, indicative | Earlier evidence | Open |
|---|---|---|---|---|
| M1 Task completion | **met on 15b** (E2E-WAVE-001 60/60, E2E-WAVE-002 60/60 with 8 helpers, 0 harness failures, 10 Oct; `gate-5.md`); earlier met on c14b-rc (60/60 and 59/60); the model's class is the Product Owner's question | E2E-WAVE-001 58/60 = 96.7 %, E2E-WAVE-002 59/60 = 98.3 %, 0 harness failures (2 Oct) | 59/60 both waves, `a14afc0` (28 Sep) | Owner question: does the wave model count as "GLM-5.3-Flash class"? |
| M2 Harness overhead | **met on 15b** (PERF-001 journal p50 0.80 / 0.83 ms, p99 4.70 / 1.04 ms, the rerun's two candidate rounds; bound p50 ≤ 50 ms, p99 ≤ 200 ms); earlier met on c14b-rc | PERF-001 release: journal p50 1.90 ms, p99 28.7 ms; memory p50 0.076 ms, p99 0.078 ms; at load 25 (not idle) | none | none (done on 15b's quiet-host rerun) |
| M3 Sandbox start | **met on 15b** (PERF-010 p50 452 / 448 ms; PERF-011 new task p50 501 / 510 ms, within PRD M3's 3 s); earlier met on c14b-rc; the warm-snapshot stretch is the Product Owner's question | Docker PERF-010 p50 515 ms, p99 681 ms; Firecracker cold boot 170–240 ms; the person's view on the microVM: new task p50 674 ms, p99 724 ms | Docker p50 413 ms (dev, 26 Sep) | Owner question: the warm-snapshot stretch |
| M4 Concurrency | **met on 15b** (PERF-020 passed: CPU p95 18.9 / 18.4 %, RSS 12 MB); earlier met on c14b-rc (0 dropped, CPU p95 20.1 %) | PERF-020: 16 conversations, 3 runs, 0 dropped events, CPU p95 19.2 % of 2 vCPU, RSS peak 12.2 MB | 0 dropped, CPU p95 24.2 % (dev, 26 Sep) | none (done on 15b) |
| M5 Reliability | **met on 15b**, by the Product Owner's rule of 9 Oct 2026 (PRD §12 item 25): RSS +1.2 % (projected +0.7 %), descriptors flat, the database connections within the pool, on a 12.06 h lifetime; **with the gap the Product Owner accepted** (10 Oct): the last 8 of 72 conversations' end-of-run checks didn't run (an expired sign-in at hour 24), to be completed by c16's soak. Earlier: c15's soak showed two leaks (fixed in 15b); soak 4 on c14b-rc failed on the harness's wait | No passing soak: the 24 h run was waived for 13h's release only; both 8 h soaks failed (2h25 on the outage audit's rule; 3h19 on the full-disk defect, fixed since) | rehearsals only (candidates 4–6) | c16's soak completes every end-of-run check (the gap) |
| M6 Capability parity | **met on 15b**: E2E-CMP-003 ours 50/50, reference 50/50, a difference of 0 points (bound ±5), 10 Oct (`gate-5.md`) | not run | 48/50 vs 50/50, −4.0 points, 95 % interval −10.0 to 0.0 (`a14afc0`) | none (done on 15b) |
| M7 Isolation | **met on 15b**: G-13's `egress`, `escape` and `history` passed on the microVM, and the Docker sandbox and security suites passed, in 15b's integration (10 Oct); earlier met on candidate 7; subject to the Product Owner's items | G-13's `egress`, `escape` and `history` passed on the microVM; the Docker stack suites passed | twelve steps on both backends (candidate 7) | The Product Owner's items: the SEC-023 microVM limit; the audit's G1 row |
| M8 Openness | **met on 15b**: G-04's coverage passed in 15b's integration, and each 15b number on this page names its raw data (the soak's samples and judge, the rerun's kept JSONs, the GPU runs' records in `gate-5.md`) | pure-crate coverage 95.90 % lines | 93.67 % (`a14afc0`) | none for 15b |

## Per metric

### M1: Task completion
- **PRD:** "≥ 90 % of tasks complete with a passing check when the model is GLM-5.3-Flash class; the remaining
  failures are model errors, not harness errors (classified per run)."
- **Evidence:** `waves.md`: E2E-WAVE-001 (one at a time) and E2E-WAVE-002 (eight at a time) on `a14afc0`, 2026-09-28,
  20 tasks × 3. Both 59/60 = 98.3 % (95 % Wilson 91.1–99.7 %), 0 harness failures, each one miss `model.wrong_work`,
  no provider retry; lost or duplicated events 0.
- **Verdict:** met on the numbers, on a named candidate. **One open point:** the PRD fixes the model's class. The page
  records "a 27B open-weight model (FP4)" without naming it or its class, so the owner has to confirm the class, or the
  page has to name the model and its class.
  Candidate 13h (indicative): 58/60 and 59/60, no harness failure (`waves.md`).
  **Release candidate c14b-rc (7 Oct 2026):** E2E-WAVE-001 60/60 = 100 % (95 % Wilson 94.0–100 %), E2E-WAVE-002
  59/60 = 98.3 % (91.1–99.7 %), 0 harness failures (`waves.md`). E2E-WAVE-002's one miss,
  `ts-add-tests-slug` (harness label `model.budget` then, `model.stuck` since 7 Oct), is classified by hand as the model's: its own tests asserted
  behaviour the code doesn't have, three times, and the repeated-failure guard ended the run (`waves.md`, Results).
  **Met on c14b-rc.** The model's class stays the Product Owner's question 1.

### M2: Harness overhead
- **PRD:** "≤ 50 ms p50, ≤ 200 ms p99 (PERF-001)" per model turn, excluding model time.
- **Evidence:** the measuring test (`tqa-core/tests/performance.rs`, `perf_001_harness_overhead`) is ignored by design
  (release mode, idle host) and runs only when invoked. Candidate 6's integration ran only its five helper tests. No
  recorded p50/p99 exists on any candidate; the one recorded run in the integration logs failed.
- **Verdict:** not yet. Candidate 13h measured it for the first time (journal p50 1.90 ms, p99 28.7 ms; release mode)
  but at load 25, not on an idle host, so it is indicative only. The verdict comes from PERF-001 on an idle host on the
  next candidate (`gate9_run.py`, step a).

### M3: Sandbox start
- **PRD:** "Docker on Linux: ≤ 3 s p50 warm image. Firecracker: ≤ 2 s cold boot p50; ≤ 500 ms from a warm snapshot
  (Phase 4 stretch) (PERF-010/011)."
- **Evidence:** Docker: PERF-010 p50 413 ms, p99 694 ms, in the Gate 4 audit (2026-09-26). Firecracker:
  PERF-011 cold boot p50 136 ms, max 364 ms over 31 boots (`gate-8.md`, still a draft, 2026-09-26/27). On candidate 6
  PERF-011 first failed on a rootfs build under load, then passed its ≤ 2 s assertion on the rerun (17:07), which keeps
  no value.
- **Measured risk (2026-10-02, candidate 13h's images, harness `37bc4922`, a tooling run on a loaded host, not gate
  evidence):** PERF-010 p50 559 ms, p99 2,324 ms; the returning task as a person sees it (`PERF-010-returning-task`: the
  sandbox removed after a 10 s test idle window, the next message on a new container) p50 530 ms, p99 19,906 ms. Three
  of its 50 samples took 13 to 20 s, all in Docker's own create-to-start (11 to 17 s), on a loaded host (one-minute load
  9 to 25 over the run; short image builds ran on the same engine at about that time). The p50 targets hold, but under
  engine load a person returning to a task can wait 10 to 20 s; the next measurement on a quiet host shows whether the
  tail is the host's or the product's. Raw data: the two `perf-010-*.json` results of that run (samples and splits),
  reproduced by `deploy/tests/perf_start.py`.
- **Verdict:** not yet. On candidate 13h every value is well inside its bound (Docker p50 515 ms; microVM cold boot
  170–240 ms; a person's new microVM task p50 674 ms), indicative only. The verdict comes from the next candidate's
  PERF-010/011 and the person's view of a start, which also measure the start-budget fixes made since 13h. The warm
  snapshot is not built: an owner question.

### M4: Concurrency
- **PRD:** "16 live conversations on one gateway host with 2 vCPU / 4 GB and no dropped events (PERF-020)."
- **Evidence:** PERF-020 in the Gate 4 audit (2026-09-26): the gateway capped at 2 vCPU / 4 GB by its
  cgroup (`deploy/tests/perf_concurrency.py`), 16 conversations, 0 dropped events, CPU p95 24.2 %, RSS 12 MB.
- **Verdict:** met on candidate 13h: 16 conversations, three runs of 4,800 events, 0 dropped or repeated, CPU p95 19.2 %
  of 2 vCPU, RSS peak 12.2 MB (`perf-020-152130.json`). Confirmed by the next candidate's PERF-020 (step b).

### M5: Reliability
- **PRD:** "24 h soak at 8 conversations: no crash, gateway RSS growth < 5 %, zero lost or duplicated events across
  forced reconnects, every run resumable after a gateway restart (SOAK-001…004)."
- **Evidence:** the soak driver and its disruption plan exist (`deploy/tests/soak*.py`, `deploy/README.md`). Only
  rehearsals have run (candidates 4, 5, 6). The full-disk loop they found is fixed (GW-I-114). No 24 h run, and no 2 h
  run with kept evidence on a candidate.
- **Verdict:** not yet. No soak has passed on a candidate: the 24 h run was waived for 13h's release only, and both 8 h
  soaks on 13h failed (at 2 h 25 min on the outage audit's rule; at 3 h 19 min on the full-disk defect, fixed since).
  The verdict needs the 24 h soak on the next candidate, judged as the README says (growth on the last 12 h lifetime),
  with its evidence directory kept; no waiver is asked.

### M6: Capability parity
- **PRD:** "Task completion within 5 points of OpenHands on the shared subset (E2E-CMP)."
- **Evidence:** `comparison.md`: E2E-CMP-003 on `a14afc0`, 2026-09-28, 10 tasks × 5: 48/50 vs 50/50, −4.0 points,
  95 % interval −10.0 to 0.0. Both misses were `harness.check` timeouts on work that passes; the cause (the check
  container's removal counted against its bound) is fixed in `a87b8fc` (TK-U-025).
- **Verdict:** not yet shown. Not rerun since candidate 6. The next candidate runs E2E-CMP-003 at 50 attempts a side,
  reported as measured against PRD's ±5. At these rates the 95 % interval is about ±12 points; if it can't decide
  either way, the technical lead brings the numbers to the owner.

### M7: Isolation
- **PRD:** "The security test set passes in both runtime modes: no secret readable from inside the sandbox, no egress
  outside the allow-list, no write outside the workspace (SEC-*)." The network allow-list is the only guardrail: the
  approval step is removed after candidate 7, so SEC-003 (forged approval) is no longer part of M7, and SEC-019 counts
  only its credential half (the push credential never enters the sandbox).
- **Evidence:** the security set audit of 2026-09-28 and the rows added since (SEC-005/006/007/017/041). TEST-PLAN §14
  says which rows run on both backends (the sandbox-side ones) and which run once (the gateway-side ones, SEC-024
  among them, since the gateway resolves and connects to a connector's host itself). On Docker the set is tested
  (SEC-017's stack half passed on candidate 5). On the microVM, SEC-001…006, 014 and 016 ran in the Gate 8 draft
  (contract suites), and on candidate 6 SEC-010 (`contract.py pids`: the storm stopped at the configured limit, the
  neighbour VM ran), SEC-013 (`connector_isolation --backend firecracker`) and SEC-019 (`workspace_push.py --backend
  firecracker`) passed. Their logs, on the run host: `~/.cache/g8-pids.log`, `~/.cache/g8-sec013.log` and
  `~/.cache/g8-sec019.log` (each names the gateway and runtime image IDs and prints its verdict line).
- **Candidate 7 run (29 Sep 2026, 13:14–15:05, the script's twelve steps one stack at a time; all twelve pass):** candidate 7's
  images: gateway `sha256:f7b9278c2f82…`, runtime `sha256:26470a3e2e60…`, workstation `sha256:cb399c4ac472…`, vmm
  `sha256:357c7555bdac…`, fake model `sha256:6a862c6f25b1…`. The harness is candidate 7's own tree (`ff897c0`) for steps
  1–7 and that tree plus one harness fix (SEC-002's host-side check handed the rootless installation back before reading
  it; on the candidate's release branch) for steps 8–11. Logs on the run host: steps 1–7 in `~/.cache/m7-20260929-1314/`,
  steps 8–11 in `~/.cache/m7-20260929-1321/`, one file per step.

  | Step | Rows | Backend | Result (the case's own verdict line, shortened) |
  |---|---|---|---|
  | 1 `egress` | SEC-001, 006, 016, 018 | microVM | Pass: no secret in the guest or transcript; every bypass failed (gateway's other port, the database through the proxy, a denied host, loopback, metadata, a public IP, UDP DNS) |
  | 2 `escape` | SEC-002, SEC-014 (sandbox side) | microVM | Pass: 20 file-tool escapes refused with typed codes; root and token writes refused; outside canaries and git metadata unchanged |
  | 3 `pids` | SEC-010 | microVM | Pass: a storm of 356 against a limit of 256 held at 256 (108 refused); the neighbour VM ran; the stormed VM destroyed when idle |
  | 4 `history` | SEC-017 (stack half) | microVM | Pass, the first time on a candidate: the second conversation held only its own state device; nothing named the first; a forged history left the first unchanged |
  | 5 `sec013fc` | SEC-013 | microVM | Pass: the tool called through the proxy with the gateway's credential; elsewhere refused with 403; the credential absent from the guest and the captured streams |
  | 6 `sec019fc` | SEC-019 (credential half) | microVM | Pass: after a real push, the credential absent from the guest, the events, host storage (verified by a planted copy) and the logs |
  | 7 `sec001` | SEC-001 | Docker | Pass: README-driven shell and fetch exfiltration denied; no provider or database key |
  | 8 `sec002` | SEC-002, TOOL-S-017 | Docker | Pass: 20 file-tool escapes refused; outside canaries and git metadata unchanged |
  | 9 `sec013` | SEC-013, TOOL-I-018 | Docker | Pass: as on the microVM, the credential also absent from 6 reassembled streams of captured traffic |
  | 10 `sec019` | SEC-019 (credential half, `--credential-only`) | Docker | Pass: the credential absent from the sandbox, the events, workspace and state storage including the journal, and the logs |
  | 11 `sec017` | SEC-017 (stack half) | Docker | Pass: 4 mounts with only its own state; only its own conversation id seen; the first conversation's history unchanged by the second |
  | 12 `docker` | SEC-006, 016, 018; SEC-010; SEC-014; SEC-023 (nextest) | Docker | Pass (15:02–15:05, a quiet host): SEC-016 and SEC-018 (`docker_network`); SEC-010's storm and report (`docker_pids`); SBX-C-006, SBX-C-007 and SBX-C-016 with SEC-014 (`runtime_posture`); SEC-023 (`skills_isolation`). SEC-001 is step 7 (the `egress` binary wraps the same drivers as steps 7 and 8). Logs `~/.cache/m7-20260929-1502/docker.log` and `docker-rest.log`; a first attempt at 13:22 stopped on host load, not a finding |
- **Gateway-side rows on candidate 7** (they run once, TEST-PLAN §14), from the candidate's integration run (29 Sep, 11:29–11:30):
  - the database suites (`--run-ignored all`): 2047 passed, 0 failed; log `~/.cache/int-ff897c00-ignored.log`. Named cases
    for SEC-001, 003–005, 007, 009, 011, 012, 014, 017, 019–022, 024, 026–028, 031–036, 041, 042 and 044, and
    SEC-015's channel cases (9 passed);
  - the unit suites: 2018 of 2019 passed; log `~/.cache/int-ff897c00-unit.log`. The one failure is a runtime reaper test
    (RT-I-001) that passed on its retry in the same run and in the runtime suite, not a security row;
  - the repository gates: SEC-030 as `G-05-images` passed; log `~/.cache/int-ff897c00-check.log`;
  - SEC-008: the dependency gates (G-05) there, and the release vulnerability gate in the release dry run
    (`~/.cache/release-dist/0.9.0-rc.1/sbom/vulns.md`, candidate 6; on candidate 7 when it is released);
  - SEC-037–039 test agent teams (v1.1, ADR-0034), outside v1.0's M7.
- **Verdict:** met on candidate 7, subject to the Product Owner accepting the SEC-023 microVM limit (below; the planned
  skills work would remove it).
  - Done: the sandbox-side rows on both backends on one candidate (the twelve steps above), the gateway-side rows from
    the candidate's run with the test database, SEC-036 among them (the decision engine ships in v1.0, ADR-0013), and
    SEC-019's credential half; SEC-003 left with the approval step.
  - The one other open item: the security audit's G1 row (connector results marked as untrusted content).
- **Limit:** SEC-023 (skills trees read-only and private) has no microVM half: skills mounts are unsupported on
  microVMs (refused, "not available on the microVM backend yet"). M7 on the microVM holds without it; this is for the
  owner to accept.

### M8: Openness
- **PRD:** "Every published number has its configuration, raw data and a reproduction script. Coverage ≥ 90 % lines on
  the pure crates (G-04)."
- **Evidence:** coverage on candidate 6: the pure crates 93.67 % lines (G-04-ok, 2026-09-28 16:31). Publication:
  `comparison.md` names its runner and attempt records; `gate-8.md` names its contract commands; `waves.md` names its
  configuration and harness but no raw-data location; Gate 4's PERF-010/012/020 values name neither raw data nor a
  command.
- **Verdict:** not yet: coverage met (13h: 95.90 % lines). Publication: this page names the raw data and command of
  each 13h number, and `waves.md` its 13h records; the next candidate's numbers come from `gate9_run.py`, whose index
  names each result and its SHA-256.

## Release candidate 15b (9 to 11 Oct 2026)
**15b**: c15 (`b5caac76`) and its soak-harness fixes, plus the two heap fixes below. Tag file
`candidate15b-tags.txt`, built 9 Oct 12:41 at runtime protocol revision 13:
- gateway `040068250224`, web `9caeb9309fc8`, runtime `62d8588edbfa`, Workstation `1648c896cdc4`, fake model
  `f54d5c74cacc`, VM host `466fccc57357`.
- **Every image was rebuilt, so every digest is new.** Only two have new code since c15:
  - the gateway: the two heap fixes;
  - the web: Next.js 16.3.6 to 16.3.8, c15's web fix.
- **The runtime, Workstation, fake model and VM host images are rebuilt from unchanged sources:** no file under
  `images/`, the runtime's crates, the guest init or `deploy/firecracker/` changed between `b5caac76` and `23112f6b`.
  Their new digests come from the rebuild, not from a change. 15b's rootfs is therefore built again from its own
  runtime image.

**How M5 is judged on 15b: the Product Owner's rule of 9 Oct 2026 (PRD §12 item 25; TEST-PLAN SOAK-001).**
- **Gateway RSS** keeps M5's 5 % rule: the second hour's median against the last hour's, and the slope projected to
  24 h.
- **Open descriptors other than the gateway's database connections** take the same 5 % rule.
- **The gateway's database connections** are judged against the pool:
  - never above its maximum plus the gateway's listeners (16 + 0 in the soak);
  - at the maximum in under 90 % of the last hour;
  - on a lifetime of 11 h or more, their floor (10th percentile) up by at most 2 from the first three settled hours to
    the last three.
- **The probe's own database session** is no longer counted as the gateway's.
- **The 24 h run fails if no lifetime of 11 h or more judges that floor.**
- **The rule before it** judged all three metrics by the 5 % rule. It failed descriptors and connections on the
  pool's normal swings, on both soaks it judged (6 and 8 Oct). c15's numbers under both rules are below.

### Why c15 was respun: its soak's memory growth (M5)
- **The run.** c15's 24 h soak on its own images (gateway `8fe72138b44c`), with harness `8325ebaa`, from 8 Oct 16:40
  (evidence `r2-soak-24h-20261008-1639`; the earlier starts that day ended on harness problems and test-rule fixes, not
  on the product). It was **stopped at 9 Oct 11:40, after 19.0 h**, when the product owner chose to respin;
  `result.json` records `failed`, "soak interrupted".
- **The growth, against SOAK-001.** It's judged on the 12 h lifetime from hour 12, after its first hour: the second
  hour's median against the last hour's, and the least-squares slope projected to 24 h, the harness's horizon.
  - **The judged span:** the 7.0 h of that lifetime that ran.
  - **The data:** the soak's 2,263 resource samples (`chaos.jsonl`). The harness's own `lifetimes()` splits them at
    its 12 recorded restart windows, by each sample's start time; the judged lifetime holds 834 samples.
  - **The judge:** `deploy/tests/soak_growth.py`, 15b's for the old rule, the PRD §12 item 25 version for the new.

  | Metric (c15, 8 Oct) | The 5 % rule on every metric (before 9 Oct) | The Product Owner's rule of 9 Oct 2026 (PRD §12 item 25) |
  |---|---|---|
  | Gateway RSS | 36.1 MB → 37.7 MB, +4.3 %; projected **+34.2 % over 24 h: fails** | the same rule: **fails** |
  | Open descriptors | 57 → 59, +3.5 %; projected **+20.9 %: fails** | without the database connections: 51 → 51, 0.0 %; projected −1.3 %: passes |
  | Database connections | 7 → 9, +28.6 %; projected **+180 %: fails** | the gateway's highest 15 against a ceiling of 16; at the pool's maximum in 0 % of the last hour: passes. The floor isn't judged on a 7 h lifetime (it needs 11 h) |
  | **Verdict** | **fails on all three** | **fails on RSS only.** The floor wasn't judged: the run stopped at 19 h, so its last lifetime ran 7 h, not 12 |

  - **RSS's failure is real under either rule:** c15's two heap leaks, below. Over the whole run, RSS went from 34.8 MB
    to 38.2 MB.
  - **The descriptors and connections failed the old rule on noise.** The descriptors other than database sockets
    stayed at 51, while the pool's normal swings (4 to 16 connections within an hour) moved the rest.
  - The gateway's anonymous memory, read directly at 08:42 and 09:12 on 9 Oct, rose from 13.3 to 14.8 MB, about
    50 KB a minute, while its file pages stayed flat: the growth was the heap.
- **The cause: two leaks in the gateway, found by reading the code.**
  1. **Metric histograms kept every sample when nothing scraped `/metrics`** (the larger). Each HTTP request and each
     relayed model call records a histogram sample. The recorder the gateway builds runs no upkeep of its own, so
     samples were only folded into their buckets (and freed) when `/metrics` was rendered. The soak's installation
     doesn't serve metrics, so every sample stayed for the gateway's life; any install with metrics off, or never
     scraped, did the same. **Fixed in `53202edc`:** the gateway folds the histograms every 5 s for its whole life
     (GW-U-177).
  2. **The event hub kept one node per task until the task was deleted.** Each node holds a broadcast channel whose
     ring (1,024 slots, about 40 KB) is allocated even when empty, and nodes were dropped only on delete or at a cap of
     4,096. The soak rotates its tasks without deleting them (55 tasks in 16.6 h). **Fixed in `16235fe4` and
     `23112f6b`:** every minute, the gateway drops the nodes nobody holds, meaning no open stream and no append in
     flight (GW-U-178).
- **The respin:** `release/c15` moved to `23112f6b`, and 15b was built from it. Whether the two fixes end the growth is
  shown only by 15b's soak (GW-U-177 and GW-U-178 test the mechanisms, not freed memory).

### A failed soak that started Gate 9 (9 Oct 11:41), and its fix
- **What happened.** When c15's soak stopped (11:40), the tooling that chains the soak and the Gate 9 runs went on to
  Gate 9 at 11:41, although its rule is that a failed soak never does. Gate 9's first job (PERF-001's baseline build)
  was stopped by hand within a minute; no Gate 9 result was recorded, and the host's pause was released.
- **The cause.** That tooling (outside this repository) is a long-running shell script. Its check on the soak's result
  was added on 8 Oct at 16:33, after the c15 run had started; the file was replaced on disk, and the running instance
  kept reading the old one, without the check.
- **The fix (9 Oct):** the script now runs from a private copy made at its start and logs that copy's SHA-256;
  just before Gate 9, it reads the soak's `result.json` again and stops unless it says `passed`. Tested on a dry run
  (the copy and its SHA) and on the check's own lines (failed, passed and missing results).

### 15b's runs
| Run | Result | When / evidence |
|---|---|---|
| SOAK-001…004, 24 h (from 9 Oct 12:48, evidence `r2-soak-24h-20261009-1248`) | **Passes PRD §12 item 25's rule**, judged on the 12.06 h lifetime (2,864 samples, 12 restart windows): gateway RSS 37.4 MB → 37.9 MB, **+1.2 %**, projected **+0.7 %** over 24 h; descriptors other than the gateway's database connections 51 → 51, 0.0 %, projected −1.7 %; the gateway's database connections at most **16** against a ceiling of 16, at the maximum in **0 %** of the last hour, floor (10th percentile) **6 → 6**. **The gap, accepted by the product owner (10 Oct):** the harness signed in once, and at hour 24 its sign-in had expired (the server's 24 h session limit, HTTP 401), so the end-of-run receipt and run checks of the last 8 of its 72 conversations didn't run; `result.json` says `failed` for that reason. Orphans: 0, checked by hand. The harness now renews its sign-in (a 2 h rehearsal with 60 s sessions passed on 11 Oct, every end-of-run check run); c16's soak must complete every end-of-run check | The offline judge on the soak's samples (`offline-judge.json`: "all_checks_pass": true); the soak ended 10 Oct about 12:48 |
| Gate 9 steps a, b, d against c14b-rc (`16589e55`), ABBA | **Passed** (11 Oct 00:22 to 02:17). Candidate rounds (two each): PERF-001 journal p50 0.80 / 0.83 ms, p99 4.70 / 1.04 ms, memory p99 0.094 ms both; PERF-003 added p50 0.69 / 0.81 ms, p99 2.18 / 1.85 ms; PERF-010 p50 452 / 448 ms, p99 630 / 626 ms; PERF-011 new task p50 501 / 510 ms (the owner's 3 s met), returning 377 / 375 ms; PERF-020 CPU p95 18.9 / 18.4 %, RSS 12 MB, events ≥ 4,800. **PERF-002:** the run flagged its p99 against c14b-rc ("regression over 10 %"), at p99 30.5 / 11.6 ms against the baseline's 28.1 / 7.5 ms, the baseline's own rounds 3.7× apart. Run alone on the quiet host straight after (02:18 to 02:26, ABBA): baseline p99 9.22 / 7.41 ms, candidate 8.07 / 7.64 ms, **no regression** (exit 0); PRD's bound is p99 ≤ 100 ms | The run's index and every row's JSON, kept: `tree-results-20261011-021714` (`gate9-2026-10-10T16-23-09Z.json`); PERF-002 alone: `perf002-only/`. **Why a rerun:** the first run, 10 Oct, kept only its log; its script's final clean-up removed the tree that held the JSONs. The script now copies them out first |
| PERF-021 (step c), alone on the quiet host | **Passed** (11 Oct, 03:33 to 04:33): CPU 261.55 s an hour (p50 3.149 s a minute), 58 bytes a stream a minute | `perf-021-203350.json` (the U00 step's results) |
| PERF-012, Docker | **Passed** in the rerun's window (11 Oct 02:16): sequential write 0.927, read 0.998; random write 0.976, read 0.961 (bound ≥ 0.8). On 10 Oct a run by hand straight after the first Gate 9 run gave sequential write 0.653; an ABA rerun then (15b, c14b-rc, 15b) gave 0.923, 0.879, 0.922, so that value was recorded as an outlier | `perf-012-181650.json`; the ABA's JSONs in `perf-012/` |
| PERF-012, microVM | **Failed; waived by the product owner for 15c (11 Oct)**, as a known issue. Two measured runs (11 Oct 04:35 and 05:13; 3 warm-ups and 12 rounds each): sequential write **0.588** and **0.546**, random write **0.385** and **0.329**, random read 0.672 and 0.689, sequential read 0.837 and 0.803 (bound ≥ 0.7). The analysis: the method is fair on durability (both sides' writes reach the disk inside the timed span); it isn't a 15b change (nothing on the microVM path changed between c15 and 15b) and is the first valid microVM measurement; no CPU pinning and short random spans may cost the guest. The measurement's fixes and a fair run come with c16 | `perf-012-203534.json`, `perf-012-211331.json` |
| REL-I-001, 003 | **Passed** in the rerun's window (11 Oct 02:17): 7 tests, 2 skipped (the Firecracker artefacts, which REL-I-002 covers) | the rerun's `rel-i.log` |
| REL-I-002, on 15b's own rootfs | **Passed** (11 Oct 04:34): 7 tests, OK, on the Workstation rootfs built from `1648c896` the same morning. An earlier attempt (03:33) ran before that rootfs was built and errored; it isn't a result | the U0a step's log |
| M1 and M6 (the waves and E2E-CMP-003, on the GPU) | **Passed** (10 Oct 20:04 to 22:44): E2E-CMP-003 50/50 against the reference's 50/50 (0 points; M6's bound ±5); E2E-WAVE-001 60/60; E2E-WAVE-002 60/60 with 8 helpers | `gate-5.md` |

REL-I-002 and PERF-012 are P1 rows: a fail of either blocks the push until it's explained and fixed, or the Product
Owner waives it in writing (decided on 9 Oct 2026). **PERF-012's microVM half was waived so** on 11 Oct, and 15c
was pushed to main the same day (`1586fd9f`, its tree `release/c15` at `bf85085f`).

## Release candidate c15 (8 to 9 Oct 2026; respun as 15b)
**C15**, the tag file `candidate15-tags.txt`, built 8 Oct 01:22 at runtime protocol revision 13:
- gateway `8fe72138b44c`, web `c21dae74e9d7`, runtime `489ab9525b4c`, Workstation `797499325f94`, fake model
  `03718db2e50f`, VM host `d6a37e4c360a`.

c15 differs from c14b-rc by c15's merges, listed in its release notes. Gate 9's own runs on it are planned for a quiet
host after the soak (the plan for c15: steps a, b and d against c14b-rc, PERF-012 on Docker and the microVM, PERF-040,
REL-I-001…003, about 2 h 20 min; PERF-021 carried over). Until then:

**M5, the soak.**
- The 24 h soak on c15's images started 8 Oct 06:02 (evidence `r2-soak-24h-20261008-0602`), and ends about 06:15 on
  9 Oct (24 h, then a 10 min drain).
- **Its harness is `abac7217`:** `b5caac76` plus one soak-harness fix in `deploy/tests` only. The images are c15's.
- **A first start at 05:53 ended after a minute on the harness's own setup.** The harness read its scratch
  installation's `gateway.toml` after handing that folder to the gateway's user, and couldn't read it (permission
  denied); `abac7217` fixes that. No resource sample was taken, nothing was left behind, and the candidate wasn't
  involved.

**The integration's values (indicative for the verdicts, which take the quiet reruns).** From the integration of
`b5caac76` (8 Oct 01:23 to 04:24 +08:00, logs `int-b5caac76-*`). The performance steps started at a one-minute load of
1.58.

| ID | c15 integration value | Bar | c14b-rc (Gate 9 run) |
|---|---|---|---|
| PERF-001 (M2) | journal p50 0.79 ms, p99 0.90 ms; memory p50 0.089 ms, p99 0.156 ms | pass | journal p50 0.78 ms, p99 1.07 ms |
| PERF-002 | lag p50 1.49 ms, p99 7.5 ms; 550,000 events sent and stored. On a disk database (fsync on): p50 1.53 ms, p99 8.94 ms | p99 ≤ 100 ms | p50 1.42 ms, p99 8.2 ms |
| PERF-003 | added per request p50 0.97 ms, p99 3.08 ms; to the first token p50 0.96 ms | p50 ≤ 10 ms and ≤ 20 ms | p50 0.89 ms, p99 3.5 ms |
| PERF-010 (M3, Docker) | fresh p50 432 ms, p99 630 ms; returning task p50 443 ms, p99 688 ms | p50 ≤ 3,000 ms | fresh p50 442 ms, p99 639 ms |
| PERF-011 (M3, microVM) | new task p50 506 ms, p99 640 ms; returning task p50 383 ms, p99 418 ms; upload to a new task p50 469 ms, p99 632 ms; PRD M3's 3 s met | p50 ≤ 2,000 ms | new task p50 501 ms, p99 574 ms |
| PERF-020 (M4) | 0 dropped (at least 4,800 events); CPU p95 18.2 %; RSS 12 MB | CPU p95 < 70 %, RSS < 1.5 GB | CPU p95 20.1 %, RSS 12.9 MB |
| PERF-030 | p50 1.82 ms, p99 2.22 ms | pass | – |

**M7, isolation on the microVM:** G-13's egress, escape and history gates passed on c15's images, in the
integration's first pass and in its rerun (8 Oct 05:19 to 05:28; Gate 8, Candidate 15). The first pass failed only
G-13-suites, on SBX-F-009's host memory.

**M8, openness:**
- **Coverage:** pure crates 95.61 % lines (G-04 met; the whole workspace with the database and Docker suites is
  88.16 %).
- **Reproduction:** `gate9_run.py --dry-run` against this candidate passed (its tree, images, database and every
  job's command).
- **The release dry run** (`release.py`, version `1.0.0-rc.1` of `b5caac76`, 8 Oct 01:33 to 01:50, nothing
  published) passed its 8 checks:
  - the release directory complete, with no partial left, and `SHA256SUMS` verifying;
  - `release.json` saying candidate, not releasable, not published;
  - every image ID matching the local engine;
  - the notes saying it needs the owner;
  - the vulnerability gate's report written;
  - no tag `v1.0.0-rc.1` created, and nothing pushed.

  Its vulnerability gate: **0 HIGH** (156 before the images' security updates, `17d51fa4`). Five medium findings
  remain in the Workstation (ip-address, postcss-selector-parser), which were there before.

**M1 and M6** (the waves and E2E-CMP-003, on the GPU) aren't scheduled on c15 yet.

**Beside the gate:**
- the upgrade test from the review install's database (at migration 79, before c15) to c15 passed 33 of 33 checks (8 Oct 05:32; users,
  workspaces, conversations and channels intact; only the expected rows changed by 0088);
- the agent-teams stack suite passed 5 of 6 on c15 (E2E-AGENTS-020, 022 to 025; 021 is a test waiting for c16's
  fix).

## Release candidate c14b-rc, Gate 9 runs (5 to 7 Oct 2026)
Candidate 14b's images with the rebuilt web image, the tag file
`candidate14b-rc-tags.txt` (gateway `fde0b91bf88b`, web `83a8d1ad493f`, runtime `347b895b203d`, Workstation
`84ff9c47aa24`, fake model `272dfcee3718`, VM host `c8a54cf8f5ea`). Measured by `deploy/tests/gate9_run.py` from
`2b0ff0bd` (`101ffeca` plus the harness fixes of that run) in ABBA order against 13h (`5b79a932`), each job under the
load watch (a result is disturbed if the one-minute load rose above 10). PERF-011 and PERF-021 ran in a quiet window
(6 Oct 01:02–03:03) after a calm wait (load under 4 for 3 minutes, then quiet I/O). The index, from `89c876c8`, is
`gate9-2026-10-05T19-03-57Z.json` (`gate9_run.py --index-only` over the kept results, with the notes under "The next candidate's runs"); every
candidate run of PERF-001/002/003/010/011/020/021 passed undisturbed and is recorded. Values are the mean of the two
candidate runs unless stated; the raw results stay on the run host, beside the index.

| ID | c14b-rc value | Bar | 13h (indicative) |
|---|---|---|---|
| PERF-001 (M2) | journal p50 0.78 ms, p99 1.07 ms; memory p50 0.072 ms, p99 0.087 ms | pass | journal p50 0.93 ms, p99 1.42 ms |
| PERF-002 | lag p50 1.42 ms, p99 8.2 ms at 10,000 events/s; 550,000 sent, stored and replayed | p99 ≤ 100 ms | not measurable (below) |
| PERF-003 | added per request p50 0.89 ms, p99 3.5 ms; added to the first token p50 0.89 ms | p50 ≤ 10 ms and ≤ 20 ms | first token p50 4.31 ms; total not comparable (below) |
| PERF-010 (M3, Docker) | fresh p50 442 ms, p99 639 ms; returning task p50 419 ms, p99 692 ms | p50 ≤ 3,000 ms | p50 358 ms, p99 632 ms (one run, indicative; below) |
| PERF-011 (M3, microVM) | first start 419 ms; new task p50 501 ms, p99 574 ms; returning task p50 381 ms, p99 433 ms; upload to a new task p50 424 ms, p99 525 ms; PRD M3's 3 s met | p50 ≤ 2,000 ms | new task p50 613 ms; upload p50 571 ms |
| PERF-012 (Docker) | bind mount ratios: sequential write 0.93, read 1.01; random write 0.98, read 0.97 | ≥ 0.8 | not run |
| PERF-012 (microVM) | invalid measurement, re-measure pending (finding 6; gaps list 7) | ≥ 0.7 | not run |
| PERF-020 (M4) | 16 conversations, 0 dropped; CPU p95 20.1 % of 2 vCPU; RSS peak 12.9 MB | CPU p95 < 70 %, RSS < 1.5 GB | CPU p95 22.0 %, RSS 12.6 MB |
| PERF-021 | 64 idle streams for 55 minutes: 2.6 CPU-seconds a minute (p50), 58 bytes a stream a minute; gateway RSS −0.7 % | growth < 5 % | not run |
| REL-I-001/002/003 | 7 of 7 tests pass; REL-I-002 with the Workstation rootfs built from `84ff9c47aa24` | pass | not run |

**The GPU rows (7 Oct 2026).** The waves on c14b-rc's images (gateway `fde0b91bf88b`, runtime `347b895b203d`,
Workstation `84ff9c47aa24`), the wave runner from a clean checkout of `16589e55` (its run record), and the same 27B
open-weight model (FP4) as before (`waves.md`, Setup). E2E-WAVE-001 ended 06:48; E2E-WAVE-002 ran 06:59:52–07:11:13, inside the logged admission
step (route `local` at concurrency 8, restored at its end). Raw data: the wave runner's records
`tests/e2e/results/2026-10-07/slot2-wave-c14b-rc` and `slot2-wave8-c14b-rc` (`run.json`, `attempts.jsonl`, `events/`,
`summary.md`), kept on the run host. Command: the wave runner with `--route local --repeat 3 --max-total-tokens 30000000`
and `--concurrency 1` (E2E-WAVE-001) or `--concurrency 8` (E2E-WAVE-002); the method is `waves.md`'s, with the run
cap raised to 30 million tokens.

| ID | c14b-rc value | Bar | 13h (indicative) |
|---|---|---|---|
| E2E-WAVE-001 (M1) | 60/60 = 100 % (95 % Wilson 94.0–100 %), 0 harness failures, no provider retry | ≥ 90 %, failures classified | 58/60 |
| E2E-WAVE-002 (M1) | 59/60 = 98.3 % (91.1–99.7 %), 0 harness failures; one miss, `ts-add-tests-slug`, the model's (classified by hand, `waves.md`) | ≥ 90 %, failures classified | 59/60 |

A VM host crash during the 5 Oct PERF-011 run (exit 139 after start, the upload answered 409) is finding 5. The quiet
rerun's four rounds had none (a watcher kept every sandbox's logs from its start).

## Candidate 13h, indicative (1 to 2 Oct 2026)
Measured by the technical lead's integration of `5b79a932` and its quiet reruns from `ae236763` (13h's images). The
raw results stay on the run host, outside the repository: the integration checkout's `tests/perf/results/2026-10-01/`
(and `target/perf-results/` for PERF-001's own copy), and the logs `~/.cache/int-5b79a932-*.log` and
`~/.cache/rerun13h-*.log`. Each line names its result file and the command that reproduces the number alone on a
candidate (`deploy/tests/gate9_run.py --candidate-only`, a reproduction and never gate evidence).

| ID | Value | Result file | Reproduce |
|---|---|---|---|
| PERF-001 (M2) | journal p50 1.90 ms, p99 28.7 ms; memory p50 0.076 ms, p99 0.078 ms; 5 warm-ups, 50 samples, 10 steps; load 25.0 before, 23.3 after | `PERF-001-2026-10-01T15-56-01Z.json` | `gate9_run.py --candidate-tags C --ids PERF-001 --candidate-only` |
| PERF-010 (M3, Docker) | p50 515 ms, p99 681 ms | `perf-010-152048.json` | `gate9_run.py --candidate-tags C --ids PERF-010 --candidate-only` |
| PERF-011 (M3, microVM) | cold boot 209–240 ms in the integration, 170–210 ms on the reruns (five samples each); first start 628 ms; a new task p50 674 ms, p99 724 ms; a returning task p50 401 ms, p99 455 ms; an upload to a new task p50 626 ms, p99 722 ms (the quiet rerun; the integration's run failed on SBX-F-032's 10 s bound, see `gate-8.md`) | `perf-011-*.json` of 2 Oct, log `rerun13h-perf-start-fc.log` | `gate9_run.py --candidate-tags C --ids PERF-011 --candidate-only --kvm` |
| PERF-020 (M4) | 16 conversations, 3 runs of 4,800 events each, 0 dropped or repeated (the harness fails a run on any gap in the sequence), 0 events after completion; CPU p95 19.2 %, max 19.2 %, mean 12.6 % of 2 vCPU; RSS peak 12,161,024 bytes | `perf-020-152130.json` | `gate9_run.py --candidate-tags C --ids PERF-020 --candidate-only` |
| E2E-WAVE-001/002 (M1) | 58/60 and 59/60 (`waves.md`) | the wave runner's `tests/e2e/results/2026-10-02/slot2-wave-13h` and `slot2-wave8-13h` | `waves.md`, Method |
| G-04 (M8 coverage) | pure crates 95.90 % lines | log `int-5b79a932-cov-pure.log` | `tools/check.sh` (G-04) |

Not run on 13h, in this gate's scope (the TEST-PLAN audit of 13h): SOAK-001…004 (the 24 h run), PERF-002/003 (they
refuse a non-release build), PERF-012, PERF-021, REL-I-001/003 (Docker-gated), TK-I-006 and E2E-CMP-003.

## The Product Owner's questions (asked with the gate review)
1. **M1:** the wave model is a 27B open-weight model in FP4 (`waves.md`, Setup). Does it count as "GLM-5.3-Flash
   class"?
2. **M3:** the warm-snapshot target (≤ 500 ms) is a Phase 4 stretch that is not built: drop it from v1.0, or build and
   measure it?
3. **M7:** accept the SEC-023 limit on the microVM (skills mounts unsupported there), and the audit's G1 row.
4. **M6, if the runs can't decide it:** at 50 attempts a side the 95 % interval is about ±12 points, wider than PRD's
   ±5; if the next candidate's interval contains both −5 and 0, the technical lead brings the numbers to the owner.

## The next candidate's runs
From a clean checkout of the candidate commit, on a quiet host, through `deploy/tests/gate9_run.py` (PRD M8's
reproduction script: it waits for a one-minute load under 10, marks a result disturbed if the load rose above 10, and
compares PERF-001/002/003/010/011/020 with the previous release in ABBA order):
- step a: PERF-001 (M2, on an idle host), PERF-002, PERF-003 (release);
- step b: PERF-010 with the person's view of a start, PERF-020 (M3, M4);
- step c: PERF-021 (1 h of idle streams);
- step d (`--kvm`): PERF-011 with the person's view on the microVM (M3);
- step e: the 24 h soak at 8 conversations, SOAK-001…004 (M5);
- outside the script: PERF-012 on Docker (`deploy/tests/perf_io.py`) and on the microVM
  (`deploy/firecracker/perf_io_vm.py`, from the next measurement; see the gaps list), E2E-CMP-003 at 50 attempts a side (M6,
  `comparison.md`), the waves (M1, `waves.md`), REL-I-001/003 with Docker (`tools/release/test_images.py`,
  `TQA_DOCKER_TESTS`), TK-I-006 (`tests/e2e/concurrency_run.py`), and every other P0 and P1 row the candidate's
  TEST-PLAN audit lists as not run.

PERF-003 against 13h (candidate c14b-rc, 5 Oct 2026): the baseline total added is consistently negative in 13h (-31.9,
-30.6 ms): not comparable as a percentage; judged by the absolute floor (1 ms). The comparable first-token added p50
went 4.3 -> ~0.6-1.1 ms. Why: 13h predates `021f7bbc` (the fake model server sets TCP_NODELAY) and the baseline tree
runs 13h's fake model server, so its direct reference waits out delayed ACKs (direct total p50 20.6 and 20.4 ms against
0.08 ms on the candidate), and "relay minus direct" goes negative. The baseline-vs-candidate comparison is therefore not
like-for-like (different fake model servers): PERF-003's verdict rests on the candidate's absolute added latency
against the floor. `gate9_plan.compare` clamps any added latency at or under 0 to 0 and says so in the index.

Gate 9 is measured on the candidate, with 13h indicative only: the verdict for PERF-002, PERF-010 and PERF-020 rests on
each row's absolute bar, which the candidate's jobs meet undisturbed.
- PERF-002: the baseline is not measurable. 13h can't sustain the offered rate ("the link held the sender back" in both
  baseline runs), so PERF-002 is candidate-only.
- PERF-010 and PERF-020: the baseline is 13h's gateway with c14b's Workstation image. 13h can't pin its built-ins, and
  the engine's `tqa-runtime:workstation` tag points at c14b's image, so the comparison isolates the start code only and
  is indicative. PERF-010's baseline round 1 was disturbed (load up to 26.5) and is excluded.

## Gaps list (4 Oct 2026)
1. **M2–M4:** the next candidate's step a, b and d runs (13h's values are indicative).
2. **M5:** the 24 h soak on the next candidate, with its evidence kept.
3. **M6:** E2E-CMP-003 at 50 a side on the next candidate.
4. **M1, M3, M7:** the Product Owner's questions above.
5. **M8:** each published number's raw data and command, on this page and in `waves.md` (done for 13h), and for the
   next candidate's results as they come.
6. **Scope rows not run on 13h:** PERF-002/003/012/021, REL-I-001/003, TK-I-006, and the soak rows.
7. **PERF-012's microVM half is unmeasured** (5 Oct 2026). TEST-PLAN's target for it is a Firecracker block image at
   ≥ 70 % of the host disk. `deploy/tests/perf_io.py` measures a Docker sandbox only, and no driver measures a guest's
   workspace image. So this candidate's PERF-012 is the Docker half (bind mount ≥ 80 %), and the microVM half is
   recorded as unmeasured, not as met. Its driver, `deploy/firecracker/perf_io_vm.py` (TK-U-045, offline-tested),
   exists since 5 Oct; the half stays unmeasured until a measurement on a KVM host runs it.
   Candidate c14b-rc, PERF-012 microVM (`deploy/firecracker/perf_io_vm.py` from `57bd2d64`, 5 Oct 2026): invalid
   measurement, re-measure pending (finding 6). The guest's reads and writes hit the host page cache (sequential ratios
   2.1 and 3.5, random write 0.09), and the drives ran on Firecracker's default cache mode (Unsafe);
   the drive cache fix (SBX-F-039) and the corrected driver address both, and the re-measure runs with them.
