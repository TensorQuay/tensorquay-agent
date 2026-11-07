# Gate 6: Extensions (live status)

**State:** the gate is **reached**, by the technical lead's evaluation below (2026-09-27, dev `dd97549`).
The first audit was on `3c4493f` the same morning.

## Scope
The TEST-PLAN §17 Gate 6 row: TOOL-I-012/013/018; RT-I-008; GW-I-024…026, GW-I-029; CORE-U-031; SBX-C-014; SEC-011,
SEC-013; WEB-E2E-010; E2E-MCP-006, E2E-PROFILE-007. Also the Phase 6 build list and demo (DEV-PLAN §4), and PRD
FR-9, FR-22…25 and FR-28.

## How the status was established
- **Re-run by the auditor**, no containers:
  - the morning's audit on a clean export of dev with its own build directory: the row's Rust tests and the untagged
    rows covering them, database tests included, 66/66; the web tests for the connector, credential, approval and
    environment pages, 105/105;
  - at 12:10, the profile rows (GW-I-025, CORE-U-031, GW-I-085, GW-I-086, GW-U-050, RT-I-028), database tests
    included: 19/19.
- **Taken from recorded runs:** the container-backed IDs (stack drivers and end-to-end tasks) and the KVM-backed
  SBX-C-014 guest tests, each named below.

## Per ID
| ID | Status | Evidence or gap |
|---|---|---|
| TOOL-I-012 MCP over stdio | pass | 8 tests: discovery and calls, typed start failures, one restart then disabled, cancel notices, a clean server environment |
| TOOL-I-013 MCP over streamable HTTP | pass | 8 tests through the gateway proxy, plus the boundary cases added on 27 Sep (the 64-tool bound, the 1 MiB message bound, oversized SSE events) |
| TOOL-I-018 Gateway-proxied MCP | pass | stack driver green (`egress.connector_isolation`), also with the task made from a person's profile (`--profile`, 3 of 3 on 2026-09-27, the `dd97549` images): the profile alone brings the connector, and a connector outside its allow-list is refused inside the sandbox with 403 `connector_not_allowed` |
| RT-I-008 MCP start-up | pass | a failing server is disabled and reported; the reaper spares only recorded server groups |
| GW-I-024 Connectors and vault | pass | per-user before shared credentials, origin binding, deletion with the connector, sealed at rest; connector administration under GW-I-068 |
| GW-I-025 Agent profiles | pass | 5 tests (27 Sep): per-user profiles managed with versions and field errors; bounds per owner and per install; a conversation from a profile keeps its copy; administrators list the tool servers a shared profile may name |
| GW-I-026 Templates | pass | template seed, checks and administration (GW-I-075…080) and the root filesystem from the same digest (SBX-F-005); those tests carry the GW-I-026 tag |
| GW-I-029 Secret store | pass | the built-in store only (ADR-0033): authenticated encryption under an install key outside the database, rotation without restart, values never in logs or errors, failing closed |
| CORE-U-031 Profile applied | pass | 1 test (27 Sep); the profile's tools, instructions and budgets reach the run. Also GW-I-085 (a request never widens a profile) and RT-I-028 (no skills index without the skill tool) |
| SBX-C-014 Custom template posture | pass | KVM guest tests pass; the Docker image refusals (environment keys, declared volumes) carry the SBX-C-014 tag, and the template check's credential-file refusal passes on the real engine |
| SEC-011 MCP name collision | pass | an MCP tool named `shell` is namespaced; the built-in is unaffected |
| SEC-013 Connector credential isolation | pass | the credential, in every encoding, is absent from the sandbox's processes, files and reassembled traffic, with positive controls, also with the task made from a person's profile (`--profile`, 3 of 3; 6 reassembled streams each). Process memory is not read (needs ptrace; documented) |
| WEB-E2E-010 Profiles and connectors | pass | the connector half green on the real stack; the profile half green on the real stack (run 2 on `57bdb59`, merged as `574e193`) |
| E2E-MCP-006 MCP task | pass | a stdio MCP tool is called and the run completes on the real stack |
| E2E-PROFILE-007 Profile task | pass | a task under a restricted profile with one connector completes using only the allowed tools; first real run green on revision 8 (27 Sep, 08:48); now part of the lead's integration run |

**Totals:** 15 pass, 0 covered, 0 partial, 0 missing. On the morning audit it was 8 pass, 2 covered,
2 partial, 3 missing.

## Phase 6 build list
- **Built:**
  - MCP over stdio and the MCP proxy for connectors, with the connector registry;
  - agent profiles (ADR-0012): the schema, CRUD, the picker on New task (WEB-U-050), Settings → Profiles, personal
    and shared (WEB-U-049), and a task made from a profile showing what it fixes (WEB-U-051);
  - custom templates by digest (ADR-0032);
  - admin pages for connectors, templates (Environments) and profiles;
  - the extension checker `tqa-ext-check`: template posture and boot to ready on Docker (TK-I-002), running a stdio
    server in a template image (TK-I-003), and the catalogue lint (TK-U-015, gate G-09-catalog);
  - the catalogue skeleton `docs/catalog/`, with its first two entries: the words server and the Workstation template.
- **Decided on 27 Sep:**
  - the built-in secret store is the only secret store (ADR-0033); the external backend was removed;
  - everything shipped uses Apache-2.0-compatible licences (ADR-0017 amended), enforced for third-party compose
    images by gate G-05-images (SEC-030).

## Demo
An admin registers an issue-tracker MCP server with a vaulted credential; a user creates a profile that uses it; the
agent files an issue; the credential appears nowhere in the sandbox. Every step has passed in its own test
(WEB-E2E-010 both halves, E2E-PROFILE-007, SEC-013). The end-to-end run is E2E-EXT-015 (`tests/e2e/gate6.py`, over the
public API on the Docker stack with the fake model and the SEC-013 connector fixture). Its runbook and the evidence it
asserts (E1-E6) are in `tests/e2e/gate6_evidence.py`:
- E1: the administrator stores the credential in the secret store and allows the connector to use it; the admin view
  flips `key_missing` to false, and the secret's only consumer is the connector.
- E2: an invited member with no credential of their own makes a personal profile; only the profile brings the
  connector, which is off by default.
- E3 and E4: the approved call reaches the remote once, from the gateway, with the stored credential, and files the
  issue; a connector outside the task's set is refused inside the sandbox.
- E5: the credential, in every encoded form, is absent from the sandbox's traffic, processes and files (with positive
  controls), the events, the logs, the database dump and every API answer.
- E6: the secret store records the connector's read for the task.

**Result (2026-09-27):** six runs on the same images. Runs 1-3: 2 of 3 passed; run 1 failed in the stack's setup,
before the demo's first step, from an engine stall traced to host I/O saturation. Runs 4-6, after the fix: 3 of 3 passed.

Images, built from dev `dd97549`: gateway `sha256:f1bf9008b05a0129f8606840c0532eaeedfd7f7b5605b08e8860804fc611da8a`;
Workstation runtime `sha256:a453f371e8fce92fe9c618f66fbe620dcb1dc51121e5d7991fbe32b156108bd8` (protocol revision 8).
Driver: `tests/e2e/gate6.py` as merged in dev.

| Run | Start | Duration | Outcome |
|---|---|---|---|
| 1 | 18:17:45 | 103 s | **Failed in setup.** The stack's check of the seeded `gateway.toml` runs the gateway image once; its positive control (a copy with an unknown key, which must be refused) produced no output within its 30 s bound. The container engine started no container for about 28 s (its log shows the check's container created at 18:18:11 and never started), then carried on by itself. No demo step ran. |
| 2 | 18:19:28 | 47 s | Passed: E1-E6. The credential was absent from 6 reassembled TCP streams (one carrying the runtime token and the approved call) and from 17 API answers; the store recorded 4 reads by the connector for the task. |
| 3 | 18:20:15 | 46 s | Passed: E1-E6, with the same counts. |
| 4 | 18:46:25 | 23 s | Passed: E1-E6 (6 streams, 17 API answers, 4 reads). No wait for disk I/O to quieten. |
| 5 | 18:46:48 | 23 s | Passed: E1-E6, with the same counts. No wait. |
| 6 | 18:47:11 | 23 s | Passed: E1-E6, with the same counts. No wait. |

Runs 4-6 followed the cause and its fix (below): the stack runner now waits, inside its slot, until the host's disk I/O
is not saturated (`full` avg10 at most 10 %, 15 minutes at most) and says when it waited. None of the three had to.

**Run 1's engine stall, investigated.** A first start of a freshly built image was ruled out: it takes 0.15 s, the same
as later starts. A targeted repro then probed `docker run <gateway> check` every 0.5 s. During probes, stack dumps of the
engine (dockerd and containerd) and the host's I/O pressure were taken:

| Case | Probes | Longest | Host I/O "full" (avg10) |
|---|---|---|---|
| Quiet (twice, around a warm `docker system df`) | about 100 | 0.2 s | 0-5 % |
| Stacks up and down with new networks (3 cycles) | 27 | 2.0 s | not sampled |
| Small image builds with a new layer (3 cycles) | 26 | 9.5 s | 13.6 % (60 s average) |
| A cold `docker system df` (36.9 s), then builds | 27 | 31.2 s | up to 53.7 % |

Every probe over 4 s fell in a period of host I/O saturation, and none in a quiet one. The stack dumps taken during the
stalls show neither dockerd nor containerd blocked on a lock or busy with work: the time is spent waiting for I/O. The
likely mechanism is the engine's metadata commits (each container create and start writes and syncs its database) waiting
behind a saturated disk. At 18:18 the integration run's compile and tests were writing to disk. The host's I/O pressure at
that exact moment was not recorded, so run 1 itself is attributed by this mechanism, not measured. From now on the stack
helpers print the engine's stacks and the host's I/O pressure whenever such a step times out.

## Evaluation (technical lead, 2026-09-27)
**Verdict: Gate 6 is reached.** Every row of the TEST-PLAN Gate 6 set passes, and the demo passed on the images built
from dev `dd97549`.

- **Checked by the lead:**
  - the lead's full integration run on `dd97549` (18:16-18:56), on the same images as the demo: E2E-MCP-006 and
    E2E-PROFILE-007 pass, and the database, deployment-stack, runtime and web suites that carry this gate's rows pass;
  - the demo's record above: six runs, each with its images, start time and outcome. The one failure (run 1) was in
    the stack's setup, before any demo step. Its cause was investigated to a mechanism, host I/O saturation, and
    reproduced; the stack runner now waits for a quiet disk and prints the engine's state on any timeout. The three
    runs after the fix all passed.
- **Also reviewed:** the tags on GW-I-026 and SBX-C-014, and the TOOL-I-018 and SEC-013 re-runs with the task made
  from a person's profile (3 of 3).
- **Outside this gate, open in the same integration run:** a regression in how a tool call's arguments are recorded
  (optional fields recorded as null; SEC-002 and TOOL-S-017 on the real runtime) and test-isolation fixes in the
  extension checker's Docker tests. Neither is in this gate's row set. Both are being fixed, and the next green full
  integration run re-confirms this gate's rows with them.
- **Known limits, accepted:** SEC-013 does not read the sandbox's process memory (it needs ptrace; documented). Run 1's
  own I/O pressure was not recorded, so it is attributed by mechanism.

The tags (GW-I-026, SBX-C-014) are in place, and TOOL-I-018 and SEC-013 were re-run with the task made from a profile.
The secret store's read audit is built (ADR-0033; GW-I-094, GW-I-095): who read a shared secret or a person's own
credential, for which task, with what outcome. Rotating the install key, offline and on the host, is the one planned
secret-store change; it is post-v1 and needs its own Product Owner decision. The store never depends on a cloud service.
