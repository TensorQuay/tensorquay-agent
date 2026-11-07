# TensorQuay Agent: Development Plan

Status: **v0.6** (21 Oct 2026): v0.5 (8 Oct 2026) with where each phase stands and the c16 step of the path brought to c16's candidate 16y (§4a, §4b): its acceptance steps, its evidence so far and the PO's decisions for it since 12 Oct. This is the phase-by-phase half of the development
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
by slice, behind the install switch `[agent_teams] enabled` (off by default), and merged into dev behind it; the 13h
release and the next keep first call on testing time. Each slice comes with its tests:
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
Status (8 Oct 2026): complete for the release after v1.0 (c16), outside c15. The runtime half of the screen bridge and
Sign out everywhere are built; the panel's review fixes are in; the security review's threats each map to a test, and
the gaps found on 8 Oct are filled (another person, a share holder and an administrator refused the screen and its
records; a sandbox's bridge reaching only its own task; the secret search in a Desktop sandbox; a share holder's and a
fork's desktop each a sandbox of their own). What remains runs on the stack after c16's merge: the Desktop suite, the
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

## 4a. Where each phase stands (21 Oct 2026)
Phases 0 to 8 are built, and so is Phase 9's feature work. Phase 10 (agent teams) and Phase 11 (computer use) are built
for the release after v1.0, c16. The v1.0 candidate is **15c**: c15 became 15b (two memory-leak fixes, after c15's soak
stopped on the gateway's growing memory), then 15c (15b's server with a new web image that shows a model retry in
words); 15c is on main as `1586fd9f`. Gates 1 and 3 to 9 are reached on 15b's evidence, gate 9 with the PO's waiver
of PERF-012's microVM half; gate 2 is reached on one condition, and gate 0 is open on the PO's MS0 record (the first
row). A gate counts as reached only by its evaluation report in `docs/evals/`.

c16's candidate is **16y** (dev `5a59ad1d5`, built 20 Oct): 16x and one fix, TCP keepalive on every socket the gateway
accepts, which 16x's soak called for (below). Dev `b256c56ee` adds only a test fix (GW-I-314's case). Gates 10 and 11
are not reached yet: their pages are the drafts of 12 Oct, being brought to 16y's evidence.

| Phase | Built | Gate (21 Oct 2026) |
|---|---|---|
| 0 to 3 | yes | **reached** by the technical lead's evaluation (12 Oct 2026), on 15b's evidence (`gate-0.md` to `gate-3.md`): gates 1 and 3 reached (3 for its 20 live IDs, Docker mode); gate 2 on one condition, SOAK-002's complete run (c16's soak): 16x's soak ran every end-of-run check (20 Oct; no orphans, a clean teardown, its sign-in renewed once), and the technical lead's ruling on the condition goes in `gate-2.md`; gate 0's technical criteria met, open on the PO's MS0 record |
| 4 Team | yes | **reached** (`gate-4.md`, 28 Sep) |
| 5 Local-model quality | yes | **reached** on 15b (`gate-5.md`, the technical lead's verdict, 11 Oct): every local-model run passed on 15b's images (10 Oct): E2E-CMP-003 50/50 against the reference's 50/50, E2E-WAVE-001 60/60, E2E-FALLBACK-004 24 of 30, the phase demo, and E2E-WAVE-002 60/60 with 8 helpers; M6 met |
| 6 Extensions | yes | **reached** (`gate-6.md`, 27 Sep) |
| 7 Channels | yes | **reached** on c15 (`gate-7.md`, the technical lead's verdict, 8 Oct) |
| 8 Firecracker | yes | **reached** on 15b (`gate-8.md`, the technical lead's verdict, 11 Oct): every scope row passed in 15b's first pass (G-13 18 of 18, SBX-F-009 included); c15's SBX-F-009 failure is accepted as host memory pressure. Outside the gate: the start crash of c14b-rc wasn't reproduced in 1,000 starts on 12 Oct, and the microVM host-key case passed on c16's images |
| 9 Release | features yes | **reached** on 15b (`gate-9.md`, the technical lead's verdict, 11 Oct), **with the PO's waiver of PERF-012's microVM half** (writes at 0.33 to 0.59 of the host's speed, against 0.70; no small fix, not a 15b change; the Docker half passed). Every metric M1 to M8 is met on 15b: M5, the 24-hour soak, by the PO's approved growth rule (PRD §12, item 25), with the gap below; Gate 9's rerun passed |
| 10 Agent teams (v1.1) | yes, in c16 | **not reached yet; c16's acceptance in its last steps** (below): the tests, the page pass, the live sessions and the evaluation are done; 16y's soak and the rest of Gate 9 are under way, then `gate-10.md`. Open there: no c16 soak, 16y's included, has agent teams in its mix |
| 11 Computer use | yes, in c16 | **not reached yet; c16's acceptance in its last steps** (below): 16y's integration passes every functional suite, the live checks passed on 16o to 16q, 16u and 16x, and the grounding evaluation is measured; 16y's soak and the rest of Gate 9 are under way, then `gate-11.md` |

**The soak's gap, accepted by the PO (10 Oct):** the soak signed in once, and its sign-in expired at hour 24 (the
server's 24-hour session limit), so the end-of-run checks of the last 8 of its 72 conversations didn't run. Leftover
environments were checked by hand (none). The harness now renews its sign-in: a 2-hour rehearsal on dev `298fddf1`
with 60-second sessions passed on 11 Oct, and every end-of-run check ran. c16's soak must complete every end-of-run
check; 16x's did (20 Oct).

**c16's acceptance, where it stands (21 Oct 2026).** The order set on 12 Oct (from a row-by-row review against 16c's
evidence) still holds; the candidate moved from 16d to 16y as fixes landed.
1. **Before the build: done** (12 Oct): the `done` acceptance check built (GW-I-198), SBX-C-020 narrowed to Docker
   for v1.1, and the Desktop and test fixes merged.
2. **The integration: done on 16y** (20 Oct, 20:08 to 23:16): every functional suite passed, with no failed gate. Two
   speed-only tests failed beside other teams' jobs, both in code 16y doesn't change and both passing on 16w and 16x:
   PERF-002's event throughput (the sender was held back) and a microVM flush time (p95 91 ms against 50 ms). They
   rerun on 16y with the rest of Gate 9 (step 5). The agents stack suite passed 17 of 17 in it.
3. **The page pass and the live sessions: done on 16k to 16x** (16y changes no page): the real-world campaign the PO
   asked for, on 16k (17 Oct): 14 computer-use and 14 agent-team runs with the real model, 20 findings, fixed from
   16m on or kept as c17 items and PO decisions (`c16k-realworld/RESULTS.md`); the live checks on 16o to 16q, then on
   16u (Take over after a run, a lead's header while its team works, Stop team) and on 16x (20 Oct: the desktop's own
   accessibility reader, Take over after clearing the browser's sign-ins, uploads, a time field, typed addresses;
   the team page never reads "Done" while its team works, WEB-U-416).
4. **The GPU evaluations: done.** Gate 5's runs on 16x (20 Oct): E2E-CMP-003 50/50 against the reference's 49/50,
   E2E-WAVE-001 58/60, E2E-WAVE-002 60/60 with 8 helpers, the phase demo 1/1, no harness failure. E2E-FALLBACK-004
   passed its row at 17/30 on 16x, down from 27/30 on 16j; the binary is ruled out by replay, and a rerun on 16y gave
   26/30. The agent teams' evaluation (three nights, 16b, 16c and 16e, 40 attempts judged blind; one agent against a
   team on the same budget; indicative): "Not shown either way: both intervals include zero, on 7 and 13 tasks"
   (the judged results, 14 Oct; `agent-teams.md` §3 is being filled from them). The grounding evaluation
   (`computer-grounding.md` §6, 12 Oct, Qwen3.8 27B): by ref 200/200, by mark 199/200, by coordinates 46/200, so
   `pixel_grounding` stays off for the local route; a rerun on 16i's capture (15 Oct) gave 200/200, 196/200 and
   34/200, the same verdict.
5. **The soak and Gate 9: under way.** 16x's soak (19 to 20 Oct) passed SOAK-001's rule at 4.74 % (limit 5 %) and
   found a held connection: after a network cut, the gateway kept a sandbox's idle keep-alive open with no peer. 16y
   fixes it (GW-U-205, GW-I-314: red on 16x, green on 16y) and gets its own 24-hour soak (from 21 Oct, 00:07), with a
   check registered in advance that the same fault's connection is freed within 2 minutes. Gate 9 on 16x: PERF-001,
   010 and 020 showed no slowdown and count for 16y; PERF-021, 012, 011, 002 and 003 run on 16y itself after the
   soak. The upgrade from a 15b database (step 0088) through every c16 migration passed 66 of 66 checks on 16x.
6. **The write-ups: next.** `gate-10.md`, `gate-11.md`, `agent-teams.md` and `computer-grounding.md` are brought to
   16y's evidence, then the technical lead's verdicts.

**The PO's decisions for c16 since 12 Oct** (not yet in PRD §12): the zip advisories listed under ADR-0017 point 9
(17 Oct); the zbus 5 "unmaintained" notices not ignored, so the desktop's accessibility reader is our own on zbus 6
(18 Oct, in 16v); 16y with its own soak, while 16x's Gate 5 and the clean parts of its Gate 9 count for it (20 Oct).

**Agent teams' acceptance** (the PO's bar of 7 Oct: every test passes; the screens checked by automated tests and by a
live session; long tasks and real collaboration measured):
1. **Tests:** every agent-team row listed with its status; the edge cases written. The agents stack suite (E2E-AGENTS-020
   to 030 and WEB-E2E-062) never ran on a c16 build: on 12 Oct it ran on dev's images for the first time, found two
   faults (a helper that ends without reading its brief was never reported idle; a test found a task by a title two
   suites share), and passed 12 of 12 once they were fixed. Every integration since runs it, and the Desktop suite
   too (the agents suite 17 of 17 on 16y).
2. **Screens:** the design review is done; its fixes (an ended team reads as ended, a failed helper can be stopped, the
   lead's Stop line) are in c16.
3. **The lead's live sessions:** on c16's images with the real local model, from 11 Oct, 14:21 (agent teams, then
   computer use), again on 16i, then the real-world campaign the PO asked for, on 16k, and the team and Desktop live
   checks on 16o, 16p, 16q, 16u and 16x (step 3 above).
4. **The evaluation** (one agent against a team, on the same budget): three nights on c16's images (16b, 16c and
   16e), with long tasks and tasks that need real collaboration, 40 attempts judged blind (night 1's team side
   started no helper and was left out): no difference shown either way, and teams used their helpers in few tasks.
   Results are labelled indicative; the write-up is step 6.

The PO's decisions since v0.2 are in PRD §12, items 10 to 25.

## 4b. The path to v1.0 (11 Oct 2026)
1. **15c, the v1.0 candidate** (`release/c15` at `bf85085f`: 15b's server, dev `23112f6b`, with 15c's web image).
   - **Done:** the 24-hour soak (passed, with the accepted gap); Gate 9's rerun; Gate 5; 15b's full integration, where
     every gate and step passed and the four browser suites that failed were explained (test faults, and the retry
     display that 15c fixes); every browser suite on 15c's web; the first-hour check on a fresh install (no P1); the
     lead's browser pass (no P1). The TEST-PLAN audit's verdict: ready once the PO waives PERF-012's microVM half.
   - **Waiting on the PO:** that waiver, and the OK for the main push (§7).
   - **After the push:** the PO's own checks: the hosted model providers (E2E-PROVIDER-014) and a Mac with Docker
     Desktop (SBX-D-002, not run for 15c: no sandbox, egress or proxy file changed since c15).
2. **Gate reports refreshed** on 15c (gates 5, 8 and 9; gate 7 is done), then the PO's sign-off (MS5) and v1.0.
3. **c16, the release after v1.0** (agent teams and computer use):
   - **Contents:** computer use (Phase 11) and its review fixes; agent teams' fixes (a failed or given-up helper, the
     lead's Stop, an ended team); turns on one workspace in the order they were accepted; the compaction density
     measured per request; the repeated-failure guard's reset; the decision engine's test fake and the stack tests of
     the c15 functions that had none; the Simplified Chinese and accessibility passes.
   - **Built on 11 Oct** from dev `298fddf1`, with the Desktop; the review stack moved to it the same day. The
     candidate moved on as fixes landed, to **16y** (dev `5a59ad1d5`, 20 Oct); where its acceptance stands is in §4a
     (21 Oct).
   - **Still to come (21 Oct):** 16y's 24-hour soak (from 21 Oct, 00:07), then the rest of Gate 9 on 16y; the
     write-ups (`gate-10.md`, `gate-11.md`, `agent-teams.md`, `computer-grounding.md`) and the technical lead's
     verdicts; then the PO's own try of 16y and the PO's OK before c16 goes to main.
4. **c17, after c16:** the merge sheet's steps, in order: 15b's commit onto dev; the Web search list; the task's
   latest run (B1); "Recently refused" with "Allow this site"; the disabled decision engine's fixes; a new
   environment's slot (B6); the first hour's small fixes; copy sweeps 1 and 2; the app's own not-found and error
   pages; Escape stops a run after a confirmation; and a cancelled waiting message leaving the page with its text
   back in the box (B16, a known issue in 15c). Also for later: test stacks bind the machine's network address,
   allowed for test runs for now (the PO, 10 Oct).
5. **PO decisions (8 Oct 2026), each as the team recommended:**
   - **c15 ships with the slot leak as a known issue:** rarely, a task slot stays in use when waiting work never runs
     (after several queued tasks were cancelled, deleted or left waiting, or a busy replacement of a task's
     environment); a new task then shows "Starting…" with no error. Restarting the gateway frees the slots, and a
     later release fixes it (c15's release notes).
   - **ADR-0035's network extras** (PRD §12, item 23): the allow-list as built is confirmed; per-profile lists come
     when someone asks for them; observe mode is dropped. **"Recently refused" with "Allow this site" is queued after
     c16**, about 2 to 3 days with its tests.
   - **A desktop sign-in lasts one run in c16** (the computer-use review, finding 23; ADR-0039 §6; PRD §12, item 24):
     every run ends signed out, so working in an account means a Take over and a sign-in before each message.
     Keeping a sign-in for the whole task is for a later release.
6. **PO decisions since (9 to 11 Oct 2026):**
   - **The soak's growth rule** (9 Oct; PRD §12, item 25; TEST-PLAN SOAK-001).
   - **The soak's gap is accepted, and written down to be done later** (10 Oct): c16's soak completes every end-of-run
     check (4a).
   - **`suggest_environment` is dropped** (10 Oct; ADR-0039 amended).
   - **Test stacks may bind the machine's network address for test runs, for now** (10 Oct); to change later.
   - **The PO tests the hosted model providers personally** (10 Oct): E2E-PROVIDER-014's evidence is that test, after
     the push.

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
