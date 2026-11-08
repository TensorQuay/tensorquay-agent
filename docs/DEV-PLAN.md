# TensorQuay Agent: Development Plan

Status: **v0.7** (7 Nov 2026): v0.6 (21 Oct 2026) with where each phase stands and what comes next brought to releases 17.0 and 17.1 (§4a, §4b). This is the phase-by-phase half of the development
plan; the design half is [ARCHITECTURE](ARCHITECTURE.md). Requirements: [PRD](PRD.md). Tests: [TEST-PLAN](TEST-PLAN.md).
Rules: [DEV-GUIDELINES](DEV-GUIDELINES.md). v0.2 replaces the layer-by-layer phases of v0.1 with a **walking
skeleton first, then vertical iterations**: something runs end to end in the browser within the first weeks, and
every later phase adds one capability across all layers and ends in a demo.

## 1. How we work
- **Walking skeleton first.** Phase 1 builds the thinnest path from the agent loop through tools, the runtime in a
  Docker container, the gateway and the web transcript. It is small, but it is real: real protocol, real
  persistence, real sandbox, real relay. Everything after it is an iteration on a running product.
- **One capability per phase, across all layers.** A phase adds one thing (robustness, safety, team, local-model
  quality, extensions, channels, microVMs) from the core to the UI, with its tests, and ends with a demo the
  Product Owner can click through.
- **Design, then tests, then code, per phase.** Each phase starts with the contracts it touches and the TEST-PLAN
  cases it must pass; the technical lead accepts against those tests and the demo, not against a slide.
- **Never skip the foundations that are expensive to retrofit.** Even the skeleton has: the event log as the source
  of truth, sequence numbers on every frame, path confinement, secrets outside the sandbox, the relay. Everything
  else can be added later; those cannot.
- **Lean.** Build the smallest thing that passes the tests. Prefer a maintained permissive dependency to a rewrite;
  prefer deleting a feature to keeping one nobody asked for (PRD §6 non-goals).
- **Independent implementation.** Public designs are studied and cited; code is written from our contracts
  (DEV-GUIDELINES §2.5).
- **Fakes first.** The fake provider, fake sandbox backend, fake relay, fake clock and the fake model server are
  built in Phase 0 so every phase tests deterministically without a GPU, and Docker is needed only for integration.
- **One protocol, grown additively.** Protocol v1 starts with the frames the skeleton needs; later phases add frames
  and never change existing ones.
- **Docker is the default path; Firecracker is a backend.** Nothing before Phase 8 assumes a microVM; Phase 8 adds
  one behind the same interface and the same suites. It can be pulled earlier if host risk needs retiring sooner.
- **Report at every phase** with a demo and a short evaluation report in `docs/evals/` (§8).

## 2. Software stack
Exact versions are pinned in `rust-toolchain.toml`, `Cargo.lock`, `web/package.json` and lockfiles at Phase 0 and
recorded in every release. Major lines:

| Area | Choice | Why |
|---|---|---|
| Harness, runtime, gateway | Rust stable; `tokio`, `axum`, `serde`, `schemars`, `reqwest`, `tracing`, `thiserror`, `proptest` | One toolchain across the project; shared protocol types; small static binaries for the sandbox image |
| Database | PostgreSQL; `sqlx` with compile-time checked queries; forward-only migrations | One engine (PRD §12 decision 2) |
| Docker backend | `bollard` (Docker Engine API) | Maintained, async, no shelling out |
| Storage | The host's local disk under `storage.root`: workspace directories or block images, blobs | Fast file I/O for the agent; no object storage |
| Secrets | The built-in store (ADR-0033): AEAD in PostgreSQL, install key outside the database | One store, nothing extra to install or license |
| Firecracker backend | Firecracker and its jailer (pinned), a pinned guest kernel, `nftables` rules, tap devices | Production isolation on Linux KVM hosts |
| Web | Node 24 LTS with pnpm pinned through corepack, Next.js (App Router), React, TypeScript, Tailwind, TanStack Query, Vitest, Playwright; no component library | Team standard; generated protocol types |
| Images | Multi-stage Dockerfiles; `tqa-runtime:{base,full}`; rootfs export to ext4 for Firecracker | One artefact per mode, same digest lineage |
| CI | Hosted Linux runners for tiers 1–2; a self-hosted Linux KVM runner for tier 3; scheduled tier 4 | DEV-GUIDELINES §6 |
| Tooling | `cargo-nextest`, `cargo-llvm-cov`, `cargo-deny`, a secrets scanner, `tools/git-guard.py` | Gates G-01…G-10 |

## 3. Repository layout
```text
crates/
  tqa-types/      tqa-llm/       tqa-context/   tqa-tools/     tqa-prompt/    tqa-core/
  tqa-runtime/    tqa-sandbox/   tqa-gateway/   tqa-testkit/ (fakes, fixtures, fake model server, protocol test client)
web/              Next.js application
brand/            logo, wordmark and fonts with their licences (read-only source for the web build)
images/           runtime image Dockerfiles, rootfs build script, pinned kernel manifest
deploy/           compose/ (docker mode), firecracker/ (host setup script and checks)
docs/             PRD, ARCHITECTURE, DEV-PLAN, TEST-PLAN, DEV-GUIDELINES, adr/, evals/, admin/, protocol/
tools/            git guardrails, gate scripts
tests/            cross-crate integration, e2e task sets, performance and soak harnesses
```

## 4. Phases
Estimates assume one senior full-stack engineer using coding agents under review, plus technical-lead acceptance.
Each phase lists: goal, what is built, the demo, and the tests it must pass (its gate in TEST-PLAN §17).

### Phase 0: foundation (about 1 week)
Goal: an empty but fully gated repository, the first protocol frames, and the fakes.
1. Cargo workspace with all crates as empty libraries; `web/` scaffold; `rust-toolchain.toml`; `deny.toml`.
2. Guardrails: `.gitignore`, `AGENTS.md`, hooks, `tools/git-guard.py`; CI with gates G-01…G-10 green on the empty tree.
3. `tqa-types`: the skeleton subset of protocol v1 (`session.init`, `user.message`, `run.cancel`, `session.ready`,
   `event`, `error`; events `run.*`, `step.started`, `text.delta`, `reasoning.delta`, `tool.*`, `sandbox.state`),
   JSON Schema export, web type generation. PROTO-C-001/002, PROTO-U-003, PROTO-U-007.
4. `tqa-testkit`: `FakeProvider`, the fake OpenAI-compatible model server (`FakeModelServer`), `FakeGateway` (the
   runtime link and a streaming relay over the fake model server), `FakeClock`, the protocol test client, fixture task
   repositories; the fake sandbox backend is `tqa-sandbox`'s `FakeSandboxBackend`.
5. ADR-0001 (loop in the sandbox with a relay), ADR-0002 (Rust gateway), ADR-0003 (PostgreSQL), ADR-0005 (sequence
   numbers). `THIRD-PARTY-NOTICES.md`; admin guide and protocol reference skeletons.
Gate 0: CI green; the skeleton protocol subset published; PO approval of the documents (milestone MS0).

### Phase 1: walking skeleton (2–3 weeks)
Goal: **see an agent run end to end in the browser** on Docker mode (milestone MS1).
What is built, thinnest version of each:
1. `tqa-llm`: `openai_chat` with SSE streaming; retry with back-off; the route registry.
2. `tqa-context` and `tqa-prompt`: the message model, the write-ahead log, the projection and the resume point, the
   window with fixed slots, tool-result caps and spill, the system prompt and instruction files.
3. `tqa-core`: the native tool-call loop emitting the full event set (§5.5) from day one; a step budget; cancel.
4. `tqa-tools`: paged `read_file`, `write_file`, `edit_file`, `glob`, `grep`, `shell`, with path confinement, bounded
   output and the permission floor. No approvals yet: `ask` is answered as a denial with a clear message.
5. `tqa-runtime`: the outbound WebSocket link, `session.config`, the run supervisor with heartbeats; the
   `tqa-runtime:base` image.
6. `tqa-sandbox`: the hardened `docker` backend on an internal network, one container per conversation, the workspace
   and state directories bind-mounted from `storage.root`, the token file.
7. `tqa-gateway`: bootstrap admin login; conversations; event persistence in PostgreSQL; runtime tokens per sandbox;
   the runtime link; the server-sent event stream per conversation and the action POSTs; the sandbox manager; the
   transparent relay with the `builtin` secret store; `deploy/compose/docker.yml` with the gateway on the sandbox
   network.
8. Gates: G-09 (documentation links and test IDs) and a secrets scanner in G-10, so every tier-1 gate is real.
9. `web/`: login, conversation list, new conversation (a new empty workspace), the transcript with streaming
   text and reasoning, tool cards with output, cancel; assistant text rendered as Markdown and a transcript that
   follows new content (added after the Product Owner's first hands-on session); works over plain HTTP on a LAN
   address, not only on a secure origin.
Demo: sign in, start a conversation, ask the agent to create a small project and then change it, watch the tool cards,
see the files in the workspace directory on the host. The web pages reach the browser through the gateway (ADR-0022).
Gate 1 (TEST-PLAN §17): the skeleton subset of P0 tests and WEB-E2E-004 with the fake model server; PERF-001
baseline recorded.

### Phase 2: robustness (about 2 weeks)
Goal: the skeleton survives everything that happens in real use.
1. Reconnect with exact replay by `Last-Event-ID`; runtime link redial with buffered frames; history over HTTP then
   live.
2. Resume from the journal after a runtime or sandbox death; `run.resumed`.
3. Retry with back-off and jitter; stream-cut retry; iteration, output and wall-clock budgets; repeated-failure and
   no-progress detection; cancellation at every await.
4. Activeness by lifecycle events, heartbeats, the reaper (idle, lifetime, dead agent, parking), start-up
   reconciliation, graceful shutdown of gateway and runtime.
5. Admission for limited hosts: the resource budget and slots, per-user cap, eviction of idle warm sandboxes, the
   fair queue with the position streamed to the UI, the keep-alive pin, laptop defaults; sleeping, waking and queued
   states in the transcript header. ADR-0016.
6. Environments (ARCHITECTURE §4.7, added 23 Sep 2026 at the Product Owner's request): the Workstation image
   (`tqa-runtime:workstation`: coding toolchains, headless Chromium with Playwright, common CLI tools) as the default,
   the Minimal image, the template registry, declared capabilities in `session.config` and tool registration by
   capability, `environment.changed`, switching while idle, and the environment picker in the conversation bar.
Demo: kill the sandbox mid-run, close the browser mid-run, restart the gateway; the run finishes and the transcript
is complete. Gate 2.

### Phase 3: safety (about 2 weeks)
Goal: the boundary holds before more than one person uses it.
1. Permission policies and rules; the permission broker; prepared-action bindings; the approval prompt in the UI.
2. Egress allow-list with the policy proxy; workspaces created from a git URL, cloned inside the first sandbox through
   the proxy; the security suite against the hardening shipped in Phase 1; log redaction; opaque validated workspace
   ids.
3. The security suite in Docker mode. ADR-0004, ADR-0008.
Demo: a repository README tells the agent to send secrets to an external host; the attempt is blocked and visible;
a command outside the policy asks for approval in the browser (builtin file tools never leave the workspace,
whatever the decision). Gate 3.

### Phase 4: team (2–3 weeks)
Goal: more than one person on one box (milestone MS2 with Phase 5).
1. Users, roles, invitations, personal API tokens, API parity contract test.
2. Workspaces as entities: ownership, isolation, the write lease with `wait` and `refuse`, fork, sharing,
   quota and retention; the workspaces page and the workspace picker in the conversation bar.
3. Files and diff panel; push branch; settings; admin basics (models, secrets, egress, limits, usage).
4. Attachments: attach or drop files in the composer, stored in the workspace and passed to the agent (FR-36).
5. Languages: all product text through catalogues, ten languages, community contribution guide (FR-35); the account
   menu moves to the bottom of the sidebar (UI-GUIDELINES §7).
Demo: two users; one shares a workspace; the second's run waits for the lease, then forks and runs in parallel;
both push branches. Gate 4.

### Phase 5: local-model quality (2–3 weeks)
Goal: good behaviour on open models with small windows, measured.
1. Structured compaction with pinned items; dedupe by content hash; run notes memory; per-route cap tuning.
   Image input: a `vision` route capability (verified by the route test); images go natively to vision routes (FR-36).
2. The text-protocol fallback executor and grammar; `openai_responses` and `anthropic_messages` providers; per-route
   registry tuning; `fetch_url`; the optional `apply_patch` preset if E2E results favour it for a model. (Paged
   `read_file`, `grep` and gitignore-aware `glob` moved to Phase 1, because read and search efficiency decides how
   far a small context window goes.)
3. The E2E task set on the TensorQuay box with a real local model; failure classification; the fallback measurement.
   ADR-0007, ADR-0009.
Demo: a 50-turn task on a local model completes with a visible compaction and no loss of the task statement. Gate 5.

### Phase 6: extensions (2–3 weeks)
Goal: everything beyond the core arrives by configuration.
1. MCP over stdio inside the sandbox; the MCP proxy for connectors; the connector registry. (An external vault
   backend was dropped: ADR-0033.)
2. Agent profiles: schema, CRUD, the picker, profile-scoped tools and connectors; custom sandbox templates derived
   from Workstation or Minimal and registered by digest (the registry itself arrives in Phase 2).
3. Admin pages for connectors, templates and profiles; `tools/ext-check` conformance runners; the catalogue skeleton.
   ADR-0011, ADR-0012.
Demo: an admin registers an issue-tracker MCP server with a credential in the vault; a user creates a profile that
uses it; the agent files an issue; the credential appears nowhere in the sandbox. Gate 6.

### Phase 7: channels (1–2 weeks)
Goal: agents and humans communicate through a shared forum (milestone MS3 with Phase 6).
1. The channel entity and message schema; the four tools; atomic claims; `human` and `majority` decision rules.
2. The channel page with live updates, voting and veto. ADR-0014.
Demo: two agents on one channel propose two approaches; a human votes; the decision is posted; the winning agent
claims the task and reports. Gate 7.

### Phase 8: Firecracker backend (3–5 weeks; Linux KVM host required; milestone MS4)
1. Rootfs pipeline (OCI image to ext4), pinned guest kernel, digest manifest; host preflight and setup script.
2. The backend: jailer, tap networking and per-VM rules, block devices (rootfs, workspace image, tmpfs), boot to
   ready, destroy, list, reconciliation; the dead-agent path; resume in a new VM.
3. The same SBX-C and SEC suites on Firecracker; PERF-011; the self-hosted KVM runner and gate G-13. Snapshot restore
   as a stretch; ADR-0006.
Demo: the same product, same UI, on microVMs; a VM killed mid-run resumes in a new one. Gate 8.

### Phase 9: release hardening and v1.0 (2–4 weeks; milestone MS5)
1. The decision-engine seam (`DecisionEngine`, `ModelDecides`, the `decision/v1` types and fake) and the external
   adapter; the with-versus-without measurement. ADR-0013. An optional per-engine budget (calls and tokens, an
   administrator's setting, off by default) comes with the external adapter; until then engine use is metered as
   usage rows and bounded by the owner's relay rate (ADR-0013 amendment, point 3).
2. Soak and chaos; the full security set; dependency audit; agent waves on the TensorQuay box; the comparison with the
   open reference product.
3. Documentation: install guides, admin guide, protocol and API references, extension guide, catalogue; OpenAPI;
   release pipeline with digests and SBOM; public-content audit of the whole tree.
4. **Guardrails at the network boundary (PO, 29 Sep 2026; PRD §6 Guardrails, FR-10 superseded).** After candidate 7:
   remove the per-call permission policy (allow, deny and ask rules, the permission broker's approvals, parking while
   waiting for approval and its limit, the approval UI and API, and the human floor on the agent's git push). Inside
   the sandbox the agent works freely. The guardrail is the network allow-list (egress and ingress, sites and ports),
   and every blocked connection tells the agent which host and port were refused. Git hosts, APIs and connectors apply
   their own permissions; credentials stay in the gateway. An ADR records what is removed, the upgrade path for stored
   rules, the security analysis and the tests that go or change (SEC-003, RT-I-016, SEC-019 and others); M7 is
   restated to match.
Gate 9: PRD metrics M1–M8 met or explicitly waived; all P1 tests pass; PO sign-off.

### Phase 10 (v1.1 outline): agent teams (3–4 weeks)
A lead and its members (helpers), as decided on 2 Oct 2026 (PRD §12 item 20, ADR-0034). Built now, test-first, slice
by slice, behind the install switch `[agent_teams] enabled` (off by default), and merged into the development branch
behind it; the internal releases then in testing keep first call on testing time. Each slice comes with its tests:
1. **Roles and delivery.** `agent_teams` and `agent_team_members`; roles stored at start and checked on every route;
   the agent-team channel kind with addressed messages and per-reader last-read seqs; a send stored or refused with a
   reason, against the S1 contract in ADR-0034 (GW-U-069, GW-U-125, GW-U-126, GW-I-109's delivery
   cases, GW-I-195, PROTO-U-023, SEC-038).
2. **Starting members.** `start_member` with the fixed frame and the lead's settings, `own_copy` and `my_files`; the
   limits as `[agent_teams]` keys in `gateway.toml`, the member cap below the slots (refused with `member_limit`);
   depth 1 (GW-U-128, GW-I-106, SEC-037).
3. **Tools by role, waits and waking.** The member's two tools and the lead's ten in the runtime, with `start_member`
   alone until a task leads; waits of at most 30 s that answer at once with `yield_slot` or `yield_files`, and a
   result that tells the model to end its turn. With them comes the wake: a message for a conversation whose run isn't
   active queues one run on it, through admission, and several messages that arrive meanwhile make one wake. So a
   helper that ends its turn is always woken by its next message, and the no-deadlock case can be run end to end
   (TOOL-U-037, GW-I-109's waking cases, GW-I-196).
4. **Liveness.** The reconciler loop: idle, stuck and failed notices to the lead, `stop_member`, the idle stop and the
   deadline; and `read_helper`, the lead's read of a helper's transcript (GW-I-111, GW-I-197).
5. **Files and budget.** Handing files back with a report and `take_files` on both backends; the budget at the relay
   (GW-I-110, GW-I-107, SEC-039).
6. **Stopping and the web.** `done` with the optional acceptance check, Stop, the end note, deleting a lead's task
   with its helpers and their copies; helpers grouped under the lead in the task list and the team panel on the
   lead's page; the team limits as the administrator's runtime settings, with their page (GW-I-108, GW-I-198,
   WEB-E2E-024). With the labels in the app's copy, the guide texts waiting in `docs/planned/` move into the user and
   administrator's guides, and the guides' copy check covers them.
7. **End to end and the evaluation.** E2E-AGENTS-020; then, before v1.1 ships, the evaluation (proposal J, taken):
   about 20 representative team tasks (research, report, code) with judged results, a single agent against an agent
   team at the same token budget, written up in `docs/evals/`.


### Phase 11 (after v1.0 outline): computer use
A `desktop` capability on the Workstation template (display server, screenshot and input tools for the agent), a live
Screen panel in the conversation page; the switch to a desktop environment stays the person's, in the environment
picker (ARCHITECTURE §4.7; the agent's one-click suggestion was dropped by the owner, 10 Oct 2026, ADR-0039 §8). Needs its own security review (what a desktop session may reach).
Status (8 Oct 2026): complete for the release after v1.0 (shipped in 17.0), not in the v1.0 candidate. The runtime half of the screen bridge and
Sign out everywhere are built; the panel's review fixes are in; the security review's threats each map to a test, and
the gaps found on 8 Oct are filled (another person, a share holder and an administrator refused the screen and its
records; a sandbox's bridge reaching only its own task; the secret search in a Desktop sandbox; a share holder's and a
fork's desktop each a sandbox of their own). What remains runs on the stack once that release is merged: the Desktop suite, the
security tests that need a Desktop sandbox (SEC-057, SEC-058, the Desktop secret search, the live sign-in), the page
pass on the review stack (which gets the Desktop with one Desktop at once) and the lead's live check.

### Phase 12 (later outline): swarms
One goal across a person's own agent teams, designed in ADR-0038 (PRD §12 item 21): agent teams coordinated one level
up by a coordinator task, through the same addressed delivery (ARCHITECTURE §16.4). Scope, set by the owner on 2 Oct 2026, with a Pause goal button and the word "Coordinator": one person's own teams only; each part
of a goal becomes a new lead task from an agent profile the person picks (their own or one an administrator shared);
no publishing and no sharing between people. Where its messages are seen (ARCHITECTURE §16.4, UI-GUIDELINES §13c):
- a goal page with the parts, the budget and time, a row per team, and the coordinator's conversation with every lead,
  filterable by team, where the person can type to the coordinator;
- each lead's page with its goal messages as quiet lines;
- the task list with the goal as one item, its teams under it and their helpers under those.

It is one stored log per goal, deleted with the goal. It starts with its contract and tests, written before code, as
Phase 10 did: E2E-SWARM-009 and E2E-SWARM-017 are its end-to-end rows, and the gateway and tool rows are numbered
then.

## 4a. Where each phase stands (7 Nov 2026)
**Release 17.0** (3 Nov 2026) is the initial release and carries Phases 0 to 11; **17.1** (7 Nov) is a fix release
(§7). The builds before 17.0 were internal, the v1.0 candidate of October among them. A gate counts as reached only
by its evaluation report in `docs/evals/` ([the list](evals/README.md)); the build names those pages use are
explained in [TEST-PLAN](TEST-PLAN.md#build-names-and-ids).

| Phase | Built | Gate |
|---|---|---|
| 0 Foundation | yes | technical criteria met (12 Oct 2026); open on the product owner's MS0 record |
| 1 Walking skeleton | yes | reached (12 Oct 2026) |
| 2 Robustness | yes | reached on one condition (12 Oct 2026): SOAK-002's complete run; the ruling is pending |
| 3 Safety | yes | reached (12 Oct 2026), Docker mode |
| 4 Team | yes | reached (28 Sep 2026) |
| 5 Local-model quality | yes | reached (11 Oct 2026) |
| 6 Extensions | yes | reached (27 Sep 2026) |
| 7 Channels | yes | reached (8 Oct 2026) |
| 8 Firecracker | yes | reached (11 Oct 2026); PERF-012's microVM half waived by the product owner |
| 9 Release hardening | yes | reached (11 Oct 2026), with the same waiver |
| 10 Agent teams (v1.1) | yes, in 17.0 | not reached yet: `gate-10.md` is a draft. The evaluation of one agent against a team on the same budget showed no difference either way (indicative, `agent-teams.md`) |
| 11 Computer use | yes, in 17.0 | not reached yet: `gate-11.md` is a draft; the grounding evaluation is in `computer-grounding.md` |
| 12 Swarms | no | a later phase |

## 4b. What comes next (7 Nov 2026)
1. **Gates 10 and 11:** `gate-10.md`, `gate-11.md` and `agent-teams.md` brought to the current release's evidence,
   then the technical lead's verdicts.
2. **Gates 0 and 2:** the product owner's MS0 record, and the ruling on SOAK-002's condition.
3. **Fix releases** as findings come in (§7).
4. **Later:** "Recently refused" with "Allow this site" (ADR-0035 point 3); keeping a desktop sign-in for a whole
   task (ADR-0039 §6); Phase 12, swarms, when the product owner calls for it.

## 5. Parallel tracks
Engineering runs in three streams that own disjoint parts of the tree, so they progress in parallel and meet at each
gate: **agent and environment** (core loop, context, tools, runtime, sandbox backends and images; later channel tools,
local-model quality, Firecracker, agent teams), **gateway** (lifecycle, admission, relay, approvals API, egress
proxy, team, workspaces and channel APIs) and **web UI** (transcript, composer, approvals, workspaces, admin and channel
pages). The technical lead owns the documents, integration and acceptance.

### Track schedule by phase (set 23 Sep 2026)
Each engineer always has the next task queued; a phase closes with its gate report.

| Phase | Agent and environment | Gateway | Web UI |
|---|---|---|---|
| 2 Robustness + environments | Restart and kill-and-resume (031); parking resume (049); reconnect soak (SOAK-002 short, 062) | Environments API and template registry (033); admission, eviction, fair queue (048); parking (049) | Environment picker and switching (034); queued, sleeping and waking states (050) |
| 3 Safety | Permission broker, prepared actions, admin rules; session grants (040, 041); security suite in Docker mode | Approvals API and bindings; egress allow-list proxy; workspace from a git URL | Approval prompt; blocked-egress visibility |
| 4 Team | Git push tool; clone in the first sandbox | Users, invitations, workspaces (fork, share), personal API tokens | Workspace pages, sharing, admin pages |
| 5 Local-model quality | Compaction, fallback routes, the measured task set | Metering and rate limits per user | Usage and budget views |
| 6 Extensions | MCP client, connectors in the runtime, custom templates | Connector and profile registry API | Profiles, connectors and template pages |
| 7 Channels | Channel tools | Channel APIs | Channel pages |
| 8 Firecracker | Firecracker backend (Linux KVM host) | Backend selection, admission for microVMs | Status for the new backend |
| 9 Release hardening | Decision engine seam, soak, performance | Upgrades, backups, audit | Accessibility and performance budgets |
| 10 Agent teams, 11 Computer use | Team tools by role; X11/AT-SPI tools | Roles, delivery, wakes, budget; screen WebSocket relay | Helpers in the task list, the team panel; live screen view |

| Track | Runs during | Content |
|---|---|---|
| Documentation | all phases | Admin guide, protocol reference, API reference kept current in the same PRs |
| Benchmarks | Phases 1, 5 | Task set curation; result JSON schema; reproduction scripts; publication format |
| Security review | Phases 2, 4, 5 | Threat model updates (ARCHITECTURE §12); red-team test additions; dependency audits |
| Model coverage | Phases 1, 5 | Registry entries and E2E results per local model family; fallback tuning |

## 6. Estimates summary
| Phase | Weeks (one engineer + agents) | Cumulative | Visible result |
|---|---|---|---|
| 0 Foundation | 1 | 1 | CI, protocol, fakes |
| 1 Walking skeleton | 2–3 | 3–4 | **An agent run in the browser** |
| 2 Robustness | 2 | 5–6 | Survives kills and restarts |
| 3 Safety | 2 | 7–8 | Approvals, egress control, hardening |
| 4 Team | 2–3 | 9–11 | Users, workspaces, sharing, push |
| 5 Local-model quality | 2–3 | 11–14 | Compaction, fallback, measured task set |
| 6 Extensions | 2–3 | 13–17 | MCP, connectors, profiles, templates |
| 7 Channels | 1–2 | 14–19 | Agents and humans in one forum |
| 8 Firecracker | 3–5 | 17–24 | MicroVM mode |
| 9 Release hardening | 2–4 | 19–28 | v1.0 |
| 10 (v1.1) Agent teams | 3–4 | after v1.0 | A lead and its helpers |

The total is about the same as the layer-by-layer plan, but the first working product appears in week 3–4 instead
of week 12, and every later phase is judged on a running system. With a second person on the web UI from Phase 2,
the total is about 16–24 weeks. Deferring Firecracker to v1.1 (PRD §12 decision 3) saves 3–5 weeks.

## 7. Release process
**As practised since 1 Oct 2026 (PRD §12 item 16).** A candidate is a set of images built from one commit. It goes to
`main` only after all of these, on that same build:
1. The technical lead's full integration: every Rust suite in its own setup (unit, database, Docker, rootless network,
   microVM), coverage (G-04), every repository gate (`tools/check.sh`) and every web check step, the fake-front
   browser suite, **every** real-stack browser suite and gate demo, and the performance set (PERF-001 in release mode,
   PERF-010/011, the user-facing start, PERF-020). The verdict counts failed tests, failed gates and failed steps;
   a failure is explained and fixed, never rerun until it passes (ENGINEERING-GUARDRAILS §2.6).
2. The first-hour check: a fresh install and a new user's first session (the first-hour kit).
3. A 24-hour soak (SOAK, M5) with no other heavy work on the host.
4. The technical lead's adversarial review of every merged change and own pass in a real browser on the real stack
   (a new user, the main tasks, error paths, phone width, dark mode), with screenshots for the PO.
5. A TEST-PLAN audit: every P0 and P1 ID implemented and passing on the candidate, or its gap stated.
6. The PO's OK. Then a squashed commit to `main`, with the release notes.

**Amended 6 Nov 2026 (PO): the technical lead releases, through a pull request.** The team works solo under the
technical lead, who decides and ships each release without a separate OK from the PO. A release reaches `main` only
through a pull request opened by the TensorQuay account (never a direct push, never a personal account); the pull
request carries the release notes and the evidence of steps 1 to 5, so the PO can follow it and object there.
- **A feature release** (a new capability, a migration, a protocol change) keeps every step above, 1 to 5.
- **A fix release** (fixes only: no new capability, no migration, no protocol change) may ship the same day. It needs
  step 1 for the crates and images it changes and every repository gate, the first-hour check when a page or the
  Desktop changes, step 4, and the audit of the TEST-PLAN IDs it touches. The 24-hour soak (step 3) is required only
  when it changes the gateway's connection handling, the runtime link, the sandbox lifecycle or the event store.

**Unchanged from v0.2:**
- Semantic versions; `main` always releasable behind gates; release branches only for fixes.
- A release produces: gateway, web and runtime images by digest; the Firecracker rootfs and kernel manifest; an
  SBOM; release notes listing pinned versions and the evaluation reports.
- Protocol changes are additive within a major version; a breaking change bumps the protocol version and keeps the
  previous version served for one release.

## 8. Reporting to the Product Owner
- A short written status at the end of every week: what passed, what is blocked, decisions needed.
- An evaluation report in `docs/evals/` at every gate, written by the technical lead: scope, what was independently
  verified, measurements with environment, limits of the acceptance.
- Decisions needed from the PO are collected in PRD §12 and updated as they are made.
