# Gate 8: Firecracker

**State:** **reached on release candidate 15b**, by the technical lead's verdict (11 Oct 2026; "Evaluation of
15b", below): every scope row passed in 15b's first pass, SBX-F-009's c15 failure is accepted as host memory
pressure, three TEST-PLAN §19a entries are closed, and the microVM host-key gap is recorded outside the gate's scope.
15c (15b's server with a new web image) is on main as `1586fd9f`. Before that: **On c15 (8 Oct 2026), every row but one passed in the integration's first pass on the candidate's
images; SBX-F-009 failed there** (a vCPU ENOMEM under host memory pressure; the analysis below) **and passed in the
G-13 rerun**, with all 18 gates. **9 Oct: c15 was respun as 15b** (dev `23112f6b`; only the gateway's telemetry and
event hub changed), so the evaluation is drafted for 15b below. **11 Oct: refreshed on 15b, every row passed** (G-13
18 of 18 on the first pass, SBX-F-009 included; PERF-011 within the owner's 3 s; REL-I-002), **except PERF-012's
microVM half, which failed and was waived by the product owner for 15c**. The gate stays at that until its evaluation
is signed (the technical lead, 11 Oct); it was signed the same day.
Earlier: refreshed on candidate 14b (`c14b-rc`, dev `16589e55`, the integration of 4 to 5 Oct 2026) for the
technical lead's acceptance. **Not reached** until that evaluation is recorded here. On 14b every row in the gate's
scope passed in the integration's first pass, under its load, with no rerun: G-13 passed all 18 of its gates, and
13h's first-pass failures did not recur. Still open: the closing of four TEST-PLAN §19a entries and the evaluation
itself. The 13h refresh, what dev added between the two candidates, the 13g
refresh and the history from 26 to 28 Sep follow.

## Scope
The TEST-PLAN §17 Gate 8 row: SBX-C-001…016 on Firecracker; SBX-F-001…007 and SBX-F-009; since 1 Oct 2026 also the
backend's P0s built in this phase, SBX-F-010…014, SBX-F-016…018, SBX-F-020, SBX-F-022 and E2E-VM-013; since 4 Oct 2026
also the microVM rows SBX-F-023…025 and SBX-F-027…036; since 7 Oct 2026, from candidate 15 on, also SBX-F-038 and
GW-I-227 (a VM host that dies before its runtime registers: logged with why, started once more; not in c14b-rc);
SEC-001…006 (SEC-003 removed) and SEC-014 on Firecracker; PERF-011; plus gate G-13. The phase demo is E2E-VM-012
(= SBX-F-006).

## Candidate 15 (7 to 8 Oct 2026): known issue so far
c15 was respun as 15b on 9 Oct, and its evaluation is drafted for 15b (below, after this section's rows). This section
holds the one Gate 8 row that failed in c15's integration, and the lead's decision on it (8 Oct 05:29).

**A VM host dies at start when the host kernel can't allocate a vCPU (SBX-F-009, ENOMEM): host memory pressure, not
c15.**
- **What happened.** C5's integration (dev `b5caac76`, images of 8 Oct 01:22) failed one test of G-13-suites,
  SBX-F-009/SBX-C-011, 16 microVMs at once within the memory budget (8 Oct 02:57). Two of the 16 VM hosts exited 139
  about 0.23 s after starting, with `oom=false`. SBX-F-038's new logging kept the hosts' last lines. In both,
  Firecracker v1.17.0 itself panicked at `src/vmm/src/vstate/vcpu.rs:144:48`: `called Result::unwrap() on an Err
  value: VcpuFd(Error(12))`. Errno 12 is ENOMEM, returned by a KVM vCPU ioctl. That's host kernel memory, not the
  container's cgroup. The test took 217 s, against about 42 s on a quiet host. Its failure cancelled the suite's other
  11 of 97 tests (fail-fast). The VM host was the gate's only Team A job at the time; the release dry run had ended at
  01:50.
- **Not c15's code or images.**
  - C5's VM host image (`sha256:d6a37e4c360a`, the new Debian base of the release gate fix) and 2012f9d5's
    (`sha256:9c45ccc9cc1e`) carry the same statically linked Firecracker binary (sha256 `99ad0f5cd0514a88…`,
    v1.17.0). The base image doesn't touch Firecracker's code.
  - On the same rootfs (`489ab9525b4c…`), kernel and test binary, 8 Oct 05:14 to 05:18: C5's VM host passed 3 of 3
    (41, 43 and 42 s) and the old one 3 of 3 (45, 42 and 42 s), with no exit 139 and no panic.
  - The whole G-13 on C5, rerun at 05:19 to 05:28: exit 0, all 18 gates passed, the suites 97 of 97 (the 11
    cancelled ones included), SBX-F-009 in 42.4 s. A 5 s memory sampler showed at least 51.2 GB of 63.5 GB
    available throughout. Swap already held 8.4 GB, so the host had been short of memory earlier.
- **Likely cause.** Starting 16 VMs at once, the host kernel couldn't allocate KVM's per-vCPU memory. Other teams'
  load at 02:57 isn't recorded.
- **Impact.** That VM fails to start and its start is reported as failed; the host and the other VMs stay up. The
  gateway starts a VM host that dies before its runtime registers once more (GW-I-227). Firecracker panics instead of
  returning the error, which is upstream behaviour (an `unwrap` in `vcpu.rs`).
- **Likely the same as c14b-rc's P1 known issue** (below, in candidate 14b's evaluation: "The SIGSEGV: not
  reproduced, no host log, outside the gate", finding 1. A VM host exited 139 about 0.2 s after starting, before its
  runtime registered, with no host log). The shape matches: exit 139, about 0.2 s, before registration. This time
  SBX-F-038 captured the cause. It also means that P1's "fault before Firecracker's own handlers" reasoning no longer
  holds: Firecracker can print a panic and still end with 139. This is an inference from one shape, not a
  reproduction of the 14b crash.
- **Evidence.** `int-b5caac76-kvm.log` (line 139). The reruns, with host memory before and after each run, and the
  G-13 rerun with its sampler are in the team's `c15-run/sbx-f-009/` folder.

### The other rows on c15 (8 Oct 2026)
**Source:** the same integration of C15 = dev `b5caac76` (logs `int-b5caac76-*`, on the candidate's images: gateway
`8fe72138b44c`, runtime `489ab9525b4c`, Workstation `797499325f94`, VM host `d6a37e4c360a`), and the G-13 rerun above
(8 Oct 05:19 to 05:28, `c15-run/sbx-f-009/g13-kvm.log`).
- **Method:** each scope ID's Rust tests were found by the ID in a `TEST:` line or comment above the test, or in the
  file's header, and looked up in those logs.
- **The ID rows:** every row but SBX-F-009 had all its tests pass in the first pass: SBX-C-001…016,
  SBX-F-001…007, SBX-F-010…014, SBX-F-016…018, SBX-F-020, SBX-F-022…025, SBX-F-027…036, SBX-F-038, GW-I-227,
  SEC-001/002/004/005/006 and SEC-014 on Firecracker, 50 rows.
- **SBX-F-009** (with SBX-C-011's matching test) failed in the first pass, as above, and passed in the rerun. The 11
  tests its failure cancelled ran in the rerun, all passed (97 of 97).
- **G-13:** 17 of 18 gates passed in the first pass (G-13-suites failed on SBX-F-009); all 18 in the rerun.
- **E2E-VM-012** (= SBX-F-006, the phase demo): the `gate8` step passed. The killed VM's run was interrupted, resumed
  in a new VM on the same image and completed; the completed call's file survived; recovery took 1 s.
- **E2E-VM-013:** workspace files on the microVM backend, passed (`vmfiles`).
- **PERF-011** (`perf_start --kvm`): passed. New task p50 506 ms, p99 640 ms; returning task p50 383 ms, p99 418 ms;
  new task with an upload p50 469 ms, p99 632 ms; the owner's bound is 3 s.
- **Beside the gate:** the rootfs build tests (SBX-F-005/010) passed, and the file previews passed on the microVM
  (`demo previews firecracker`).

**What is left for the c15 evaluation:**
1. The technical lead's judgement on SBX-F-009's first-pass failure: the reading above is host memory pressure,
   with the rerun clean.
2. The four TEST-PLAN §19a entries still open from 14b (below).
3. The evaluation itself, after the soak.

### Evaluation for 15b (draft for the technical lead, 9 Oct 2026; 15b's results filled 11 Oct; signed below)
**Proposed verdict then: not reached yet. Gate 8 is reached on 15b** if the lead:
1. accepts SBX-F-009's first-pass failure on c15 as host memory pressure (above: the same images and binaries passed
   3 of 3 on a quiet host, and the whole G-13 rerun passed 18 of 18, with at least 51 GB of memory free);
2. closes the four TEST-PLAN §19a entries still open from 14b (below);
3. finds 15b's own results below in line with c15's.

**What 15b changes for Gate 8.** Between c15 (`b5caac76`) and 15b (`23112f6b`), only the gateway crate's source
changed: six files, the metric histograms' upkeep (GW-U-177) and the event hub's sweep of idle nodes (GW-U-178). The
sandbox, Firecracker, runtime, Workstation and VM host code is the same, and so is the gateway's microVM lifecycle.
15b's runtime, Workstation and VM host images were rebuilt from that same source (new image IDs, the same code).
So c15's Gate 8 rows above stand for 15b's code. The rows below confirm them on 15b's images.

**c15's evidence, carried over** (the integration of `b5caac76` and the G-13 rerun, above):
- every scope row passed, SBX-F-009 in the rerun;
- G-13: 17 of 18 gates in the first pass, 18 of 18 in the rerun;
- E2E-VM-012 (the phase demo on the microVM) and E2E-VM-013 passed;
- PERF-011: new task p50 506 ms.

**15b's results:**
| Row | Result on 15b's images | When / evidence |
|---|---|---|
| G-13 (all 18 gates), with SBX-F-009 in G-13-suites | **pass, 18 of 18 on the first pass** ("All gates passed"); SBX-F-009 (sixteen microVMs at once within the memory budget) passed both in the integration's Firecracker step (108 of 108) and in G-13-suites | 15b's integration, 10 Oct: `int-23112f6b-kvm.log`, `int-23112f6b-fc.log` |
| PERF-011 (M3, microVM): new task, returning task, upload | **pass** (the owner's bound 3 s). Gate 9's rerun, the candidate's two rounds: new task p50 501 / 510 ms, p99 578 / 581 ms; returning task p50 377 / 375 ms, p99 436 / 413 ms; upload p50 422 / 426 ms, p99 485 / 477 ms. The integration's run: new task p50 526, p99 618 ms | Gate 9's step d (11 Oct, 01:24 to 02:15), `gate-9.md`, 15b; `int-23112f6b-perf_start-fc.log` |
| PERF-012's microVM half | **fail** (sequential write 0.588 and 0.546, random write 0.385 and 0.329, against ≥ 0.7), **waived by the product owner for 15c on 11 Oct**; the analysis and the numbers are in `gate-9.md`, 15b | 11 Oct 04:35 and 05:13 |
| REL-I-002 (each template's rootfs is its source image plus the guest init) | **pass**: 7 tests, OK, on the Workstation rootfs built from `1648c896` | 11 Oct 04:34 |
| 15b's soak: no orphan VM hosts, taps or chroots left (SOAK-001, SOAK-004 on the soak's backend) | **Doesn't apply to 15b: the soak ran on the Docker backend** (its run script: "R2: the 24 h soak (SOAK-001…003, PRD M5) on Docker"), so it started no VM hosts, taps or chroots. Its orphans on Docker: 0, checked by hand (`gate-9.md`, 15b) | `gate-9.md`, 15b |

**Findings to decide** (from c15 and 14b, unchanged by 15b):
- SBX-F-009's first-pass ENOMEM: host memory pressure (condition 1).
- c14b-rc's P1 known issue (a VM host exiting 139 before its runtime registers, no host log): likely the same shape as
  SBX-F-009's, now with its cause captured, as above. Still an inference, not a reproduction.

### Evaluation of 15b (the technical lead's verdict, 11 Oct 2026)
**Verdict: Gate 8 is reached on 15b.** The technical lead accepts the reading in 1 and decides 2b as below. 15c, now on main (`1586fd9f`), runs 15b's sandbox, runtime, Workstation and VM host images unchanged; only
its web differs, and no Gate 8 row runs the web.

**The rows.** On 15b's integration (10 Oct, the candidate's images, logs `int-23112f6b-*`), every row in the gate's
scope passed in the first pass, with no rerun:
- the Firecracker step, 108 of 108 (`fc`), SBX-F-009 and SBX-C-011 among them;
- G-13, 18 of 18 gates, "All gates passed" (`kvm`), G-13-full and G-13-lifetime included;
- the phase demo E2E-VM-012 (`gate8`), E2E-VM-013 (`vmfiles`), the rootfs build tests (SBX-F-005, SBX-F-010) and
  the file previews on the microVM;
- PERF-011 within the owner's 3 s, here and in Gate 9's rerun (the table above);
- REL-I-002 on 15b's own rootfs (11 Oct).

This is a stronger result than c15's: there, SBX-F-009 failed its first pass and passed only in the rerun.

**1. SBX-F-009's first-pass failure on c15: host memory pressure (accepted).** The same images and binaries
passed 3 of 3 on a quiet host, and the whole G-13 rerun passed 18 of 18 (8 Oct). 15b runs the same sandbox and VM
host code, and SBX-F-009 passed in its first pass, under the integration's load. Firecracker panicking on the
kernel's ENOMEM, instead of returning it, is upstream behaviour; the gateway reports the failed start and starts such
a VM host once more (GW-I-227).

**2. The TEST-PLAN §19a entries for this gate.**
a. **Three are closed on 15b's evidence** (11 Oct), as each entry says:
   - **G-13-full's fill command passed its tool deadline** (SBX-C-015): "closes when G-13-full passes on the next
     candidate's loaded run". It passed in 15b's integration.
   - **Two Firecracker contract tests cut at the 30 s Docker API bound** (the posture tests, SBX-C-015): "closes if the
     next candidate's loaded runs don't show the cut". Both tests passed in 15b's Firecracker step (in 4.8 and 6.8
     s), and neither that step's log nor G-13's has a timed-out Docker request.
   - **G-13-lifetime: the rootfs build exited 141** (SBX-F-036): kept "until the fix's VM host image is rebuilt and
     G-13-lifetime passes again". It passed with 15b's rebuilt VM host.
   - (The fourth from 14b, **PERF-011's redacted retry**, is already closed, with SBX-F-035 as its test.)
b. **One stays open, outside the gate: "SEC-019 has no microVM half for host keys"** (P0, a test gap, not a product
   defect). A keyed request from a microVM task reaches the egress proxy through the guest's tap, not the Docker
   sandbox network, and no case checks it there: the host-key suites (SEC-047 to SEC-049, WEB-E2E-035) run on
   Docker only, and passed in 15b's integration. SEC-019 itself is retired, and neither it nor SEC-049 is in this
   gate's §17 scope (SEC-001…006 and SEC-014 on Firecracker). **Decided:** the gate is judged on its scope, so this
   entry stays open, outside the gate, as a **known gap of v1.0's microVM backend** (a P0 test gap), with the
   host-key microVM case planned for c17.
   - **12 Oct 2026:** that case (`crates/tqa-sandbox/tests/egress/host_keys_microvm.py`, SEC-047 and SEC-049 on the
     microVM) passed on c16's integration images (`3940a08d`); TEST-PLAN §19a has its run. **Closed** by the technical
     lead (12 Oct 2026).

**3. Findings outside the rows.**
- **c14b-rc's VM host crash at start (exit 139, 1 of 31 starts), its P1 known issue:** not seen since. 15b's
  Firecracker and G-13 logs have no exit 139. c15's ENOMEM showed the same shape with its cause logged (SBX-F-038).
  The 500-start reproduction, prepared on 8 Oct, hasn't run; it stays a likely reading, not a reproduction, and the
  reproduction is a follow-up item.
  - **Run 12 Oct 2026 on a quiet KVM host, on c16's integration images (`3940a08d`): not reproduced in 1,000 starts.**
    `deploy/firecracker/start_repro.py product` started 500 tasks through the product: 500 started, and all 500 VM
    hosts ended with exit 0. `bare` ran a clone of one VM host 500 times without the gateway: all 500 were still
    running at the end of their window. Every start used the default template on the base runtime, as c14b-rc's
    PERF-011 starts did (a task with no template, `TQA_RUNTIME_IMAGE` the base image), not the Workstation.
  - **What it can't say:** a quiet host recreates neither c15's memory pressure nor c14b-rc's load, so it doesn't rule
    out SBX-F-009's ENOMEM shape; 0 failures in 500 starts per mode bounds the start's own failure rate below about 0.6 % per mode (95 %, the rule of three), and below about 0.3 % over the 1,000. Recreating the
    pressure would need artificial memory load, which this shared machine doesn't run.
- **Drive caching:** since c15 the microVM's drives use `Writeback` (SBX-F-039), so a guest flush reaches the host's
  disk; c14b-rc's `Unsafe` finding is closed.
- **PERF-012's microVM half:** failed on 15b and waived by the product owner for 15c (11 Oct). It is FR-29's and Gate
  9's row, outside this gate's scope; its measurement fixes and a fair run come with c16 (`gate-9.md`, 15b).
- **The previews harness fix** (14b's condition): the file previews on the microVM passed in 15b's integration and in
  15c's run.

**Signed:** the technical lead, 11 Oct 2026. The three entries in 2a are closed in TEST-PLAN §19a; 2b's entry records
the decision.

## Candidate 14b (4 to 5 Oct 2026)
**Source:** the technical lead's integration run on `16589e55` (images built from it at 23:45 on 4 Oct; the run ended at
02:45 on 5 Oct with exit 0, logs `int-16589e55-*`). The images: gateway `sha256:fde0b91bf88b`, runtime
`sha256:347b895b203d`, Workstation `sha256:84ff9c47aa24`, VM host (`tqa-vmm`, rebuilt) `sha256:c8a54cf8f5ea`. Each row was
checked against its log: a row passes only if each of its test IDs ran and none failed. Every row below passed in the
first pass; nothing needed a quiet rerun.

| Rows | 14b on Firecracker | Evidence (log) |
|---|---|---|
| SBX-C-001…005, SBX-C-009…012, SBX-C-014…016 | pass | the Firecracker step, 106 run, 106 passed (`int-16589e55-fc.log`); G-13-suites, 95 of 95 (`int-16589e55-kvm.log`). SBX-C-005/006/007/016 are one posture test, SBX-C-011 is inside SBX-F-009's density test |
| SBX-C-008 reconciliation, SBX-C-013 destroy ends an active process | pass | `G-13-reconcile` (the orphaned VM host destroyed, the two bound ones kept), `G-13-destroy` (`int-16589e55-kvm.log`) |
| SBX-C-003 OOM, GW-I-066 lifetime, SBX-C-015 a full workspace (G-13's cases) | pass | `G-13-oom`, `G-13-lifetime` (retired 182 s in, cap 60 s + grace 120 s; its rootfs build passed on the rebuilt `tqa-vmm`), `G-13-full` (ENOSPC from a command and a file tool, one `workspace.full`, the run continued) |
| SBX-F-001…004, SBX-F-007, SBX-F-009, SBX-F-011, SBX-F-013, SBX-F-014, SBX-F-017, SBX-F-018, SBX-F-022 | pass | the Firecracker step and G-13-suites; SBX-F-007 also as `G-13-reboot` |
| SBX-F-016 the guest init | pass | the unit step, `tqa-vm-init`'s six SBX-F-016 tests (`int-16589e55-unit.log`, 2323 of 2323) |
| SBX-F-005 rootfs lineage, SBX-F-010 rootfs build | pass | `images/rootfs/test_build_rootfs.py`, 12 tests OK (`int-16589e55-rootfs.log`) |
| SBX-F-006 VM crash (the demo, E2E-VM-012) | pass | `tests/e2e/gate8.py` (`int-16589e55-gate8.log`) and `G-13-demo`: the killed VM's run resumed in a new VM on the same image and completed, the completed call's file survived, recovery 1 s |
| SBX-F-012 host setup | pass | `G-13-unit` (`deploy/firecracker/test_host_setup.py` among its 108 tests) and `G-13-host` |
| SBX-F-020 the product on the microVM | pass | `G-13-stack`: a task ran in a microVM through the public API, the shell saw the guest kernel |
| SBX-F-023…025, SBX-F-027…035 (budgets, retries, clean-up) | pass | the Firecracker step and G-13-suites; SBX-F-034's image clean-up also as `G-13-images` ("no test images left behind") |
| SBX-F-036 a first-line read under `pipefail` | pass | `check.sh`'s DEPLOY-U-tools (`tools/test_pipefail_first_line.py`, `int-16589e55-check.log`), and in effect `G-13-lifetime`'s rootfs build |
| E2E-VM-013 workspace files on the microVM | pass | `tests/e2e/vm_files.py` (`int-16589e55-e2e-vmfiles.log`): files, uploads, attachments, run changes and fork from the runtime in the guest; an upload that starts the VM 470 ms, a message and run about 530 ms |
| SEC-001, SEC-006 (`egress`), SEC-002 and SEC-014's sandbox side (`escape`) on Firecracker | pass | `G-13-egress` (no secret in the guest or the transcript, every bypass failed), `G-13-escape` (18 escapes refused with typed codes); SEC-017's stack half also passed (`G-13-history`) |
| SEC-004, SEC-005, SEC-014's gateway side | pass, run once | the gateway-side rows, run once on either backend: the ignored step (`int-16589e55-ignored.log`, 2540 of 2540) and the unit step |
| PERF-011 cold boot to dial | pass | `perf_011_firecracker_cold_boot_to_dial` in the Firecracker step and in G-13-suites (target ≤ 2 s); the logs keep the verdict, not the samples |
| G-13 (`tools/check.sh --kvm`) | **pass, 18 of 18** | `int-16589e55-kvm.log`: every gate the script runs, "All gates passed"; each stack gate's start-up had 0 engine timeouts and 0 retries |

**The gate count.** `check.sh --kvm` runs 18 gates at `16589e55` and on dev (unit, images-unit, host, kvm-acl,
suites, images, stack, oom, start-failure, egress, escape, full, destroy, lifetime, reconcile, reboot, history, demo).
The "19" in "Since candidate 13h" below doesn't match the script; 18 is the count.

**Outside this row, on the same run:**
- **PERF-012's microVM half** (a related note, not this gate's scope: PERF-012 belongs to FR-29 and Gate 9's remaining
  P1s): not run yet on 14b; it waits for its own slot after Gate 9's measurements, and its result is noted here when it
  lands.
- **PRD M3 as the user feels it** (`perf_start` on Firecracker, `int-16589e55-perf_start-fc.log`): first start 470 ms;
  a new task p50 522 ms, p99 1130 ms; a returning task p50 392 ms, p99 952 ms; an upload to a new task p50 437 ms, p99
  850 ms. The owner's 3 s is met in each. The log also holds `docker inspect` errors for containers that were already
  gone; they are the harness reading sandboxes the gateway had removed, and no measurement depended on them.
- **The previews stack suite on the microVM backend (WEB-E2E-032)**: the suite itself passed (1 of 1), then the
  harness's clean-up check failed: it lists the images under test without the VM host's, so it refused `tqa-vmm`'s
  container as "not an image under test" (`int-16589e55-demo-previews-fc.log`). A harness fix, not a product failure
 .
- **The coverage step** failed 34 tests, mostly microVM ones that pass in their own steps above: the coverage step's
  environment, not the product.
- **The Docker sandbox suite** passed 198 of 199. The one failure is RT-I-016, a git push answered `410 Gone`: the Git
  credential routes are read-only by design since `999a7fe3` (G8a), so the test exercised a retired path. Docker-only,
  outside this gate.

**The TEST-PLAN §19a entries for this gate, against 14b** (for the technical lead to close):
1. G-13-full under load: passed (`G-13-full`). Can close.
2. The posture and full-workspace tests cut at the 30 s Docker API bound: not seen; the Firecracker step passed 106 of
   106 under the integration's load. Can close.
3. G-13-lifetime / SBX-F-036: passed with `tqa-vmm` rebuilt. Can close.
4. PERF-011's redacted retry (closed with SBX-F-035): `perf_start` on Firecracker passed in the first pass, and its
   stack start-up had no engine timeout or retry. Nothing to reopen.

**What is left before the gate can be reached:**
1. The four §19a entries closed (above), and the previews harness fix (the clean-up check's image list), so that
   WEB-E2E-032's microVM run is green as a whole.
2. The technical lead's evaluation on 14b, recorded on this page.

### Evaluation on c14b-rc (draft for the technical lead, 7 Oct 2026: to review, complete and sign after the soak)
**Proposed verdict: not reached yet.** Every row in the gate's scope passed on 14b's integration, and c14b-rc runs the
same gateway, runtime, Workstation and VM host images (only its web image differs, and no Gate 8 row runs the web).
What is left is the closing of the four §19a entries and the previews harness fix (above), and a decision on two
microVM findings from 14b's other runs (below). If the lead closes the entries and accepts the findings as outside
the rows, the proposed verdict is **Gate 8 is reached on c14b-rc**.

Checked row by row against the integration's logs (`int-16589e55-*`), each figure in the table above read again from
its log:

| Rows | Log | Found |
|---|---|---|
| SBX-C and SBX-F rows in the Firecracker step | `fc` | 106 run, 106 passed (1 slow), no failure |
| G-13 | `kvm` | 18 gates, each `PASS G-13-…` (unit, images-unit, host, kvm-acl, suites, images, stack, oom, start-failure, egress, escape, full, destroy, lifetime, reconcile, reboot, history, demo), "All gates passed"; G-13-suites 95 of 95; G-13-unit ran 108 tests |
| SBX-F-005, SBX-F-010 | `rootfs` | 12 tests, OK |
| SBX-F-006 (E2E-VM-012) | `gate8` | the killed VM's run resumed in a new VM on the same image and completed, the completed call's file survived, recovery 1 s; start-up 0 engine timeouts, 0 retries |
| E2E-VM-013 | `e2e-vmfiles` | files, uploads, attachments, run changes and fork from the guest; upload with VM start 470 ms, message and run 532 and 525 ms |
| SBX-F-016, SEC-004, SEC-005, SEC-014's gateway side | `unit`, `ignored` | 2,323 of 2,323 and 2,540 of 2,540, no failure |
| PERF-011 and PRD M3 | `perf_start-fc` | first start 470 ms; new task p50 522, p99 1130 ms; returning p50 392, p99 952 ms; upload p50 437, p99 850 ms; the owner's 3 s met in each |
| Outside the rows | `demo-previews-fc`, `cov-all`, `sandbox` | WEB-E2E-032 1 passed, then the image guard refused the VM host's `c8a54cf8` containers (a harness fix); coverage 34 failures in its own environment; the Docker suite 198 of 199, RT-I-016 the retired git relay |

**Two microVM findings outside the rows, for the lead to weigh** (both from 14b's other runs, both in c14b-rc):
- **A VM host crashed at start, 1 of 31 starts** (Gate 9's PERF-011 run on 14b): `tqa-vmm` exited with SIGSEGV about
  0.2 s after starting the microVM, before the task's runtime registered; the gateway said so and the upload got
  `409`. The host log wasn't captured. c14b-rc's release notes list it as a P1 known issue; the next release starts
  such a VM host once more (GW-I-227). No Gate 8 row failed on it, but SBX-F-020 and the start rows ran fewer starts
  than that run.
- **The microVM's workspace and state drives used Firecracker's default `Unsafe` cache**, so a task's `fsync` isn't
  durable across a host crash; `Writeback` was decided on 6 Oct for the next release. No Gate 8 row tests durability
  across a host crash.

**Confirmed before the lead signs** (by the microVM work's owner, 7 Oct 2026):
1. **The SIGSEGV: not reproduced, no host log, outside the gate.**
   - **Not reproduced.** Since 5 Oct the only microVM starts on record are two (PERF-012's microVM run and the Gate 9
     redo's step 2), both clean. The repro plan (about 500 starts through the product and 500 of the bare VM host)
     needs a quiet KVM slot and hasn't run.
   - **What the code says:** 139 is a bare signal death. Firecracker's own handlers would have exited 148 to 157, so
     the fault was before them: in the launcher's network setup (`ip`, `nft`), `setpriv`, or Firecracker's first
     moments.
   - **c14b-rc can't say more.** Its gateway logs no VM host status or lines. Dev has both since 6 Oct (`697872f4`,
     not in c14b-rc):
     - SBX-F-038 logs the exit status in words and the VM host's last 20 lines, and the launcher names each step;
     - GW-I-227 starts such a VM host once more.
   - **The rows:** no Gate 8 row failed on it. SBX-F-020 and the start rows assert that a start works and how fast,
     not a crash rate, and they passed. So it stays outside the gate, as c14b-rc's P1 known issue.
   - **For c15:** SBX-F-038 and GW-I-227 are the rows to add to this gate's scope, and the repro runs on c15's
     images after its soak.
2. **The `Unsafe` cache: no row in scope assumes durable writes; a known issue of c14b-rc, outside the gate.**
   `Unsafe` loses only what the host kernel still holds: a guest `fsync` reaches the host's page cache, not the disk.
   So data is lost only in a host kernel crash or power loss, and nothing in Gate 8 does that.
   - **SBX-F-007's reboot** (`contract.py reboot`) stops the gateway and every VM host
     (`gateway_restart(stop_vms=True)`). The host kernel and its page cache stay up, so the writes survive under any
     cache mode.
   - **SBX-F-006's VM crash** kills the VM process: the same holds.
   - **SBX-C-015's full workspace:** the `ENOSPC` comes from the guest's ext4 on a fixed-size image, whatever the
     host caching.
   - **For c15:** its drives use `Writeback` (SBX-F-039, `a157506e`, not in c14b-rc), so a guest flush becomes a host
     `fsync`. No row tests a host crash itself.
3. **The VM host in c14b-rc is 14b's rebuilt `c8a54cf8`: confirmed.**
   - `candidate14b-rc-tags.txt` has `TQA_VMM_IMAGE` `sha256:c8a54cf8f5ea…`,
     the same ID as 14b's `tqa-vmm:16589e55` in `candidate14b-tags.txt`.
   - On the host that image carries the tags `tqa-vmm:16589e55` and `tqa-vmm:c14b-rc`, built at 20:55 on 4 Oct.
   - The previews log (`int-16589e55-demo-previews-fc.log`) shows the same ID.
4. **PERF-012's microVM half: no evidence for or against this gate.**
   - **c14b-rc's measurement (5 Oct) is invalid.** The old driver let the guest read from the host's page cache, and
     its `fsync` never reached the disk (seq write 2.08, seq read 3.48, rand read 2.64 of host).
   - **The fixed driver** flushes and evicts the workspace image between the guest's phases (`a157506e`, in dev). It
     hasn't been measured yet.
   - **Plan (6 Oct):** it is measured on c15 itself, with Writeback drives, in its own run in c15's
     Gate 9 window.
   - **Rand write at 0.087** was already conservative under the old driver, so it is probably a real cost (queue
     depth 1, `Sync` engine). It is re-read then.
   - PERF-012 stays FR-29's and Gate 9's row, outside this gate.

## Since candidate 13h (on dev, 4 Oct 2026; proven on 14b, above)
**Source:** dev at `e9bd390c` (4 Oct 2026) against 13h, read from the merges and TEST-PLAN; one KVM run off a
candidate. Nothing here is a candidate result: each change is proven by the next candidate's runs (below).

| On dev since 13h | Rows | What it changes for this gate |
|---|---|---|
| A microVM's container creates get the create bound, and start-up an 80 s budget | SBX-F-032 | the cause of four of 13h's first-pass failures (the demo, `G-13-oom`, `G-13-lifetime`, `G-13-reconcile`) and half of a fifth |
| A retry removes its own late-created VM host; a create still in flight is waited for, not counted | SBX-F-033, SBX-F-035 | the likely cause of `perf_start`'s redacted retry (PERF-011 §19a entry, closed with SBX-F-035 as its regression test) |
| Sandbox creates share one start budget with same-name retries; one VM-host helper image; a stray sweep | SBX-F-029, SBX-F-031 | a new microVM task p50 692 → 552 ms (the merge's measurement, not a candidate's) |
| microVM test stacks size the workspace by its quota (`workspace_bytes` is ignored since SBX-F-025) | G-13-full | the test-only cause of `G-13-full`'s failure; its §19a entry closes when it passes under load |
| Test images no longer pile up: the probe cache keeps one image per variant, ext-check images are removed | SBX-F-034, G-13-images | `check.sh --kvm` fails when a run leaves test images behind (a new gate, 19 in all) |
| The rootfs build reads `mke2fs`'s version whole, not through `head` under `pipefail` | SBX-F-036 | the cause of `G-13-lifetime`'s failure in a 3 Oct KVM run (below); effective once `tqa-vmm` is rebuilt |

In review, not yet on dev: a microVM engine request's failure warning names its target and the redacted answer
(SBX-F-037; ENGINEERING-GUARDRAILS §1.5, the finding in PERF-011's §19a entry).

**One KVM run off a candidate** (3 Oct 2026, `check.sh --kvm` from the test-image clean-up branch with dev merged, on
the `5ba25238` images): 18 of 19 gates passed, G-13-images and G-13-suites (93 of 93) among them. `G-13-lifetime`
failed in its rootfs build: `docker run` exited 141 with nothing on stderr, the SIGPIPE that SBX-F-036 reproduces and
fixes (TEST-PLAN §19a).

**Open before the gate can be reached** (the technical lead's decisions of 4 Oct 2026 in brackets):
1. The §19a entries for this gate: G-13-full (fix on dev; closes when G-13-full passes under load), the posture and
   full-workspace cut at the 30 s Docker API bound (the warning now names its request; closes if the next candidate's
   loaded runs don't show it), G-13-lifetime / SBX-F-036 (closes when it passes with `tqa-vmm` rebuilt), and PERF-011's
   redacted retry (closed with SBX-F-035, with SBX-F-037 for its finding).
2. Scope: the gate's TEST-PLAN §17 row gains the microVM rows among SBX-F-023…036 (budgets, retries, clean-up, the
   rootfs build); Docker-only rows stay out [decided].
3. PRD M3 as the user feels it: measured on the next candidate with `perf_start`'s user view, verdict then [decided].
4. The technical lead's evaluation, after the next candidate's runs [decided].

**Runs the next candidate needs** (all images from one commit, `tqa-vmm` rebuilt; a step failed under the
integration's load gets a quiet rerun, as on 13h):
- `check.sh --kvm`: the whole G-13, 19 gates (G-13-images, start-failure, egress, escape, history and the demo
  included);
- the Firecracker step (`firecracker_kvm`, `firecracker_contract`, `firecracker_guest`), which now carries
  SBX-F-023…036;
- the rootfs build tests (`images/rootfs/test_build_rootfs.py`, SBX-F-005/010; they need the new `tqa-vmm`);
- `tests/e2e/gate8.py` (the demo, E2E-VM-012) and `tests/e2e/vm_files.py` (E2E-VM-013);
- the previews stack suite on the microVM backend (WEB-E2E-032);
- `perf_start` on Firecracker (PERF-011's user view and PRD M3; it also measures the 552 ms of the start budget's
  merge) and PERF-011's cold-boot samples;
- the gateway-side security rows (SEC-004, SEC-005, SEC-014's gateway side) in the nextest steps.

## Candidate 13h (1 to 2 Oct 2026)
**Source:** the technical lead's integration run on `5b79a932` (1 Oct 18:30–23:56 +08:00, logs `int-5b79a932-*`), and
the quiet-window reruns of its load-failed steps (2 Oct 02:02–02:37 +08:00, host load 3.91 and 2.29 at the start). The
reruns ran from `release/13h-specs` (`ae236763`: `5b79a932` plus the stack browser specs' start waits, WEB-U-189, and
its TEST-PLAN row; no product code) with 13h's images (gateway `sha256:c159fabc1f73`, Workstation runtime
`sha256:e2a3d2683807`). Each row was checked against the logs: a row passes only if each of its test IDs ran and none
failed.

| Rows | 13h on Firecracker | Evidence |
|---|---|---|
| SBX-C-001…004, SBX-C-009…012, SBX-C-014 | pass | the integration's Firecracker step (85 run, 83 passed) and the rerun of it (85 run, 85 passed) |
| SBX-C-005/006/007/016 posture, SBX-C-015 a full workspace | pass on the rerun | both failed in the integration's Firecracker step under load (below); both passed in the rerun of that step |
| SBX-C-008 reconciliation, SBX-C-013 destroy ends an active process | pass (G-13 rerun) | `G-13-reconcile`, `G-13-destroy` |
| G-13's OOM, lifetime and full cases (SBX-C-003, GW-I-066, SBX-C-015) | pass (G-13 rerun) | `G-13-oom`, `G-13-lifetime` (retired 181 s in, cap 60 s + grace 120 s), `G-13-full` |
| SBX-F-001…004, SBX-F-007, SBX-F-009 | pass | the Firecracker step (both runs); SBX-F-007 also as `G-13-reboot` |
| SBX-F-005 rootfs lineage, SBX-F-010 rootfs build | pass | the integration's rootfs step: `images/rootfs/test_build_rootfs.py`, 12 tests OK (first run on a candidate) |
| SBX-F-006 VM crash (the demo, E2E-VM-012) | pass on the rerun | `tests/e2e/gate8.py` and `G-13-demo`: the killed VM's run resumed in a new VM on the same image and completed, the completed call's file survived, recovery 1 s |
| SBX-F-011, SBX-F-013, SBX-F-014, SBX-F-016…018, SBX-F-022 | pass | unit and Firecracker steps |
| SBX-F-012 host setup, SBX-F-020 the product on the microVM | pass | `check.sh`'s Python suites; SBX-F-020 also as `G-13-stack` |
| E2E-VM-013 workspace files on the microVM | pass | the integration's E2E-VM-013 step (exit 0): files, uploads, attachments, run changes and fork answered from the runtime in the guest |
| SEC-001, SEC-006 (`egress`), SEC-002 and SEC-014's sandbox side (`escape`) on Firecracker | pass (G-13 rerun) | `G-13-egress` (every bypass failed), `G-13-escape` (18 escapes refused with typed codes); SEC-017's stack half also passed (`G-13-history`) |
| SEC-004, SEC-005, SEC-014's gateway side | pass, run once | gateway-side rows, run once on either backend (TEST-PLAN, the security rows' backend note); they passed in the integration's nextest steps |
| PERF-011 cold boot to dial | pass | five samples each: 209–240 ms in the integration, 182–198 ms (G-13 rerun) and 170–210 ms (Firecracker rerun); target ≤ 2 s |
| G-13 (`tools/check.sh --kvm`) | pass on the rerun (16 of 16) | the integration's run passed 12 of 16 (below) |

Outside this row: the previews stack suite (WEB-E2E-032) passed on the microVM backend in the integration, and the
user-view PERF-011 harness (`perf_start` on Firecracker) passed on the rerun: first start 628 ms; a new task p50
674 ms, p99 724 ms; a returning task p50 401 ms, p99 455 ms; an upload to a new task p50 626 ms, p99 722 ms (the
owner's 3 s met in each). The Docker sandbox suite passed 192 of 192 on the same rerun.

**The integration's first-pass failures, all passed on the quiet reruns.** The integration ran on a loaded host (its
perf steps began at load 24.35). Each failure's cause, from its logs:

| Step | Failure | Cause |
|---|---|---|
| `gate8.py` (the demo) | "the second tool call never started" | **explained:** the replacement VM's block image was never made. All three engine attempts were cut at the 10 s operation timeout, then `sandbox create failed: deadline exceeded` |
| `G-13-oom` | `compose up --wait` failed: the gateway exited at start-up | **explained:** five start-up attempts, each Docker API request cut at 10 s, then `sandbox configure failed: deadline exceeded` |
| `G-13-lifetime` | the run did not end within 430 s | **explained:** the retired sandbox's replacement could not make its block image (three attempts cut at 10 s), then `sandbox create failed: deadline exceeded` |
| `G-13-reconcile` | no `run.completed` within 180 s, no stored events | **explained:** the gateway's start-up survived three 10 s cuts, then the first start's block image failed the same way |
| `perf_start` on Firecracker | no `run.started` within 60 s | **in part:** the VM host's first create was cut at 10 s (explained). Its retry failed at once with a redacted `backend operation failed` (not explained; TEST-PLAN §19a) |
| `G-13-full` | the fill command ended `timeout: tool deadline elapsed`, so the workspace never filled | **found, test-only:** the harness still sets the old `workspace_bytes` (64 MiB), ignored since `0f862d81`; the image takes the 10 GiB quota, and a ~10.5 GiB fill outlasts the deadline under load. Fix in review (TEST-PLAN §19a) |
| the Firecracker step: posture, full-workspace tests | a Docker API request cut at the tests' 30 s bound (the posture probe itself finished clean) | **likely the same class, not confirmed:** the log does not name the request (TEST-PLAN §19a) |

"Explained" means the failure matches SBX-F-032: on the microVM backend, the VM host's creates, image helpers and
start-up probes were bounded by the gateway's 10 s operation timeout while one create took 44 s under load. The fix
gives them the create bound (60 s) within the start's budget, and gives the gateway's start-up an 80 s budget. The fix and
the SBX-F-032 row are on the Docker start-up budget branch, not yet in dev, so 13h does not carry them.

**What was left at 13h** (superseded by "Since candidate 13h" above):
1. SBX-F-032 merged (the four explained failures and half of the fifth), the G-13-full harness fix merged, and the
   §19a entries above closed.
2. PRD M3 as the user feels it: the user-view numbers above pass the owner's 3 s. Whether M3 belongs to this gate's
   evaluation is the technical lead's call (DEV-PLAN §4a).
3. The technical lead's evaluation on 13h, recorded on this page.

## Candidate 13g (1 Oct 2026)
**Source:** the technical lead's integration run on `86bdd6ee` (1 Oct 03:59–07:32 +08:00, logs `int-86bdd6ee-*`): its
Firecracker step (77 run, 77 passed, 04:15–04:27), unit and ignored steps, `check.sh`'s Python suites, the E2E-VM-013
script and the previews stack suite on the microVM; and the TEST-PLAN audit of it (1 Oct 2026).

| Rows | 13g on Firecracker | Evidence |
|---|---|---|
| SBX-C-001…007, SBX-C-009…011, SBX-C-014…016 | pass (13) | the Firecracker step (SBX-C-011 inside SBX-F-009's density test; SBX-C-005/006/007/016 in one posture test) |
| SBX-C-012 | pass (P2) | the Firecracker step (the guest clock) |
| SBX-C-008 reconciliation, SBX-C-013 destroy ends an active process | **not run on Firecracker** | passed on Docker only; on Firecracker they run in G-13's contract (`contract.py`), which 13g didn't run |
| SBX-F-001…004, SBX-F-007, SBX-F-009 | pass (6) | the Firecracker step. SBX-F-004's tests carry its ID second in their names (`sbx_c_001_sbx_f_004_…`, `sbx_c_004_sbx_f_004_…`), so the audit, which reads the first, listed it as not run |
| SBX-F-011, SBX-F-013, SBX-F-014, SBX-F-016…018, SBX-F-022 | pass (7) | unit and Firecracker steps |
| SBX-F-012 host setup, SBX-F-020 the product on the microVM | pass (2) | `check.sh`'s Python suites (suite summaries) |
| SBX-F-005 rootfs lineage, SBX-F-010 rootfs build | **not run** | `images/rootfs/test_build_rootfs.py` is run neither by `check.sh` nor by the integration |
| SBX-F-006 VM crash (the demo, E2E-VM-012) | **not run** | `tests/e2e/gate8.py` runs under `check.sh --kvm` only |
| E2E-VM-013 workspace files on the microVM | **fail** on 13g, **fixed since** | HTTP 409: the built-in Workstation took the default on any backend and was then refused on the microVM. Fixed in `b36a0bde` (merged `36f61b47`; GW-I-172, GW-I-174) |
| SEC-001, SEC-002, SEC-004…006, SEC-014 on Firecracker | **not run on Firecracker** | all pass on Docker on 13g; their Firecracker runs are G-13's |
| PERF-011 cold boot to dial | **pass** | p50 187 ms, p99 429 ms (target ≤ 2 s) |
| G-13 (`tools/check.sh --kvm`) | **not run** | 13g ran `G-13-unit` only |

Outside this row, the previews stack suite (WEB-E2E-032) passed on the microVM backend on 13g.

**What is missing before the gate can be reached:**
1. G-13 (`tools/check.sh --kvm`) on the candidate: the Firecracker halves of SBX-C-008/013 and SEC-001…006/014, the
   lifetime and full cases, and the demo `tests/e2e/gate8.py` (SBX-F-006 = E2E-VM-012).
2. `images/rootfs/test_build_rootfs.py` (SBX-F-005, SBX-F-010) on the candidate, and added to `check.sh` or the
   integration.
3. E2E-VM-013 green on a candidate that carries `36f61b47` (13h).
4. PRD M3 as the user feels it (a task's first message to its run starting, on the microVM): the PERF-011
   user-view harness measured p50 673 ms and p99 1321 ms for a new task on dev on 1 Oct (a trial, not a candidate).
   The gate's PERF-011 row is the backend boot above; whether M3 is part of this gate's evaluation is the technical
   lead's call (DEV-PLAN §4a).
5. The technical lead's evaluation on that candidate, recorded on this page.

## History (26 to 28 Sep 2026)

### What was independently verified
- Runs on real microVMs, on a Linux KVM host with a rootless container engine, through the automated Firecracker stack
  (`deploy/firecracker/stack.py`, `contract.py`). It starts the gateway, the fake model and a root filesystem built
  from the runtime image.
- The demo, SEC-001/002/006, SBX-C-003/008/013 and SBX-F-006/007 ran end to end through the public API.
- The full contract (`contract.py full`) passed 3/3 with the typed out-of-space event.

### Result
- **All 33 IDs pass** after the evening update: SBX-C-015's typed event (`workspace.full`) landed and passed on KVM.
- **The demo passes:** a microVM was killed during a tool call, and the run resumed in a new VM on the same workspace
  image. The marker written before the kill was read back, and the event stream stayed contiguous. Recovery took about
  1 s.
- **Defects found and fixed along the way, each with a test:**
  - workspace writes lost on a VM crash (the guest page cache): fixed by syncing before each tool result (RT-I-024);
  - the guest OOM killer had nothing to kill;
  - root filesystem verification timed out on large images;
  - `destroy` could leave a VM host behind under load;
  - an image with a baked-in `ENV` secret reached a microVM where Docker refuses it (SBX-C-014).

### Measurements
**Environment:** one Linux KVM workstation; microVMs of 256 MiB plus 64 MiB overhead.

| ID | Result | Target |
|---|---|---|
| PERF-011 cold boot | p50 136 ms, max 364 ms over 31 boots | ≤ 2 s |
| PERF-013 flush per tool result | about +0.25 ms (small write); syncfs after a 64 MiB write p50 85 ms, p95 143 ms | within the 60 s sync bound |
| SBX-F-009 density | 16 VMs running in 8.7 s, 1.27 GiB of a 5 GiB budget; unique names and addresses; nothing left after destroy | row target |

### Limits of the acceptance
- **G-13 is local only for v1**, run in the technical lead's integration run (Product Owner decision, 2026-09-27):
  the Firecracker and microVM checks need a KVM host, and v1 has no self-hosted CI runner.
- **Scope restatement:** SBX-C-015's in-run out-of-space clause is Firecracker-only. On Docker the quota is enforced
  between runs (GW-I-030).
- **Open items** (their state on 2026-09-27 is in the last section):
  - the `max_lifetime` cap (GW-I-066), first listed as a divergence between the backends: none found. Rule 1 lives in
    the gateway's manager for every backend; the gap was between the rule and the code, fixed in `54e718e`. What
    was missing is a run of the cap on each real backend: done on 2026-09-28, both green (below);
  - the secret-file limit of the image policy: narrowed since (below);
  - one gateway start-up exit, unreproduced at first; a cause that fits it is found and fixed since (below).
- **A root filesystem rebuild is required** when upgrading: the manifest schema changed, and protocol revision 8 raises
  the runtime floor. Done for the revision 8 rerun below.

### Update: the revision 8 rerun (2026-09-27)
**Environment:** as above, with the revision 8 images (runtime, VM host and gateway built from the same commit). The
whole Firecracker set (every SBX-F test plus the contract, density, guest and state-mount suites) ran one test at a time.

- **71 of 72 pass.** No root filesystem build was refused.
- **The one failure was SBX-F-009 (density):** one of 16 VMs had not reached its workload after 3 minutes, at a load
  average of about 28. It passed when run again alone (all 16 running in 6 to 16 s in later runs). It has not recurred.
  The check now describes a VM that misses it (its container's state, exit code and last log lines), so a recurrence
  shows whether the VM exited or was still booting.
- **SBX-C-014 (a derived template keeps the posture) times out intermittently under heavy load**, at about 30 s,
  before the VM host is created. It is not the root filesystem check, whose bound scales with the 2.1 GB image. The
  backend now logs which step hit its bound (a file step with its bound, or a container-engine request), so the next
  failure names it. It passes at ordinary load.

**Fixed along the way, each with a test:**
- **Short-lived helpers could outlive the backend** (SBX-F-024): a gateway stopped between creating a helper and
  removing it left the container behind, unlabelled, so nothing swept it. Helpers now carry the instance's labels; a
  stack teardown removes them, and the backend removes its own leftovers at start. The start-up probes were a third
  leak path, found by the new test.
- **A workspace image now holds its quota** (SBX-F-025): new images are sized from the workspace quota plus the file
  system's metadata (5%, at least 128 MiB), made without a root reserve, which the guest user could never use. Measured:
  with the old defaults a 1 GiB quota left 981 MiB usable; now a 1 GiB quota gets a 1152 MiB image and at least 1 GiB
  usable. Existing images keep their size. `[firecracker] workspace_bytes` is deprecated.
- **The root filesystem check's refusals were unreadable:** only the checker's version line was kept. The builder now
  keeps the checker's whole report beside its output and names its exit code; the builder's container errors carry
  theirs. An intermittent refusal seen three times under load has not recurred since; the kept report will name it.
- **Test probe images were rebuilt on every run**, because image builds here are not reproducible, so every run rebuilt
  their root filesystems. They are now keyed by their inputs and built once.

**Open, for the flake list:** SBX-C-014 under heavy load (above); an intermittent root filesystem build failure under
load, now diagnosable. Heavy concurrency on the shared test host stalls the container engine: the Docker-backed suites
run with bounded parallelism.

### Update: integration record and fixes (evening of 2026-09-27)
The team's integration run executes the whole Firecracker set as one step, on the test host, after the other suites.
Its result at each integration commit of the day:

| Commit | Firecracker step | Failures |
|---|---|---|
| `7d019d1` | 68 of 70 | SBX-F-001 (unreadable artefact, below); SBX-C-003 after 60 s |
| `d2cc5e5` | 71 of 73 | SBX-F-001; the posture contract (SBX-C-005/006/007/016) after 93 s |
| `7e85ee0` | 73 of 75 | SBX-F-001; SBX-F-023 after 116 s |
| `dd97549` | 74 of 75 | SBX-F-001 |
| `e34110a` | 75 of 75 | none |
| `61b0668` | 75 of 75 | none |

- **SBX-F-001 fixed for root.** "An artefact that cannot be read is refused" failed in every integration run: its
  unreadable kernel was a mode-000 file, which root, including root in the user namespace the suites run in, reads
  anyway. The test now also places a socket at the kernel's path, which no user can open, and keeps the mode-000 case
  only where the process cannot open such a file. It passes as root in the user namespace and as an ordinary user.
- **The load-related failures have a named cause.** SBX-C-003, the posture contract and SBX-F-023 failed once each
  after 60-116 s and passed at the next commits; with SBX-C-014's intermittent 30 s timeouts and the one density miss
  of the revision 8 rerun, they are the class Gate 6 traced (docs/evals/gate-6.md): when the host's disk is saturated,
  the container engine starts containers late, by up to about 30 s, while neither its daemon nor containerd is blocked
  on anything. Measured there with container starts every 0.5 s: 0.2 s at most on a quiet host, up to 31 s with the
  disk's I/O "full" pressure at about 54 %. Since then:
  - the stack runner waits, inside its slot, until the disk's I/O "full" pressure is at most 10 % (15 minutes at most),
    and says when it waited;
  - a stack step that times out prints the engine's goroutine stacks and the host's I/O pressure, so a recurrence is
    measured, not guessed;
  - the Docker test fixture now survives a container the engine creates after the client gave up, and reports a
    failed setup's own error before any cleanup error (seen once at `e34110a`, outside this set).
- **The secret-file limit is narrowed.** A template whose own layers hold a credential file of a known shape (private
  keys, SSH keys, Git, AWS, Docker, npm, PyPI and `.netrc` credentials) is now refused by the template check
  (`image_secret_file`, ADR-0032), on both backends and before any root filesystem is built, unless the install file
  acknowledges the path. SBX-C-014 is restated to say so; a secret in any other shape is still not detectable.
- **Start-up exits name their step** since `cc89619` (GW-U-060): a start-up step that times out is named in the error
  the gateway exits with. The unreproduced exit has not recurred.

### Update: KVM access during density runs (2026-09-28)
**What failed.** In candidate 5's integration run (`545c3d0`, 2026-09-28 08:21), the Firecracker step passed 74 of 75:
SBX-F-009 found one of 16 VM hosts exited. The check's new record showed exit 1 within 120 ms, no OOM kill and a host
that was not low on memory. The container's last log lines held Firecracker's own error: `Error creating KVM object:
Permission denied (os error 13)`. The rerun passed.

**Cause.** The host's login manager resets `/dev/kvm`'s ACL on every change event of the device, and KVM sends one for
each VM created or destroyed, so a density run triggers many. The reset (the `uaccess` builtin) deletes every user ACL
entry and never touches group entries. The VMM was granted through a user entry only, and rule `74-tqa-kvm-acl.rules`
re-granted it in the same event but after the reset. A VM that opened `/dev/kvm` in that gap of a few milliseconds was
refused.

**Fix** (`0e8d0dd`, merged in `1cfc42d`). The VMM's group is granted too: the container's gid maps to a subordinate
gid the same way as its uid. `host-setup.sh` grants `u:<uid>:rw,g:<gid>:rw`, its `--check` requires the group entry and
`--revert` removes both; `kvm-no-uaccess.sh` re-grants either form. The group entry survives every reset, so no VM can
start inside a gap; the re-grant rule and the 30-second timer remain as safety nets. The tests cover a host with only
the user entry and one without a subordinate gid range. ADR-0006 point 3 is amended.

**Host state.** Both scripts applied with root on 2026-09-28 at 09:07 and recorded in the host's change log. Both
`--check` runs are clean, `/dev/kvm` carries the user and the group entry, and it still did after the density runs below.

**Density after the fix.** SBX-F-009 three times in a row on `1cfc42d` with candidate 5's images, through the test host's
one-slot stack queue:

| Run | Time | Result |
|---|---|---|
| 1 | 09:17–09:21 | pass, 48.5 s |
| 2 | 09:21–09:24 | pass, 47.3 s |
| 3 | 09:24–09:28 | pass, 44.6 s |

**What this explains, and what it doesn't.** This is a second cause of SBX-F-009 misses, beside the late container
starts under disk saturation traced for Gate 6: a VM that *exits* at start with `Permission denied` was this race, and a
VM that is *late* was the engine. The revision 8 rerun's miss had no record, so it can't be assigned to either; the
check's record now tells them apart. The gateway start-up exit is a different failure (the gateway container exited
before any VM existed); its cause is in the next section.

### Update: the gateway start-up exit's cause (2026-09-28)
**Recurrence:** a G-13 contract run on a loaded host (load average 25) stopped before its case, with
`application not healthy after 2m0s`. The gateway container had started; the database, the web application and the
fake model were healthy.

**Cause:** the microVM backend's connect at start-up (the host preflight, the helper sweep and the storage helper check)
bounds each engine call by `io_timeout` (10 s). The only line the gateway logged was the warning that one Docker API
request for the backend had timed out. That one timeout failed the `sandbox backend` start-up step, and the gateway
exited. Nothing retried it, and the compose files set no restart policy, so the gateway stayed down. The exit was
silent because only a timeout of the step's own was logged at ERROR, and this one arrived as a sandbox error. The
first exit left no reason in its logs either, which fits this path. It isn't proven to be the same event, but no other
path found exits without an ERROR line.

**Fix (GW-U-071):**
- Every failed start-up step is now logged at ERROR with its name and cause.
- The backend's connect is retried on a timeout only, waiting 1, 2, 4, 8 and then 10 s, and starts no attempt after
  60 s. That is inside the gateway image's 90 s health check window.
- A preflight refusal or any other error still stops start-up at once.

`deploy/README.md` states the deadline. Whether the compose files should restart the gateway is a separate question.

**The two failed starts, with the new diagnostics.** Both were on the candidate 5 images, before the fix, at a load
average of about 21 to 25:
- **One proves this path.** The gateway started at 02:54:04 UTC and the backend's Docker API request timed out at
  02:54:15. The container exited with code 1 at 02:54:15, and its last line was `sandbox configure failed: deadline
  exceeded`. The storage helper was left created but never started: its create took longer than the bound.
- **The other is still unexplained.** When `compose up --wait` gave up, the gateway had logged the same timeout warning
  but had not exited. It was running and unhealthy. That run printed neither the gateway's health checks nor its
  threads; failed starts print both now.

**The fix under load (2026-09-28 11:04 to 11:08, load average 24 to 40).**
- **Setup:**
  - three G-13 destroy runs back to back, cases at `cffd03c`;
  - a gateway built from `04a767c`, which has the fix (`sha256:da860e583249`);
  - candidate 5's runtime, web, fake model and microVM host images.
- **Result:** all three came up and passed. Each start printed `0 engine timeout(s), 0 retry(ies)`, so the retry was
  not exercised: no engine call timed out during these starts. GW-U-071 shows the retry logic; a run on a real engine
  has not yet made it retry. Every started stack now prints its count of engine timeouts and retries, so the next
  timeout at start-up will show whether it held.
- **The destroy case:** all three removals met the accepted race (`removal of container … is already in progress`) and
  passed.

### Update: G-13-full and the VM host's memory (2026-09-28)
**Candidate 6's G-13-full failure is not a code regression.** A recorder added to the Firecracker cases (`stack.py`)
keeps the engine's events. They show the VM host container of the VM filling its workspace being OOM-killed
(`oom`, then exit 137), and the gateway then replacing the lost VM, as designed. The candidate 5 images, which passed
the case that morning, failed it the same way that evening, and nothing on the guest or VM host side changed between
the two candidates.

**Mechanism.** A VM host's memory limit is the sandbox's memory plus a fixed overhead, 64 MiB up to candidate 6,
without swap. The guest's disk writes go through the host page cache, which the same cgroup is charged for. Since
workspace images are sized from the quota (SBX-F-025), the case's fill writes far more than the overhead. Measured with
the VM host's `memory.stat` every 0.1 s during the fill, on a 512 MiB guest:

| Overhead | Least clean page cache left at the limit | Dirty page cache (peak) | Outcome |
|---|---|---|---|
| 64 MiB (the default until now) | none: the cgroup sat at its limit | 129 MiB | VMM OOM-killed in 4 of the 6 runs that day |
| 256 MiB | 17 MiB | 236 MiB | passed, still at the edge |
| 512 MiB | 228 MiB | 282 MiB | passed |
| 1 GiB | about 750 MiB | 263 MiB | passed |

The larger overheads were tested by raising a running VM host's limit (`docker update`), not by a build.

**Can a heavy-writing task OOM its VMM on the shipped default? Yes, up to candidate 6.** With the 64 MiB default, any
task that writes a few hundred MiB quickly (a build, an unpack, a large download to the workspace) can have its VMM
killed when the host's writeback lags. The task then sees its sandbox stop, and the run resumes in a new VM. The fix
makes the overhead configurable (`[firecracker] vmm_overhead_bytes`, 64 MiB through 8 GiB) with a default of 512 MiB.
In the same fill that left 228 MiB of clean cache at the limit. The limit is a bound, not a reservation, because page
cache is reclaimable host memory. No build with the new default has run G-13 yet; the next candidate's G-13-full is
that check.

### What is left before the evaluation
| Item | Owner | State |
|---|---|---|
| A confirming integration run with a clean Firecracker step at the candidate commit | technical lead | done: `0e385d1` and `deef3f2` both 75/75 in the Firecracker step, and the `deef3f2` run was green in every step (28 Sep 00:21–01:07) |
| GW-I-066 on both backends: `robustness.py lifetime` (Docker) and `contract.py lifetime` (Firecracker, gate G-13-lifetime) run one shared case, a busy sandbox retired at the cap plus the grace whose run resumes in a new sandbox; no per-backend restatement needed (the rule is backend-independent) | the gateway maintainers | done 2026-09-28 on the `deef3f2` images (gateway `sha256:4bcfc8655f3e`, runtime base `sha256:fe21cf2136e5`, microVM host `sha256:32d2b3f7468e`), cases at `c2ae4d3`: with a 60 s cap and the 120 s grace, the busy sandbox was retired 181 s in on Docker and 182 s in on Firecracker; in both, exactly one call was interrupted and the run resumed once in a new sandbox on the same workspace and completed |
| Intermittent SBX-F-009 exits: the `/dev/kvm` grant raced the login manager's resets | the Firecracker maintainers | fixed (`0e8d0dd`), host applied 2026-09-28 09:07; SBX-F-009 3 of 3 afterwards (above) |
| A full G-13 run (`tools/check.sh --kvm`) on dev after the fix | technical lead | running |
| G-13-full on candidate 6: the VM host OOM-killed during a workspace fill | the Firecracker maintainers | cause found (the 64 MiB VM host overhead, above); fixed with a 512 MiB default, configurable; G-13-full on a build with it still to come |
| The gateway start-up exit | the gateway maintainers | a cause that fits it found and fixed (GW-U-071, above); three G-13 destroy runs on the fix passed under load, but no start-up timeout occurred, so the retry is still unexercised on a real engine; a hang (running, never healthy) seen once is still unexplained |
| The upgrade note: a root filesystem rebuild is required when upgrading (manifest schema 2, protocol revision 8) | the release maintainers | done: `deploy/README.md` upgrade notes |
| The evaluation on this page | technical lead | after the items above |
