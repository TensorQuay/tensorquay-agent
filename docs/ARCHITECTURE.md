# TensorQuay Agent: Architecture

Status: **v0.2** (12 Nov 2026): v0.1 (22 Sep 2026) as amended in place since, checked against the PRD on 12 Nov 2026
for the Product Owner's approval (MS0). "v1.0" and "v1.1" name the plan's scope; the releases are numbered 17.x, and
release 17.0 carries agent teams (experimental, off by default) and computer use too (PRD §6). This is the architectural design half of the
development plan; the phase-by-phase half is [DEV-PLAN](DEV-PLAN.md). Requirements: [PRD](PRD.md). Tests:
[TEST-PLAN](TEST-PLAN.md). Final types are decided in code review; sketches here fix names and responsibilities.

## 1. Design goals
1. **One harness, two runtime modes, one human surface.** The agent environment exists only inside a sandbox: a Docker
   container or a Firecracker microVM. The loop never runs on a host directly. The web UI is the only human surface;
   programs and other agents use the HTTP API and the wire protocol. There is no CLI product and no terminal UI.
   Host differences live in the runtime backend, never in the loop.
2. **The sandbox is the trust boundary.** Everything inside a sandbox (agent loop, tools, MCP servers, the user's
   code) is untrusted by the gateway. The gateway holds all secrets and enforces egress, models and quotas.
3. **Every state change is an event.** The transcript, the UI, persistence, resume and metering are all consumers of
   one ordered event stream per conversation.
4. **Local models are first-class.** Prompt shaping, tool-call fallback, budgets and caching behaviour are decided per
   model route, from the registry, not hard-coded for a vendor.
5. **Lean and testable.** Small crates with downward-only dependencies; pure crates build on macOS; contracts before
   code; each subsystem has a fake for the layer above it.
6. **Agent-first.** The environment and the protocol are built for agents; humans supervise through clients. Every
   capability in the web UI is reachable through the same HTTP API and wire protocol with an API token, so a program
   or another agent can be a client. The web UI has no private endpoints. Agents communicate with each other and
   with humans through shared channels (§16), never point-to-point.
7. **Built to be extended and contributed to.** Every extension point (§15) is a versioned contract with a
   conformance suite. A contributor adds a backend, a provider protocol, a decision engine, a template or an MCP
   server by passing that suite, not by negotiating with the core.

## 2. System overview
```text
┌────────────┐ HTTPS: SSE out ┌──────────────────────────────────────┐          ┌─────────────────────┐
│  Browser   │◄──────────────►│  tqa-gateway (control plane)          │◄────────►│ PostgreSQL           │
│  Next.js   │  POST actions  │  auth · users · conversations · events│          │ (users, convs,       │
│  (tqa-web) │                │  event streams · LLM relay · reaper   │          │  events, usage)      │
└────────────┘                │  sandbox manager ─┬─ docker backend   │          └─────────────────────┘
                              │                   └─ firecracker bknd │          ┌─────────────────────┐
┌────────────┐   same API     │  secret store (built-in, encrypted)   │◄────────►│ Host local disk      │
│  Programs, │◄────────────►  │                                      │          │ workspaces/<id>      │
│  agents    │  (API token)   └───────┬───────────────────┬───────────┘          │ blobs/ (large events)│
└────────────┘                        ▲ runtime link (WS,│ relay (per-run token)└─────────────────────┘
                                      │ dialled outbound) ▲
                     ┌────────────────────────────────────┴───┐    ┌──────────────────────────────┐
                     │  Sandbox (container or microVM)         │    │ Model endpoints              │
                     │  tqa-runtime: link client · agent loop  │    │ local: vLLM / SGLang /       │
                     │  tools · MCP servers · /workspace mount │    │ TensorQuay Engine (OpenAI API)│
                     │  write-ahead log in /run/tqa/state      │    │ remote: Anthropic / OpenAI   │
                     └─────────────────────────────────────────┘    └──────────────────────────────┘
```
- The **agent loop runs inside the sandbox** (choice A in ADR-0001). Tools execute locally with no RPC; the sandbox's
  death is a run interruption that the journal recovers from; scaling is per sandbox.
- **Model calls leave the sandbox only through the gateway relay.** The runtime speaks the route's own protocol
  (Chat Completions, Responses or Messages) to `http://gateway/relay/r/<route>/...` with its runtime token. The relay
  is a transparent, authenticated proxy: it pins the model, applies the route's vendor settings, injects the
  credential and meters usage, and never translates between protocols (ADR-0019).
- The **runtime dials out to the gateway** over one WebSocket (the runtime link) with its per-run token; nothing
  listens inside the sandbox. Browsers and programs receive events as **server-sent events** and act with plain
  **POST** requests (§9).
- The **gateway persists every event** as it relays it, so the UI can reload a transcript and reconnect with replay,
  and the reaper can see activeness.
- The **gateway is the single public origin** (ADR-0022). It serves the API and the event streams itself and forwards
  page requests, without credentials, to the Next.js server on an internal address.

## 3. Components, crates and dependency direction
Dependencies point downward only (CI gate G-07). A new crate needs an ADR.

| Layer | Crate / package | Responsibility | Builds on macOS |
|---|---|---|---|
| 0 | `tqa-types` | Wire protocol frames, event types, JSON Schemas, IDs, error codes, the `Clock` and `Sleeper` traits; `serde`, `serde_json`, `schemars`, `thiserror` only | yes |
| 1 | `tqa-llm` | `ModelProvider` trait; protocol adapters `openai_chat`, `openai_responses`, `anthropic_messages`; SSE streaming; retry and rate limiting; token limits; model registry (`models.toml`); the route shaping (model pinning, vendor settings) that the relay reuses | yes |
| 1 | `tqa-context` | Message model, token counting, compaction, tool-result caps and spill, run journal (append-only file) | yes |
| 1 | `tqa-tools` | `Tool` trait, registry, JSON-Schema validation, builtin tools, workspace path policy, shell process manager, MCP client and prepared actions; the workspace manifest walker (skip rules, records, hashing) shared by the runtime and the gateway for run changes (ADR-0030) | yes |
| 2 | `tqa-prompt` | System prompt assembly, instruction-file discovery (`AGENTS.md`), skills directory, tool-call fallback grammar | yes |
| 3 | `tqa-core` | The agent loop (`Runner`), executors (native tool calls, text fallback), budgets, recovery, event emission | yes |
| 4 | `tqa-runtime` | The process that runs the loop: the outbound WebSocket link to the gateway, TQA protocol handling, run supervisor, activeness reporting, health. Runs only inside a sandbox (container or microVM) | yes for unit tests; Docker for integration |
| – | `tqa-testkit` | Fakes (provider, sandbox, relay, clock), fixtures, the fake model server and the protocol test client used by tests and by the benchmark driver; never shipped | yes |
| 5 | `tqa-sandbox` | `SandboxBackend` trait; `docker` backend (Bollard); `firecracker` backend (per-VM rootless container, tap networking, rootfs); image resolver | trait and docker on macOS; firecracker on Linux |
| 6 | `tqa-gateway` | axum HTTP (SSE streams, POST actions) and the runtime WebSocket link; auth; PostgreSQL (sqlx); event store; LLM relay; sandbox admission and reaper; metrics; admin API | yes (PostgreSQL in Docker) |
| 7 | `web/` | Next.js app (TypeScript); generated protocol types from `tqa-types` schemas | yes |
| – | `tqa-vm-init` | The microVM guest init (PID 1): mounts, token hand-over, guest seccomp filter, starts the runtime; no workspace dependencies, and nothing depends on it | Linux |
| – | `images/` | `tqa-runtime:{base,workstation}` Dockerfiles (the Minimal and Workstation templates, §4.7); rootfs build script (OCI → ext4) and pinned kernel for Firecracker | Linux for rootfs |

Rules:
- `tqa-core` never imports `tokio::net`, `bollard` or any host API. It receives an `EventSink`, a `ToolRegistry`,
  a `ModelProvider` and a `DecisionEngine`.
- `tqa-runtime` is the only crate that constructs a `Runner`.
- `tqa-gateway` never links `tqa-core`; it treats the runtime as a black box behind the protocol.
- `tqa-gateway` may link `tqa-tools` (ADR-0030) only for pure workspace logic, the manifest walker; it never registers or runs tools,
  and never uses the shell process manager. Enforced by review; G-07 allows the crate edge.
- `tqa-gateway` may link `tqa-context` only for its token estimate (`estimate_bytes`), so that the relay meters a step
  cut off before the model server reported its usage the same way the runtime counts it. It never builds messages,
  compacts or writes a run journal. Enforced by review; G-07 allows the crate edge (`tqa-context` builds only on
  `tqa-types`, so the edge adds no cycle).

## 4. Runtime modes
### 4.1 The backend interface
```rust
#[async_trait]
pub trait SandboxBackend: Send + Sync {
    fn kind(&self) -> BackendKind;                                                   // Docker | Firecracker
    async fn create(&self, spec: SandboxSpec, cancel: CancellationToken) -> Result<SandboxHandle, SandboxError>;
    async fn wait_ready(&self, h: &SandboxHandle, readiness: &dyn ReadinessSource, deadline: Duration,
                        cancel: CancellationToken) -> Result<(), SandboxError>;      // the runtime has dialled in
    async fn stats(&self, h: &SandboxHandle) -> Result<SandboxStats, SandboxError>;  // cpu, memory, pids, uptime
    async fn destroy(&self, h: &SandboxHandle) -> Result<(), SandboxError>;          // idempotent
    async fn list(&self) -> Result<Vec<SandboxHandle>, SandboxError>;                // reconciliation after a restart
}
```
`SandboxSpec` carries: `conversation_id`, `workspace` (id and its host-disk source), `state` (the conversation's state
directory, §4.5), `image` (a content digest: a registry digest, or a local image ID for images built on the same host;
never a tag), `cpu`, `memory`, `disk`, `pids`, `egress` (mode, allow-list, proxy URL), `runtime_token`, `gateway_url`,
`env` (non-secret). Both backends accept the same spec; lifetimes are the gateway's (§4.4), not the backend's.

### 4.2 `docker` (default)
- Uses the Docker Engine API (Bollard). Works on Linux and on macOS with Docker Desktop.
- Container settings: unprivileged user, read-only root filesystem, `tmpfs` at `/tmp`, the bound workspace's host
  directory mounted at `/workspace` (bind mount by default, named volume as an option), the conversation's state
  directory mounted read-write at `/run/tqa/state`, and nothing else from the host; `cap_drop ALL` plus `cap_add`
  none, `no-new-privileges`, the built-in seccomp profile requested explicitly, PID and memory limits, no host socket
  mounts.
  The per-run token is delivered as a read-only file (`/run/tqa/token`, written with mode 0600 to a per-sandbox
  directory under `storage.root` and removed on destroy) with `TQA_RUNTIME_TOKEN_FILE` naming it, never as an
  environment value, so no secret appears in a container-create request, in `docker inspect` or in a client
  library's debug log.
- Network: every sandbox joins a per-gateway **internal** network, which has no route out. The gateway container is
  attached to that network and is the only peer a sandbox can reach: the runtime link, the relay, the MCP proxy and
  the egress policy proxy all live on it. `HTTP_PROXY` and `HTTPS_PROXY` point at the policy proxy, whose allow-list
  (default deny, host and port) decides every connection; it is the product's one guardrail (ADR-0035, §5.4), and a
  refused connection names its host and port. Changing the list never changes the network, so a sandbox cannot bypass
  the proxy. There is no external DNS: the gateway is resolved by the engine's embedded DNS, and the policy
  proxy resolves allowed host names itself (ADR-0004, ADR-0021). A development-only mode that runs the gateway on the
  host uses a non-internal network, prints a warning at start and is never the default.
- Limits of Docker mode: shared kernel; user namespaces depend on host configuration; nested sandboxing tools that
  need `bwrap` or user namespaces are not assumed.

### 4.3 `firecracker` (microVM)
- Linux host with KVM. One microVM per conversation (ADR-0006). Each VM runs inside its own container on the same
  (rootless-capable) Docker engine as Docker mode, from a pinned VM-host image: the Firecracker process runs there as a
  fixed non-root uid distinct from the service account, with no capabilities, `no_new_privs`, seccomp, a read-only root
  and per-container cgroup limits. The jailer is not used; the only host root action is granting `/dev/kvm` to that uid
  (`deploy/firecracker/host-setup.sh`).
- Rootfs: the sandbox template (the `tqa-runtime` base image or a registered custom template) exported to an ext4
  image at build time without root (one artefact per template, digest manifest with lineage to the image); a pinned guest
  kernel; the bound workspace, an ext4 image file on the host disk, attached as a virtio-block device (Firecracker has
  no shared-directory device, so a block image is the host-disk mapping); the conversation's state directory as a small
  second block image; the per-run token on a read-only configuration device, never on the kernel command line or in the
  VM or container configuration; `/tmp` on tmpfs. The guest init (`tqa-vm-init`) mounts these, applies the guest seccomp
  filter and starts the runtime as uid 1000.
- Network: one tap device per VM inside that VM's container network namespace, with per-VM nftables rules that allow
  only the gateway's address (link, relay, MCP proxy, policy proxy); no DNS and no direct egress; the container sits on
  the same internal sandbox network as Docker mode (ADR-0021). Nothing is created in the host network namespace.
- Lifecycle: create → boot → runtime health → ready. Snapshots for warm start are a Phase 8 stretch goal (ADR-0006).
- Host requirements and the setup script are in the admin guide; the backend refuses to start without KVM access for
  the VMM uid, the pinned kernel, the VM-host image and the rootfs digest. If this rootless design fails on KVM access,
  networking, performance or the SBX-C and SEC suites, the jailer-based design is reconsidered (ADR-0006).

### 4.4 Sandbox lifecycle, activeness and admission
```text
creating ──► ready ──► busy ◄──► idle (warm) ──► destroyed
    │           ▲         │  (run.started / terminal run event)     ▲
    └── failed ─┘
```
**Activeness is defined by lifecycle events, never by processes in the box.**
- A run is **active** from `run.started` until its terminal event (`run.completed`, `run.failed`, `run.cancelled`),
  both emitted by the runtime's run supervisor, which owns the loop task and therefore knows whether it is alive.
- While a run is active the runtime's run supervisor sends a `heartbeat` frame on the runtime link every 30 s. It comes
  from the supervisor, not from the socket layer, so a live socket over a dead supervisor is still detected.
  Heartbeats are liveness only; progress events (`text.delta`, `tool.output`) are liveness only. None of them flips busy or idle. Inferring activeness from traffic
  is how sandboxes get stuck busy or flip idle in the middle of a long tool call, so the design forbids it.
- Processes are not an input: a background dev server or a stuck `sleep` does not keep a sandbox alive, and a run
  waiting on the model with zero CPU is still active. Processes matter only through the keep-alive pin (the user's
  explicit choice) and as diagnostics.
- A connected client is not an input either: a run continues detached. The client only selects which idle window
  applies (attended or detached).
- Lost terminal events cannot strand a sandbox: events are journaled before they are sent, so a runtime that redials
  re-sends them; a runtime that dies stops heartbeating and the dead-agent rule takes over.

**Run states:** `running`, `waiting_channel`, terminal. No run waits for a person: there are no approvals
(ADR-0035). A run waiting on a channel keeps its sandbox busy in v1.0; parking such a run comes later (ADR-0014).
Histories written before ADR-0035 may hold `run.parked` and a `parked` sandbox state: both are retired, read-only
markers, and a run that was parked at the upgrade is ended with `approvals_removed`, so a message starts a new one.

**Reaper rules,** evaluated from the gateway's in-memory table (persisted mirror in PostgreSQL for restart):
1. max lifetime exceeded → an idle sandbox (pinned or not) is destroyed; a busy one takes no new work and gets a fixed
   grace (120 s) to finish, then is destroyed while
   busy and a replacement starts at once, where the run resumes from its journal. The lifetime counts from the sandbox's
   start, and each sandbox keeps the value it was admitted with;
2. idle longer than the idle window → destroy (defaults for a personal computer: 10 min while a client is attached,
   1 min when detached; shorter on Firecracker, where an idle VM reserves its memory). The window is measured from the
   last accepted action or run lifecycle event, never from socket traffic: a live link, its pings and pongs, and
   supervisor heartbeats do not extend it. A sandbox whose run is active is never idle;
3. busy but no heartbeat for 3 intervals → mark `dead`, destroy, mark the run `interrupted` (resumable).

**Admission: slots from a resource budget.** The gateway takes a configured share of the host (defaults: half the
memory, cores minus one), divides it by the per-sandbox limits, and gets a number of **slots**. A per-user cap
applies on top. When a run needs a sandbox and no slot is free, in order:
1. evict the longest-idle warm sandbox (free, it holds nothing durable);
2. otherwise queue the run with per-user fairness; the UI shows "waiting for a slot" and the position;
3. never evict a busy sandbox before its max lifetime, and never evict a pinned one.
The UI shows slots in use. Diagnostics warn when the budget is smaller than one sandbox.

**Keep-alive pin.** A user may pin a conversation: its sandbox stays live through idle up to the max lifetime, so its
next message starts without waking a sandbox. Pins count against the user's cap. A pin keeps the sandbox, not
processes: background processes stop when the command that started them returns, and its answer tells the model so
(ADR-0016 amendment).

- The workspace outlives the sandbox. A new message on a destroyed conversation creates a fresh sandbox on the same
  workspace and the runtime resumes from the journal; the UI shows "sleeping" then "waking" (2–3 s in Docker mode),
  never an error. While a sandbox is busy it holds the workspace's write lease (§4.6).
- A conversation is bound to one backend at creation; the binding is stored and never changes.

### 4.5 Storage: the host's local disk
Workspaces are never on object storage; it is too slow for an agent's file operations. Everything lives under one
configured directory on a **local disk of the host** where the agent environment is installed (`storage.root`: a
folder on a Mac running Docker Desktop, or a data directory on a Linux KVM host).
- **Workspace, Docker mode:** `storage.root/workspaces/<workspace_id>/`, bind-mounted at `/workspace` (default), or a
  named volume on the same disk when the admin prefers Docker-managed storage. On macOS Docker Desktop the documented
  setting is the VirtioFS file-sharing mode.
- **Workspace, Firecracker mode:** `storage.root/workspaces/<workspace_id>.ext4`, created sparse at the quota size and
  attached as a virtio-block device. The gateway reads workspace files only through the runtime inside the sandbox
  (files and diff API), so the host never mounts an image while a VM holds it.
- **Durability on the microVM backend.** A VM keeps workspace writes in its own page cache, which a VM crash loses. So the
  runtime flushes the workspace (`syncfs` on `/workspace`) before it journals each tool result. This is the backend
  property `sync_before_result`, true for microVMs and false for Docker, whose host page cache survives a container kill.
  A journaled result therefore never runs ahead of its file changes. The guest also writes dirty data back within about
  a second (defence in depth for background writes, not relied on). Writes of a call still running when its VM dies may be
  lost; that call's result is `interrupted`.
- **Contents:** `/workspace` (the user's repository or files) and `/workspace/.tqa/<conversation_id>/` (that
  conversation's spill files, run notes and MCP logs, which the agent may read), so several conversations can share
  one workspace without mixing their files.
- **Conversation state, outside the workspace:** `storage.root/state/<conversation_id>/`, mounted at
  `/run/tqa/state`, holds the runtime's write-ahead log. Only that conversation's sandbox mounts it, so an agent in a
  shared workspace cannot read or rewrite another conversation's history (ADR-0020).
- **Blobs:** event payloads above the size cap and exported artefacts in `storage.root/blobs/`, on the same disk.
  S3-compatible storage is not part of v1.0.
- **PostgreSQL:** users, sessions, workspaces, conversations, sandbox bindings, events, usage, settings.
- **Diagnostics** warn when `storage.root` is on a network or otherwise slow filesystem (PERF-012 measures).

### 4.6 Workspaces: the unit of isolation
Three units, each with one job:

| Unit | Job | Lifetime |
|---|---|---|
| **Workspace** | Isolation and continuity: a directory or block image on the host disk, owned by one user, with a quota | Until the owner deletes it or retention expires |
| **Conversation** | Interaction: one thread of messages, runs and events, bound to exactly one workspace and one profile | Until its owner deletes it (then purged, below) |
| **Sandbox** | Execution: one container or microVM that mounts exactly one workspace | Disposable; recreated on demand |

Rules:
- **Ownership and isolation.** A workspace belongs to one user. A sandbox mounts only the workspace its
  conversation is bound to, and nothing else from the host; cross-user access is refused at the API, the stream entry
  and the sandbox manager (SEC-014). Admins see workspace metadata (owner, size, last use) but not contents.
- **Binding.** A conversation binds to one workspace at creation and keeps it. Binding an agent runtime to a
  workspace is the design choice: the runtime never sees "the user's files", only "this workspace".
- **Reuse.** Many conversations may bind to the same workspace, sequentially or concurrently. Only one conversation
  at a time may **run** on a workspace: the running sandbox holds a **write lease** (recorded on the workspace,
  renewed by heartbeats, released on the run's terminal event, sandbox death or lease expiry). A second conversation that
  starts a run while the lease is held either waits (default) or is refused with `workspace_busy`, per profile
  setting. Reading files and history is always allowed.
- **Fork.** "Fork workspace" creates an independent copy (a directory copy in Docker mode, a copy of the block image
  in Firecracker mode, copy-on-write where the filesystem supports it) so two agents can work in parallel on the same
  starting point (an agent team's `own_copy` member works on one, ADR-0034). Overlay-based forks are later work.
- **Sharing.** A workspace may be shared with named users or a team with `read` or `write` access; the lease rule
  still applies to writers. Sharing is off by default.
- **Creation.** From the workspace picker in the conversation bar of a new conversation (no dialog, §11): pick an existing workspace, create an empty one, or fork one. Nothing
  is specific to git (PRD §12 item 22): the agent fetches files itself (git included, with the person's key from key
  management); the API's git address on a new workspace is removed, and a request that still sends one is refused by name (ADR-0037 amendment 4). Workspaces also have their own page for listing, renaming,
  sharing, size and deletion.
- **Deletion is final** (PRD §12 item 18). Deleting a task or a workspace marks its row `deleted` and writes one
  content-free `admin_audit` row (area `tasks` or `workspaces`, the id, the actor) in the same transaction. A deleted
  task answers `404` at once, its open streams end and its runtime token goes; a deleted workspace's files go at once.
  The rest is the background purge's (`purge.rs`): one supervised task, woken by a delete or once a minute, holding
  one connection per cycle and deleting at most 500 rows a statement. The `deleted` rows are its queue and each is
  deleted last, files before rows, so a purge stopped anywhere resumes from the database alone. It never touches a
  live task (active, or deleted with a sandbox or a runtime link). A task's purge removes its events and their files,
  stored answers (`idempotency`, by `conversation_id` and by `about_conversation`), run snapshots and manifests, push
  operations, runtime tokens and runtime state; its usage stays, attributed by `usage.user_id`, and its channel posts
  stay without `author_name`. A workspace is purged once its tasks are: files, `changes/`, snapshots, shares, its row;
  forks keep their files and lose `forked_from`. A task delete never touches its workspace. `tqa_purge_targets_total`
  and `tqa_purge_oldest_seconds` show it working.

### 4.7 Environments: templates and switching
An **environment** is the machine the agent works in. It is described by a **template**; the sandbox is only the
current instance of it.
- **Templates** (gateway table `templates`): name and display name, the OCI image digest (a registry digest or a local
  image ID, §4.1), the Firecracker rootfs digest exported from the same image, **declared capabilities**
  (`shell`, `python`, `node`, `build`, `browser`, `desktop`), resource defaults (a browser needs more memory),
  and whether it is the default. Two first-party templates ship: **Workstation** (the default: the everyday coding
  toolchain plus a headless Chromium with Playwright and common CLI tools, image `tqa-runtime:workstation`) and
  **Minimal** (the runtime and core utilities only, image `tqa-runtime:base`, for tests and locked-down installs).
  Custom templates derive from either by digest (§15). One template serves both backends.
- **Binding:** a conversation stores its current template (and the digest it resolved to). Every sandbox created or
  woken for it is built from that template. The workspace and the conversation's state directory live on the host
  disk (§4.5), so any template can mount them.
- **Capabilities are declared, never detected.** `session.config` carries the environment (template name, digest,
  capabilities); the runtime registers only the tools those capabilities allow and states them in the system prompt.
- **Switching:** while no run is active, the owner may change a conversation's template
  (`PUT /conversations/{id}/environment`). The gateway retires the current sandbox; the next action creates one from
  the new template with the same workspace and history; running processes end, as when a terminal closes. The new
  runtime compares the environment in `session.config` with the last `environment.changed` in its history and, when it
  differs, emits `environment.changed { template, capabilities }` as a normal sequenced event before anything else, so
  the transcript and the model both learn of the change (the gateway never creates sequenced events, ADR-0005). A switch
  during a run is refused with `run_active`.
- **Computer use** (a virtual desktop the agent sees and operates, streamed to the web UI) is DEV-PLAN
  Phase 11, in release 17.0 (ADR-0039); nothing in v1.0's scope depends on it. Design: a `desktop` capability on the Workstation image adds a virtual
  display (Xvfb at a fixed size such as 1280×800) with a light window manager and ordinary applications (a headed
  browser, a terminal, a file manager). The agent **observes** through screenshots scaled to the model's expected size,
  the Linux accessibility tree (AT-SPI) for element names, roles and positions, and, inside the browser, Playwright's
  DOM and accessibility snapshot, which is preferred over pixels. It **acts** through the X11 XTEST extension (driven with
  `xdotool`), which applications accept as real input (move, click, double-click, drag, scroll, type, key combinations), with coordinates mapped back from the scaled screenshot.
  Structured observation comes first: an accessibility or DOM snapshot lists elements with short-lived **refs** (role,
  name, state), and the model acts by ref (`click(ref)`, `type(ref, text)`); the tool performs the element's own
  accessibility action (press, activate, set text, focus) and falls back to a click at the centre of the element's
  reported extents, never exposing coordinates to the model. A ref from an older snapshot is refused with a request to
  re-observe. Pixels and coordinates are the fallback for content without accessibility information (canvas, charts,
  custom widgets). Preferred order: browser DOM refs, accessibility refs, pixels. Accessibility comes from AT-SPI2 over
  D-Bus (role, name, states, value, screen extents, actions, editable text); coverage is good for GTK, Qt with its
  bridge, LibreOffice, Firefox and Chromium with renderer accessibility on, partial for Electron and Java, absent for
  canvas-drawn apps. The session sets the AT-SPI accessibility flag on the session bus at start, so Chromium and toolkit
  apps build their trees without a relaunch. The display is X11 (Xvfb) because it reports exact global positions and
  accepts XTEST input, which Wayland withholds. An application can stop answering its tree, so each call to it is
  bounded (3 s; a whole window's walk 10 s): past the bound the screenshot goes without that application's elements,
  and an action on one is refused as unavailable (TOOL-U-138). A tree box whose centre lies outside the focused
  window's frame (GTK4 reports boxes relative to the window) gets no mark; the element stays listed for its own actions
  (TOOL-U-139). Typed text sends a line break or a tab as the Return or Tab key, never as a character (TOOL-U-140).
  One `computer` tool exposes `screenshot`, `click`, `double_click`, `drag`, `scroll`, `type`, `key` and `wait`; a
  route whose protocol has its own computer-use tool gets it through the transparent relay (ADR-0019); the route must
  declare `vision`. The web UI shows a **Screen** panel: a VNC server in the sandbox serves the display on the internal
  network only; the gateway relays it as an authenticated WebSocket (`/api/v1/conversations/{id}/screen`, same origin,
  ADR-0022, no published port); the browser renders it with noVNC (MPL-2.0, unmodified, ADR-0017), view-only until the
  user presses **Take over**, which pauses the run and enables input. Every `computer` action also stores a before and
  after screenshot with its result, so a run stays replayable and auditable after the sandbox is gone. Alternatives checked on 23 Sep 2026:
  KasmVNC is GPL-2.0 and cannot be bundled in the web client; Selkies (MPL-2.0, WebSocket by default, up to 60 fps with
  H.264) is the upgrade path if a measured Phase 11 spike shows VNC too slow for video-heavy work; WebRTC-only transports
  need media ports and relays that conflict with the sealed sandbox network. Screenshots are size-capped events; egress rules are
  unchanged.

## 5. The agent loop (`tqa-core`)
### 5.0 Event-sourced state
The run's single source of truth is its **event log**. The message list sent to the model, the transcript in the UI,
resume, metering and the golden tests are all projections of that log: `project(events) -> Messages` is a pure
function in `tqa-context`, and the loop never holds state that is not derivable from events. Consequences:
- resume is exact: replay the journal, project, continue;
- tests are golden event logs, not mocks of internal state;
- the UI and the model see the same history by construction.
The loop is a small function: `step(state, provider, tools, engine) -> events`; it is kept under 500 lines.

### 5.1 Turn model
A **run** is one user message processed to a final answer. A run has **steps**; each step is one model call followed
by zero or more tool executions. The loop:
1. Build the request: system prompt, instruction files, tool definitions, the context window from `tqa-context`.
2. Stream the model response. Emit `text.delta`, `reasoning.delta` and `tool.call` events as they parse, then
   `model.completed` when the response has fully arrived.
3. For each tool call: parse the arguments, look up the tool, validate the schema and freeze the call (ADR-0035: no
   permission check). A call that fails any check is answered with an error `tool.result` and never runs. The rest run
   in batches: consecutive parallel-safe calls run concurrently as one batch, any other call runs alone, and batches run
   in call order, so a read never overtakes an earlier write. A call that runs emits `tool.start`, `tool.output` and
   `tool.end` (always as a pair), then its capped `tool.result`; a call that never runs gets only its `tool.result`.
4. When every call of the step has a result, emit `step.completed`; check budgets; loop. A response without tool
   calls ends the run with `run.completed`.
5. Any interruption (cancel, stream cut, provider error after retries, budget) ends the run with a typed reason. A
   sandbox shutdown is different: the loop stops without writing a terminal event, so the next sandbox resumes the run
   from the journal (§5.5).

### 5.1a Decision engine
The loop consults a `DecisionEngine` at two points: before a step (`before_step`) and after a model response
(`after_response`).
```rust
#[async_trait]
pub trait DecisionEngine: Send + Sync {
    async fn before_step(&self, s: &RunState) -> Decision;          // CallModel { route, tool_subset } | Act(tool_call) | Compact | Stop(reason)
    async fn after_response(&self, s: &RunState, r: &ModelResponse) -> Vec<PlannedAction>; // filter, reorder or drop tool calls
}
```
- `ModelDecides` is the default: `before_step` always returns `CallModel` on the conversation's route with all tools,
  and `after_response` returns the calls as the model made them. With it, the loop is exactly §5.1.
- An external engine implements the decision protocol (`decision/v1`, HTTP with JSON, documented like the wire
  protocol) and can: narrow the tool list, act on routine steps without a model call, choose a cheaper or stronger
  route per step, request compaction, or stop early.
- Safety: the engine's output passes through the same checks as a model call; it can narrow the step's tools, never
  widen the task's tools or the network. If the engine is unreachable or returns an invalid decision, the loop emits
  `decision.fallback` and uses `ModelDecides` for that step.
- Every decision of an external engine is an event (`decision`), so its effect on tokens, steps and completion is
  measurable per run. `ModelDecides` is the identity and emits no events.
- The limits of each decision (routes, `Act` through the same path as a model call, `Stop` only as a failure in v1), the
  events that double as the replay record, and the `decision/v1` protocol are ADR-0013.

### 5.2 Executors
- `NativeToolExecutor`: the provider returns structured tool calls; the harness never parses text for calls.
  Reasoning content is streamed as `reasoning.delta` and never re-parsed; it is sent back to the provider only where
  the protocol requires it (for example Responses API items).
- `TextProtocolExecutor` (fallback, FR-2): the model is instructed to emit tool calls in a fenced JSON block with a
  fixed grammar; the parser repairs common faults (trailing commas, unquoted keys, missing fence) and asks the model to
  retry once on failure. Selected per route by `native_tools = false`. A server whose own tool-call parser takes a
  call the model wrote in its native format leaves nothing on a text route; the Chat decoder counts the chunks it held
  back, and a reply that delivered no call but has some is asked again once in its step with the tools declared
  natively. Native calls then win over any the model also wrote, so a call written both ways runs once. Once such a
  retry brings native calls back, the route declares the tools for the rest of the run: one doubling per run, not
  per step. A server that refuses declared tools leaves the route text-only for the run (CORE-U-065, LLM-U-046). A
  known edge: a genuine final answer followed by a proxy's empty keep-alive chunk looks held back, and costs one extra
  model call. Where a server has no parser and the model's native markup arrives as text, the parser
  recovers it when the reply has no fenced call (PROMPT-U-008).

### 5.3 Budgets and recovery
- Iteration budget (steps per run), output-token budget, wall-clock budget; each has a soft warning event
  (`budget.warning` at 80 %, once per run) and a hard stop (`run.failed` with code `budget`).
- The wall-clock budget counts active time only. `step.completed` carries the step's `duration_ms`, measured with the
  injected monotonic clock, so a resumed run continues its budget from history and never counts parked or stopped time.
- Transient errors (408, 429, 5xx, timeouts, stream cut, and a connection refused, reset or closed before any response
  headers - for example while the gateway's model relay restarts) retry with exponential back-off and jitter, at most
  3 attempts per step; the back-off window must outlast a gateway restart. DNS, TLS verification, other 4xx and
  request-size errors are permanent, with one exception: the runtime's calls go only to the gateway's relay, whose name
  can briefly fail to resolve and whose address can be unreachable while it restarts, so for those calls a failed
  lookup or an unreachable host before any byte is sent is retried like a refused connection (LLM-E-029). Each attempt is a new `step.started` attempt, preceded by a `provider.retry` event; other errors fail at once.
- For an HTTP error, `provider.retry` and `run.failed` also carry `provider_message`, the provider's own words from the
  response body. It's sanitised: the message field when the body is JSON, no control characters, credentials
  redacted (including any token after `Bearer` or `Basic`), and at most 300 characters. It's left out when there's none.
  `reason` and `code` stay the bounded classification, and request headers are never read.
- A refusal the gateway makes itself on its relays (not a provider's answer passed through) carries its code in the
  `X-TQA-Gateway-Error` response header, which the relays strip from anything forwarded (GW-I-148). The runtime reads
  that header only: the reason becomes `gateway.<code> <status>` (for example `gateway.key_use_unrecorded 503`), so the
  page never credits the provider with it, and no `provider_message` is quoted. It's retried by its status like any
  other HTTP error; the relay's own throttle keeps its wait-it-out handling. A connector call's refusal by the MCP
  proxy takes its `<code>` from the same header, and is `mcp_error` without it.
- A repeated identical tool failure ends the run with `repeated_failure`: the same tool with the same arguments failing 3 times with no change between, or 6 times in the run whatever changed. A successful call of a tool that can change the workspace (`ToolSpec.changes_workspace`: `shell`, `write_file`, `edit_file`, and the team lead's `take_files`) starts the count of 3 again, so fix-and-rerun goes on while the model edits. Any successful `shell` call counts as such a change, even one that only reads or rewrites the same bytes: a loop of a failing call and such a shell call is bounded by the ceiling of 6. The page shows both reasons in one wording; the journal's `run.failed` reason says which limit it was.
- Progress detection: a step makes progress when one of its tool results has content not seen earlier in the run
  (write and edit results carry a diff summary, so real changes count). After N steps (default 6) without progress
  the loop emits `budget.warning(no_progress)`, which the projection turns into a fixed nudge; after 2N it ends the
  run with `no_progress`. A shell side effect with byte-identical output is not detected.

### 5.4 Guardrails: the network boundary (ADR-0035)
- Every call the model makes runs: there is no per-call policy, rule, `ask`, approval or grant. The task's tools, from
  the profile's tool choice (ADR-0012), set what the agent can do; a task given no shell or write tools can't run
  commands or write.
- The loop freezes each validated call into a **prepared action**: the exact canonical JSON of the call and its
  context (conversation, run, step, attempt, call id), hashed. Since the Git relay's removal (ADR-0037 §7) nothing
  outside the loop consults it.
- The guardrail is the network: every connection from a sandbox is decided at the gateway's egress proxy by host and
  port (§4.2, §10). A refused connection names its host and port in the proxy's answer (GW-U-081), and the system
  prompt lists the task's allowed host names (from `session.config`'s `network`, PROMPT-U-009), so the agent knows
  the rule before it tries.
- Every git in the sandbox runs with repository hooks and the file-system monitor switched off through environment
  configuration, so a shared workspace can't run code as another member (SEC-045). That protects people from each
  other, not the agent from itself.
- The decision engine can narrow a step's tools or stop; it can't widen the task's tools or the network (§5.1a).

### 5.5 Events and journal
- Every event on the wire is an `event` frame carrying `seq` (per conversation), `run_id`, `step`, `ts` and the typed
  `event` (its `type` and fields); the journal stores the same frame shape.
- **History is authoritative at the gateway; the runtime keeps a write-ahead log.** The runtime appends each frame to
  its log (`/run/tqa/state/events.jsonl`) before sending it; the gateway persists on receipt. When a link registers,
  `session.config` tells the runtime the last stored `seq`, and the runtime resends only the frames after it. A new
  sandbox resuming a run loads history from the gateway (`GET /runtime/history`, runtime token) and takes only the
  unacknowledged tail from its own log. Nothing is ever read back from the workspace (ADR-0020).
- Log format: one JSON record per line (`seq` plus the frame), each written through to the kernel before it is sent;
  `fdatasync` at every `step.completed`, at run end and every 100 records. On open, records must be contiguous; a
  torn final line is truncated and reported; corruption earlier in the file is an error.
- **Resume point**, computed purely from history:
  - the last step has `step.completed`: start the next step;
  - the last step has `model.completed` but no `step.completed`: resume in the tool phase. Calls with a
    `tool.result` are done; a call with `tool.start` but no result gets a `tool.result` with status `interrupted`
    ("the sandbox stopped while this tool ran; it may be retried"); a call never started is dispatched now; a run
    whose last event is the retired `run.parked` (parked for an approval before ADR-0035) is failed with
    `approvals_removed` instead, and its waiting call never runs;
  - otherwise (the highest attempt has no `model.completed`) the model is called again with `attempt + 1`;
  - a trailing `message.user` without its `run.started` (a crash between the two appends): start that run;
  - no active run: idle, after closing any calls an ended run left without results.
- Projection rules: `message.user` becomes a user message. Per step, the highest attempt is authoritative: starting
  a new attempt supersedes every earlier attempt of that step, because the loop only starts one when the previous
  response failed or was rejected. The authoritative attempt contributes its deltas and `tool.call`s as the assistant
  message once it has `model.completed`, and each `tool.result` for its calls becomes a tool message, listed in call
  order whatever order the results arrived in; the model never sees `tool.output` directly.
- Run identity: a run's ID is `r-<seq of its message.user>`, and `message.user` is always followed immediately by that
  run's `run.started`. Recovery is therefore idempotent: a resumed start always recomputes the same ID.
- Every `tool.call` is closed by a `tool.result` before its run ends; on cancel the loop emits results with status
  `cancelled`. If a crash leaves calls of an ended run without results, the resume point reports them and the runtime
  closes them with `interrupted` results before the next run starts, so a provider never receives a tool call without
  its answer.

## 6. Model providers (`tqa-llm`)
```rust
#[async_trait]
pub trait ModelProvider: Send + Sync {
    fn route(&self) -> &Route;   // protocol, model, capabilities, limits (derived or guessed)
    async fn complete(&self, req: CompletionRequest, cancel: CancellationToken)
        -> Result<CompletionStream, LlmError>;   // always streaming; a non-streamed answer is a collect
}
```
- **Protocols, not vendors.** Three wire protocols are implemented, one module each: `openai_chat` (Chat
  Completions; the local default), `openai_responses`, `anthropic_messages`. **Any endpoint that speaks one of them
  is a configuration entry, never code:** a local vLLM, SGLang, llama.cpp server, Ollama or LM Studio; TensorQuay
  Engine; a cloud vendor; an aggregator; a company gateway. Vendor differences are route properties (`auth`,
  `reasoning`, `extra_body`, `quirks`), and the loop never branches on a vendor name.
- **Registry (`models.toml`, or the same records in the admin UI):**
  ```toml
  [[routes]]
  name = "local-glm"                         # the logical name users pick
  protocol = "openai_chat"                   # openai_chat | openai_responses | anthropic_messages
  base_url = "http://model-server:8000/v1"
  model = "glm-5.3-flash"
  auth = { kind = "none" }                   # none | bearer | header (name = "x-api-key") | query (name = "api-key")
  key_ref = "secrets/local-glm"              # a secret-store reference, never the key itself
  context_window = 128000                    # optional; absent means "guessed" and budgets are conservative
  max_output = 16000
  native_tools = true                        # false selects the text-protocol fallback executor
  parallel_tools = true
  vision = false
  reasoning = { field = "reasoning_effort", allowed = ["low", "medium", "high"], default = "medium" }
  cache_hint = "prefix"                      # prefix | anthropic_cache_control | none
  stream_usage = true                        # the default: ask for usage in the stream; false leaves stream_options out
  extra_headers = {}                         # merged into every request verbatim
  extra_body = { chat_template_kwargs = { enable_thinking = true } }   # vendor knobs; merged verbatim
  quirks = []                                # named workarounds implemented in the protocol adapter
  strict = "auto"                            # auto | on | off: when tools go out with strict: true
  schema_keywords = "full"                   # full | basic: which JSON Schema keywords tool schemas keep
  instructions_role = "system"               # Chat: system | developer; Responses: top_level | developer
  first_response_secs = 300                  # 5..900: how long to wait for the model's first byte
  ```
  `reasoning.field` maps to the protocol's own field (`reasoning_effort`, `reasoning.effort`, or on Messages a
  thinking token budget from `budget_tokens`, one per allowed value). A Messages route with no `budget_tokens` thinks
  adaptively: requests send `thinking = {type = "adaptive"}` and the effort as `output_config.effort`, for models that
  refuse a budget (Claude Opus 5.5 and later take `low`, `medium`, `high`, `xhigh`, `max`; set `max_output` high
  enough for the thinking too, such as 64000), and `extra_body` may add to `output_config` but not set its effort
  (LLM-U-055). Such a route replays no earlier thinking (LLM-U-057): the protocol binds each thinking block to the
  conversation before it and refuses a replayed block once that changed, and the runner re-renders earlier
  screenshots (ADR-0039 section 3); `extra_body` and `extra_headers` are the escape hatch for anything a vendor adds; a `quirk` is a named,
  tested workaround in the adapter, added only when `extra_body` cannot express it. `strict = "auto"` sends a tool
  strict exactly when its schema meets the strict rules (every builtin's does); `on` and `off` force it for an endpoint
  that needs or refuses the field. `off` is also for a server whose constrained decoding narrows what a strict call can
  say: vLLM 0.30 with the `qwen3_coder` tool parser never emits a line break inside a strict tool's string argument, so
  a multi-line edit can't be written (docs/evals/gate-5.md); such a route sets `strict = "off"`. "Test route" finds
  it: on a route that sends strict tools it asks for a two-line value from a strict tool and, when the line break is
  lost, once more without strict; if that keeps it, the check fails and suggests `strict = "off"` (GW-U-064). `schema_keywords = "basic"` reduces tool schemas to a documented keyword subset for
  an endpoint that accepts no more (without `additionalProperties`, so such a tool is sent non-strict under `auto`); the
  registry still validates every call against the full schema. Both apply in the adapter and in the relay, so the
  runtime needn't know them. `instructions_role` says where the merged instructions go: absent is the protocol's default
  (Chat: one leading `system` message; Responses: the top-level `instructions`; Messages: the top-level `system`, its only
  value), and `developer` sends one leading `developer` message instead, for an endpoint that wants it. The runtime always
  sends the default and the relay rewrites only that; one shaping step before the route's extras applies the
  instructions role, then the tool-schema settings. `first_response_secs` is how long the relay waits for the model's
  first byte (5 to 900 seconds). Unset, it is 300 for a local route (no key, or a base URL on this computer or the
  owner's network: `localhost`, a loopback address, the container host, a single-label name such as a compose service,
  `*.local`, `*.internal`, or a private, link-local, Tailscale (100.64.0.0/10) or unique-local address; no name is
  looked up; the same rule as the route check's longer bound) and 60 otherwise, since a model server on the same computer, on CPU, reads a long prompt for a minute or more before it
  answers. A slower first byte is the relay's own 504 `timeout` (named by `X-TQA-Gateway-Error`). The bound reaches the
  runtime in `session.config`'s route snapshot (revision 10), and the runtime waits ten seconds longer, so the relay's
  refusal is what a run sees; the relay's global `header_timeout` still bounds its other upstream calls. The page says
  "Waiting for the model to start answering…" once a model call has had nothing back for 10 seconds.
- **Verified routes.** The admin UI's "test route" sends a tiny streaming request with one tool call round trip and
  records what it observed: streaming, tool calls, usage reported, reasoning field accepted. The picker shows the
  result, so a misconfigured endpoint is caught before a run. `GET /api/v1/admin/routes/{name}/models` lists the
  models the route's endpoint offers (`{base_url}/models` on every protocol, a Messages endpoint's pages followed),
  with the route's stored credential, ids and display names only, to fill the model list.
- **Routes are the administrators'.** Only administrators add, change or remove routes; members choose from them
  (PO, 1 Oct 2026; PRD FR-4). A route's key lives in the secret store and never reaches a sandbox. The relay resolves
  the route per conversation and injects the credential.
- **Presets** for common servers and vendors are documentation in `docs/catalog/`, copied into the registry, not
  code.
- **Token limits** are `derived` (from the registry or a known table) or `guessed`; budgets treat a guess
  conservatively and the UI shows it.
- **Relay, not translation.** Inside a sandbox the provider speaks the route's own protocol to the relay (`base_url`
  is the relay's URL for the route, the bearer is the runtime token) and sends standard requests without vendor
  settings. The relay pins `model`, merges `extra_body` and `extra_headers`, maps the standard reasoning field to the
  route's own field, injects the credential, forwards to the upstream endpoint unchanged otherwise, and taps the stream
  for usage. Because nothing is translated, protocol features that must round-trip survive: thinking blocks with
  signatures, Responses reasoning items, cache-control markers (ADR-0019). The runtime has no direct-provider mode; in
  tests the relay is the fake gateway from `tqa-testkit`.
- **Usage:** every response carries prompt, completion, cached and reasoning token counts when the server reports them.

## 7. Context management (`tqa-context`)
- Message model: `system`, `user`, `assistant` (text, reasoning, tool calls), `tool` (result), plus a `compaction`
  marker message. The list is a projection of the event log (§5.0).
- Counting: a tokenizer per route where available (tiktoken-compatible or a byte heuristic marked as such).
- **Deterministic slots**, assembled in this fixed order so server prefix caches hit: system prompt, instruction
  files, tool definitions (all byte-stable for the whole run), then the compaction summary, then recent turns, then
  the current step. Budget: `window − reserve_for_output`; a guessed window is treated conservatively. On a route
  with native tool calls the tool definitions travel once, in the request's `tools`, so their slot is empty; they
  are still counted against the budget and stay part of the cache prefix (CTX-U-017, CORE-U-053).
- **Structured compaction**: when the budget is reached, the oldest turns are summarised into one `compaction`
  message with fixed fields (goal, done, pending, decisions, key paths, open errors) rather than free prose. The
  last N turns and every tool result they reference are kept. **Pinned** items are never compacted: the task
  statement, the instruction files and the run notes (§7.1). The summary prompt is fixed and versioned.
- **Dedupe**: a tool result whose content hash equals an earlier result (the same file read twice) replaces the
  earlier copy with a one-line stub pointing at the latest. Small-window local models gain the most from this.
- Tool result cap: `max_bytes` per result (default 64 KiB, overridable per route), and never more than half of the input
  capacity still free when the result is recorded (a floor of 1 KiB keeps the head, the tail and the spill path); the excess spills to
  `/workspace/.tqa/<conversation_id>/spill/<id>` and the message says so with the path and a head and tail preview. A result larger
  than the window is therefore cut and the run continues (CTX-E-004).
- Resume depth for the UI: load the last N turns first, then the rest on demand.

### 7.1 Memory tiers
| Tier | What | v1.0 |
|---|---|---|
| Working memory | The context window above | yes |
| Run memory | The journal, plus a **notes file** `/workspace/.tqa/<conversation_id>/notes.md` the agent writes with `write_notes` and reads with `read_notes`; it is pinned and re-injected after every compaction, so what the agent chose to remember survives | yes |
| Long-term memory | Cross-conversation and cross-workspace memory | v1.1: explicit, inspectable files per workspace and per user, retrieved through a `memory_search` tool, never injected automatically; no vector database in the core |

## 8. Tools (`tqa-tools`)
```rust
#[async_trait]
pub trait Tool: Send + Sync {
    fn spec(&self) -> &ToolSpec;                     // name, description, JSON Schema, parallel_safe
    async fn call(&self, args: Value, cx: &ToolContext) -> Result<ToolOutput, ToolError>;
}
```
| Tool | Notes |
|---|---|
| `read_file` | Line-numbered pages (default 200 lines, and never more than an 8 KiB page so a read fits the model's tool-result budget); `start_line` and `end_line` ranges, trimmed to what fits rather than refused; every partial page ends with one notice saying exactly how to continue, including an opaque stateless cursor for very long single lines (minified or generated files); a cursor it never issued beside a line range is taken for a model's guess, so the lines are read and one line before the closing notice, within the page, says the cursor was ignored (alone, it is refused); files up to 64 MiB are readable page by page; binary files are summarised. Output tokens scale with what the model asks for, not with the file |
| `write_file` | Atomic write through a staged file and rename; creates parents when asked; keeps the mode of a replaced file; refuses outside the workspace (`.git` is part of it, ADR-0035); returns a diff summary |
| `edit_file` | One or more exact search-and-replace edits applied atomically: each `old_text` must match exactly once unless `replace_all`; a miss reports whitespace-only near matches with line numbers so the model can retry; line endings and file mode preserved; returns a bounded diff for the tool card. It is the primary edit primitive because output tokens scale with the change, not the file, and an exact match fails loudly instead of misapplying. A unified-diff `apply_patch` tool is an optional per-route preset later, for models trained on that format |
| `glob`, `grep` | Built on the ripgrep libraries (`ignore` for a parallel walk that respects `.gitignore`, `.ignore` and `.tqaignore`; `grep-searcher` and `grep-regex` for SIMD literal prefiltering, binary detection and context lines). `grep` output modes: `files_with_matches` (cheapest, the recommended first step), `count`, and `content` (line-numbered matches with optional context). Both page under the same 8 KiB budget with a stateless cursor, and stop at scan budgets (files, bytes, elapsed time) with `complete: false` rather than running unbounded. No index in v1.0; revisit if PERF-050 shows a need |
| `shell` | `sh -c` in a resolved directory of the workspace; its own process group; timeout; head-and-tail output cap; scrubbed environment; when the command returns, processes it left running are stopped and the output says so. A `background` mode with `shell_read` and `shell_kill` is Phase 5 |
| `fetch_url` | Goes through the policy proxy; egress allow-list applies; size cap; text extraction; the text comes back in an untrusted-content block the page can't close (ADR-0014 point 10) |
| `skill` | Read-only; loads one skill of the run's byte-stable index on demand (its `SKILL.md` body and up to 100 helper paths under `/skills`); refuses unknown names and folders changed since run start. It lives in `tqa-prompt` beside the skill parser (dependency direction), not in `tqa-tools` (ADR-0029) |
| MCP tools | Discovered at start; names prefixed `mcp.<server>.<tool>`; `stdio` servers run inside the sandbox; remote servers (connectors) are reached through the gateway MCP proxy (§15) |
- `ToolContext` carries the workspace root, the cancel token, a clock and a sleeper, the environment allow-list and the
  limits. Events are the core's job around each call, not the tool's. Every git a tool starts runs with repository
  hooks and the file-system monitor off (§5.4).
- The registry validates arguments against the schema before `call`; unknown tools are a model error, not a crash.

## 9. Wire protocol (TQA protocol v1)
One set of typed JSON frames, defined in `tqa-types` and exported as JSON Schema, carried over **two bindings**:

| Binding | Who | Transport | Direction |
|---|---|---|---|
| **Runtime link** | runtime ↔ gateway | One WebSocket per sandbox, dialled outbound by the runtime with its per-run token | Both ways: server frames up, client actions down |
| **Client binding** | browser, programs, other agents ↔ gateway | Server-sent events for frames from the server; plain POST requests for actions | Stream down, actions up |

Server frames: `session.ready` (no `seq`; it precedes replay, is re-sent once right after replay as the closing
snapshot, and again whenever the sandbox state or queue position changes; state
`creating | ready | busy | idle | queued | parked | destroyed | failed`, and while `queued` an optional
`queue { ahead }` - how many waiting runs are served before this one, 0 = next; the gateway computes it, §4.4; `parked`
is retired, ADR-0035), `event` (always `seq`), `error`
(`seq` when the error concerns a specific frame). Event types (`event.type`): `message.user` (the accepted user
message, so history and the transcript are complete from events alone), `run.started`, `context.loaded` (what the run loaded into the model
context: instruction files, skills and skipped skill folders; for the context panel), `step.started` (with
`attempt`, so a retried step supersedes its earlier partial output), `text.delta`, `reasoning.delta`, `tool.call`,
`model.completed` (the response for this attempt fully arrived), `tool.start`, `tool.output` (chunked, what the user
sees), `tool.end` (with a failed call's `failure`: its code and, for a refused destination, the target, so a client
words it itself; revision 10), `tool.result` (exactly the content appended to the model context, after capping),
`step.completed` (every call of the step has a result; carries the step's `duration_ms`), `compaction`, `usage`,
`budget.warning`, `provider.retry` (a model attempt failed and another follows), `decision`, `run.completed`, `run.cancelled`, `run.failed`, `run.resumed`, `sandbox.state`, `environment.changed` (the conversation's template changed, §4.7).

Link-only frames, never on the client binding: gateway to runtime `session.config` (the conversation, the route
snapshot without secrets or vendor settings, the profile, the environment (template, digest, capabilities, §4.7), the
limits, the task's network allow-list for the prompt, the last stored `seq`), sent when a link
registers; gateway to runtime `session.update { route }` when the user picks another model while no run is active (the
runtime replaces its route snapshot and uses it from the next run on; on a redial the route in `session.config` is
likewise adopted for later runs, so an update missed while the link was down is not lost, while every other frozen
field must still match); runtime to gateway `heartbeat { run_id }` from the run supervisor every 30 s while a run is active,
unsequenced and never stored. The link refuses the retired frames from a runtime before storing anything
(`permission.request`, `permission.resolved`, `permission.expired`, `run.parked`, an `error` with `permission_binding`)
and closes as forbidden: they are read only from histories written before ADR-0035.

Client actions (the same payload types on both bindings):

| Action | Runtime link frame | Client binding |
|---|---|---|
| Send a message | `user.message` | `POST /conversations/{id}/messages` |
| Cancel a run | `run.cancel` | `POST /conversations/{id}/runs/{run_id}/cancel` |
| Open a stream | `session.init` (`conversation_id`, `last_seq`, `protocol`) | `GET /conversations/{id}/events?stream=1&protocol=1` with `Last-Event-ID`, or `&after=<seq>` when the client opens the stream itself |

Rules:
- `seq` is assigned by the runtime per conversation and verified contiguous by the gateway. A gap closes the link or
  the stream with `error.code = "seq_gap"`; the runtime redials, and a client reconnects with its `Last-Event-ID`.
- On the client binding, `id:` of every server-sent event is the `seq`; reconnecting with `Last-Event-ID` replays
  from PostgreSQL then continues live, so every event is delivered exactly once. A client that opens a stream itself,
  for example after it detects a gap, passes the same cursor as `after=<seq>`, because a browser cannot set the header
  on a new connection; the header wins when both are present.
- Frames have a size cap; `tool.output` is chunked. The stream carries comment keep-alives every 15 s.
- The protocol version is negotiated on both bindings; unsupported versions get `error.code = "unsupported_protocol"`.
- Actions carry an idempotency key; a repeated POST returns the first result.
- On resume after a sandbox died, every journaled `tool.result` has its workspace changes on disk (§4.5,
  `sync_before_result`); only the interrupted call's writes may be missing.

## 10. Gateway (`tqa-gateway`)
- **Auth:** email and password with argon2id; session cookies (HttpOnly, SameSite=Lax) for the browser; personal
  API tokens for programs and other agents; roles `admin`, `member`; invitations by link. OIDC is later.
- **Data model (PostgreSQL):** `users`, `sessions`, `invitations`, `workspaces` (owner, name, host source, quota,
  size, lease holder and expiry, retention), `workspace_shares` (workspace, user or team, access), `conversations`
  (owner, workspace, profile, model, backend binding, state), `channels`, `channel_members`, `channel_messages`
  (`channel_id`, `seq`, unique), `channel_claims`, `sandboxes` (handle, state, last_active, run_id), `events` (`conversation_id`, `seq`, `type`,
  `payload` jsonb or object key, unique on `(conversation_id, seq)`), `messages` (derived view for listing),
  `usage` (per run and route), `settings`, `secrets` (provider keys and connector credentials, shared or per user,
  encrypted), `connectors`, `agent_profiles`, `templates`, `egress_rules` (the approval and rule tables were dropped by
  ADR-0035).
- **API parity:** the web UI calls only these documented endpoints; a contract test fails when the web client uses a
  route that is not in the OpenAPI file (GW-U-027). Personal API tokens give programs and other agents the same access.
- **HTTP API:** `/api/v1/auth/*` (setup signs the first administrator in with the same session cookie as login;
  unauthenticated `GET /api/v1/setup` returns `{ "required": bool }` so the web can send a fresh install to setup), `/users`, `/invitations`, `/conversations`, `/conversations/{id}/events?after=`,
  `/conversations/{id}/files` (tree, read, diff), `/conversations/{id}/git/push`, `/workspaces` (list, create, fork, share, delete), `/channels` (create, members, messages since, wait, claim),
  `/models`, `/profiles`,
  `/connectors`, `/templates`, `/admin/*`,
  `/healthz`, `/readyz`, `/metrics`.
- **Runtime link:** `/runtime/link` (WebSocket, dialled by the runtime at start with its per-run token). The gateway
  registers the sandbox as ready, receives every server frame, persists it, fans it out to subscribers, and sends
  client actions down the same socket. If the link drops, the runtime redials with back-off and delivers the frames
  it buffered meanwhile in order.
- **Client streams:** `GET /conversations/{id}/events?stream=1` and `GET /channels/{id}/events?stream=1` are
  server-sent-event streams; `id:` is the `seq`; on `Last-Event-ID` the gateway replays from PostgreSQL then goes
  live. Any number of subscribers per conversation.
- **Client actions:** `POST /conversations/{id}/messages`, `POST .../runs/{run_id}/cancel`,
  `PATCH /conversations/{id}` (`{ title?, route? }`: rename, or the model for the
  next message; a route change during an active run is refused with `run_active`, and a live link receives `session.update`),
  each with an idempotency key; the gateway forwards them as protocol frames
  down the runtime link and ensures a sandbox exists first (create, wake or queue, §4.4).
- **Runtime tokens:** minted by the sandbox manager for each sandbox instance, stored hashed, revoked when the sandbox
  is destroyed, and accepted only from that sandbox's network address. A token authorises the link, the relay, the
  MCP proxy, the channel tools and `GET /runtime/history` for one conversation.
- **LLM relay:** `/relay/r/{route}/...`, bearer = runtime token. Checks that `{route}` is the conversation's route and
  allowed for the user; allows only the protocol's completion endpoints; pins `model`; merges the route's `extra_body`
  and `extra_headers`; maps the reasoning field; injects the credential from the secret store; forwards and streams
  back without translation; taps usage for metering (a reply that ends before the server reports usage, such as a
  step stopped mid-reply, is metered by the relay's estimate of what streamed and the request it sent, marked
  `estimated`); applies per-user rate limits (ADR-0019). The runtime's
  `X-TQA-Run-ID` attributes the call to its run, for the usage meter and the secret-read audit.
- **Secret store:** one built-in store (ADR-0033): values encrypted with AEAD (XChaCha20-Poly1305) in PostgreSQL
  under an install key held in a private file outside the database. The `SecretStore` trait is the internal boundary
  that keeps consumers unaware of storage, not an extension point. The gateway is the only reader, values are
  write-only from the UI and API, and nothing reaches a sandbox.
- **MCP proxy:** `/mcp/{connector}` (streamable HTTP), bearer = runtime token; resolves the connector, checks the
  conversation's profile allow-list, injects the credential from the vault (per-user first, then shared), forwards to
  the remote MCP server and streams back. The sandbox never receives the credential. There is no per-call check and
  no call-context header (ADR-0035); the connector's own tool allow or deny filter still applies.
- **Git:** no relay (removed, ADR-0037 §7). A task clones and pushes with plain git through the policy proxy, which
  swaps the person's key for the git host in like any other keyed host; the git host's rules decide.
- **Policy proxy:** an HTTP CONNECT and forward proxy for sandbox egress, on a gateway port reachable only from the
  internal sandbox network (never published). It identifies the caller by the transport address bound to a live runtime
  token, as the relay does (an optional `Proxy-Authorization: Bearer` from the runtime must match the same binding); no
  token is ever placed in an environment variable. Rules: host (exact, or `*.domain` for subdomains only) and ports,
  default deny; until profiles carry their own list (Phase 6) one `[egress]` list in gateway.toml applies to every
  conversation. The proxy resolves the host itself and checks every resolved address: loopback, private, link-local,
  metadata and the gateway's own addresses are refused unless a rule names that literal address; it connects to the
  address it checked (no second resolution, so DNS rebinding cannot bypass it). A denial answers `403`, logs one line
  (conversation, host, port, decision) and sends one unsequenced `error { code: egress_denied, target: {host, port} }`
  on the conversation's client stream, at most once per host and port per minute; the web writes its own copy from the
  code and target.
- **Reaper and reconciliation:** a periodic task applies the rules in §4.4; on start-up it lists backend sandboxes,
  reconciles with the table, and destroys orphans.
- **Metrics:** Prometheus counters and histograms for runs, events, relay latency, provider errors, sandbox states.

## 11. Web UI (`web/`)
- Next.js (App Router), React, TypeScript, Tailwind, TanStack Query for HTTP, an `EventSource` store for the live
  transcript, MIT-licensed component primitives. No proprietary UI kit.
- Served through the gateway (ADR-0022). The Next.js server renders the application shell only and never receives
  cookies or authorization headers; the browser fetches every piece of data from the same-origin API.
- Pages: `/setup` (the first administrator), `/login`, `/join/{invite}`, `/` (conversation list), `/c/{id}` (transcript, composer, sandbox
  and workspace status, files and diff panel), `/workspaces` (list, create, fork, share, delete), `/settings`
  (profile, tokens; for administrators also users, status, usage, secrets, models, egress, limits, connectors and templates as
  `/settings/<area>` in the same shell, backed by `/api/v1/admin/*`; ADR-0028). A new conversation
  starts composer-first: `/` opens an empty conversation with the composer focused; the model, the environment, and later the
  profile and the workspace (existing, new, from git, or fork), are pickers in the conversation bar above the
  transcript, never a modal and never inside the composer. The
  conversation is created when the first message is sent, titled from that message (first line, at most 60
  characters) and renamable inline in its header; the conversation list lives in the sidebar.
- Transcript rendering: text and reasoning streams append per `seq`; tool cards show command, streamed output, exit
  code and diffs; compaction and resume markers are visible; virtualised list for long transcripts.
- Markdown: assistant text is rendered as CommonMark with the GitHub extensions for tables, task lists and
  strikethrough, through an allow-list of elements. Raw HTML in model output is shown as text, never parsed; links get
  `rel="noopener noreferrer nofollow"` and open in a new tab; images are not loaded from remote hosts; fenced code
  blocks keep their language label, scroll horizontally and have a copy button. A partial stream renders safely at
  every delta (an unclosed fence or list is displayed as it stands, then re-rendered as it completes).
- Follow mode: while the reader is at the bottom of the transcript, new content keeps the latest line in view. When
  the reader scrolls up, following pauses and a "Jump to latest" control shows how much new content arrived; using it
  or scrolling back to the bottom resumes following. Sending a message always resumes following. Scrolling respects
  `prefers-reduced-motion`.
- Reconnect: `EventSource` reconnects by itself and sends `Last-Event-ID` (the last `seq`); the store dedupes by `seq`.

### 11.1 How a run reaches the screen
1. **Core → runtime, in process:** `tqa-core` emits typed Rust events (`enum Event`, `#[serde(tag = "type")]`) into
   an `EventSink`; the runtime assigns `seq`, appends to the journal, then sends.
2. **Runtime → gateway, WebSocket (outbound):** the runtime holds one socket to the gateway. The gateway persists
   every server frame in `events` and fans it out to every subscriber of that conversation.
3. **Gateway → browser, server-sent events:** `GET /conversations/{id}/events?stream=1`; each event's `id:` is the
   `seq`. The browser's `EventSource` reconnects on its own and sends `Last-Event-ID`, so replay is the standard
   mechanism. User actions are ordinary POSTs. Server-sent events fit this hop because the traffic is a dense stream
   down and a few actions up, it is plain HTTP (cookies, proxies, HTTP/2 multiplexing, any HTTP client), and it
   needs no second protocol in the product.
4. **Browser:** the Next.js server renders the application shell; the browser loads the conversation list through
   TanStack Query and opens the conversation's event stream, which replays the stored history from the start and then
   continues live (a paged history read before the stream is a later optimisation). A reducer
   `apply(event)` builds the view model: messages, tool cards keyed by call id, run state, sandbox state, slots,
   usage. Text and reasoning deltas are coalesced and flushed once per animation frame, so a fast model does not
   cause a render per token. The list is virtualised.
5. **Types:** `schemars` exports JSON Schema from the Rust enums; the web build generates TypeScript unions from it
   (gate G-08), so a new event type is a compile error in the client until it is rendered.
Operational notes: reverse proxies must not buffer the stream (one header in the install guide); the gateway serves
HTTP/2 so many streams share one connection and the browser's per-origin connection limit does not bite.
Any number of viewers subscribe to the same stream; the gateway broadcasts, and the runtime never knows how many
clients exist.

### 11.2 Presenting the event stream
The wire carries every event; the screen shows a small, readable story. A pure presentation layer
`present(transcript) -> view` sits between the reducer and the components, so each event type has exactly one declared
treatment and a new event type is a compile error until it has one (the same exhaustive switch as the reducer).

| Treatment | What the reader sees | Event types |
|---|---|---|
| **Answer** | The assistant's reply, prominent, rendered as Markdown; the final answer of a run is the visual anchor of that turn | `text.delta` of the authoritative attempt |
| **Prompt** | The user's message, right-aligned | `message.user` |
| **Activity** | One line per tool call inside the turn's collapsible *activity group*: a verb and the key argument ("Created `hello.py`", "Ran `python3 hello.py`", "Read `src/main.rs` lines 1–200", "Searched for `TODO` in `src/` · 12 matches"), a status mark only when not successful, and the duration on hover. A line opens into the full tool card (arguments, streamed output, exit code, diff) | `tool.call` with its `tool.start`, `tool.output`, `tool.end`, `tool.result` |
| **Thinking** | Collapsed "Thinking" disclosure in the activity group, quiet type | `reasoning.delta` |
| **Status** | The header pills (run, sandbox) and, only when the reader must know, one quiet inline line ("Model request failed; retrying in 2 s", "Approaching the step budget", "Resumed after the sandbox restarted") | `run.started`, `run.completed`, `run.failed`, `run.cancelled`, `run.resumed`, `run.parked`, `sandbox.state`, `environment.changed` ("Switched to Workstation"), `provider.retry`, `budget.warning`, `compaction`, `decision`, `permission.request` (until approvals, Phase 3; then an inline approval card, task 043), `permission.resolved` (quiet "Allowed" / "Denied" on that card; until 043 "Approval decision recorded"), `permission.expired` ("Approval timed out - the agent continued without it") |
| **Summary** | Under a finished turn: duration, steps, files changed, commands run, tokens; failures state the reason and the next step | derived from `step.completed`, `usage`, `tool.result`, terminal events |
| **Hidden** | Nothing; used only to structure the view | `step.started` (attempt boundaries), `model.completed`, `step.completed` |

Rules:
- While a run is active, the activity group is open and its last line is live ("Running `cargo test`…"); when the run
  completes, the group collapses to one line ("Worked for 38 s · created 2 files, ran 3 commands") above the answer,
  and one click reopens it. A failed or cancelled run keeps the group open at the failing line.
- Text from a step that also calls tools is **narration** ("I'll run the tests next"): it shows in the activity group
  between the lines it introduces, in regular body type, and folds with the group. Only the text of the run's last step
  (no tool calls) is the Answer below the group. While a step streams, its text shows live in the group until the step's
  tool calls or its end decide which it is.
- A superseded attempt (§5.5) disappears from the view; its partial text is never shown next to the retry.
- The activity line text comes from a per-tool summary function registered with the tool's name (`write_file`,
  `edit_file`, `read_file`, `grep`, `glob`, `shell`; MCP tools get a generic "Used `server.tool`"), with bounded
  arguments and no secrets; unknown tools fall back to the generic form.
- Nothing is lost: every event remains reachable from the view (the tool card, the thinking disclosure, the status
  line), and an admin "raw events" view for debugging shows the stream as stored.

## 12. Security model
| Threat | Control |
|---|---|
| Model or repository content instructs the agent to exfiltrate data | Egress allow-list (host and port, default deny) enforced outside the sandbox; keys never inside. **Accepted** (ADR-0035): data can reach an allowed host without a person seeing it, so lists stay narrow |
| Code in the sandbox inspects or traces another sandbox process (ptrace of the runtime) | Accepted within the sandbox: every process there already has the sandbox's full power, so same-uid tracing crosses no boundary (debuggers work). Held outside it: no capabilities, `no_new_privs`, seccomp, a private PID namespace (nothing outside is visible or traceable), no secrets inside, and egress and model access enforced by the gateway (SBX-C-007) |
| A push leaves under another call's authority, or twice | Given up knowingly with the Git relay (ADR-0037 §7): a push is plain git with the person's key, and the git host's branch protection and the key's scope decide what it may do (ADR-0035 §5) |
| Tool escapes the workspace | Path canonicalisation with symlink resolution against the workspace root; read-only rootfs; unprivileged user |
| One user reads another's conversation | Authorisation per conversation on every stream, action and API entry; sandboxes on separate networks; per-run tokens scoped to one conversation |
| Sandbox attacks the host | Docker: dropped capabilities, seccomp, no host mounts, resource limits. Firecracker: KVM boundary, then the per-VM container (non-root VMM uid, no capabilities, seccomp, cgroups) |
| A web page opened by the agent's headless browser attacks the environment | Chromium runs with its inner sandbox off, because enabling it would require loosening the container (user namespaces); the container or microVM stays the boundary with every restriction (non-root, read-only root, no capabilities, seccomp, limits), and a sandbox reaches only what the allow-list allows (§4.2, §4.7) |
| Secrets in logs | Structured logging with redaction for headers and known key shapes; prompts and tool output not logged by default; secrets never enter third-party request bodies (the runtime token travels as a mounted file) and the `log` facade is capped at info in release builds |
| Secret store compromise or outage | The install key lives outside the database (private file, rotated with a documented procedure), so a database copy alone reveals nothing; values are authenticated to their reference; a store that cannot be read fails new runs closed (GW-I-029) |
| One user's sandbox reaches another user's workspace | A sandbox mounts only its bound workspace; workspace ids are opaque and validated; bindings are checked at every entry (SEC-014, SBX-C-016) |
| Agent plants code that runs on the host through git (hooks, `core.fsmonitor`, `core.sshCommand`, filter and diff drivers in `.git/config` or `.gitattributes`) | The gateway never runs git or any other tool against a workspace on the host; clone, commit and push run inside a sandbox for that workspace. The file tools may write in `.git` (ADR-0035): it's the agent's own repository |
| In a shared workspace, one member's agent plants a hook or `core.fsmonitor` that runs in another member's task | Every git in the sandbox runs with repository hooks and the file-system monitor off through environment configuration (SEC-045). **Accepted:** repository config git can't switch off (filters, aliases, `core.sshCommand`, diff drivers) means sharing a workspace trusts the other member's agent |
| A tool process reads the runtime token (same uid as the runtime) and tries to use it elsewhere | The token is scoped to one conversation and one sandbox instance: minted per sandbox creation and revoked on destroy, and accepted only from that sandbox's network address; egress is limited to the gateway, so an exfiltrated token is useless outside the sandbox |
| Data leaves through DNS lookups | Sandboxes have no external DNS; the gateway is resolved by the engine's embedded DNS or a static entry, and the policy proxy resolves allowed names on the gateway side (ADR-0021) |
| An agent in a shared workspace rewrites another conversation's history | History is authoritative at the gateway and append-only; the write-ahead log lives in a per-conversation state directory outside the workspace that no other sandbox mounts (ADR-0020) |
| A connector credential reaches the sandbox | Credentials live only in the gateway vault; connectors are reached through the MCP proxy with the per-run token; templates and skills cannot add secrets (SEC-013) |
| Denial of service by one user | Per-user limits: live sandboxes, run wall-clock, relay rate; global reaper |
| Supply chain | Pinned toolchains and dependencies; `cargo deny`; image digests; SBOM in releases |

## 13. Observability
- `tracing` spans per run, step and tool call with the conversation id; JSON logs; OpenTelemetry export optional.
- Prometheus: `tqa_runs_total{result}`, `tqa_step_duration_seconds`, `tqa_relay_latency_seconds{route}`,
  `tqa_provider_errors_total{route,code}`, `tqa_sandboxes{backend,state}`, `tqa_event_lag_seconds`.
- An admin diagnostics page (backed by `/readyz` detail) checks the backend, images, KVM, model endpoints and the
  database.

## 14. Configuration
- One validated TOML per server component (`gateway.toml`, `models.toml`); environment variables only for secrets and
  the config path. Unknown keys are errors. Defaults are documented in the admin guide.
- The runtime has no configuration file. It reads three non-secret environment variables (`TQA_GATEWAY_URL`,
  `TQA_RUNTIME_TOKEN_FILE`, `TQA_STATE_DIR`), plus `TQA_EGRESS_PROXY_URL` which the sandbox sets only in proxy mode (a
  credential-free origin on the gateway's host), and receives everything else in `session.config`, so a sandbox image
  never carries deployment settings (the network allow-list for the prompt among them, ADR-0035). The runtime passes the
  proxy explicitly to shell children (`HTTP(S)_PROXY`, lowercase forms, `NO_PROXY` for the gateway and loopback, so a
  task reaches its own local servers, TOOL-U-060); `fetch_url`
  authenticates to the proxy with the token from the file, never from an environment value.
- Key settings: `storage.root` (local disk directory), `workspace.mode` (`bind` or `volume`, Docker only),
  `workspace.quota`, `workspace.retention_days`, `workspace.busy_policy` (`wait` or `refuse`), `runtime.backend`
  (`docker` or `firecracker`).
- Compose files: `deploy/compose/docker.yml` (default) and the Firecracker host script `deploy/firecracker/setup.sh`.

## 15. Extension model
The core is a kernel: `tqa-core` (loop), `tqa-types` (protocol), `tqa-sandbox` and `tqa-runtime` (isolation) and
`tqa-gateway` (control plane). Everything else plugs in at one of these points. The rule for every point: an
extension runs **inside the sandbox without secrets** or **outside the sandbox behind the gateway with credentials in
the vault**; there is no third place.

| Extension point | Plugs into | Mechanism | Trust position |
|---|---|---|---|
| Skills and instruction files | `tqa-prompt` | Files discovered at run start; shown in the context panel | inside, no secrets |
| Tools | `tqa-tools` MCP client | `stdio` MCP server started in the sandbox from the template or the workspace | inside, no secrets |
| Connectors | gateway MCP proxy → `tqa-tools` MCP client | Remote MCP server registered at the gateway; the runtime sees `https://gateway/mcp/{connector}` and the per-run token only | outside, credentials in the vault |
| Agent profiles | gateway `agent_profiles` → `SandboxSpec` and the runtime's run configuration | Validated record: instructions, model route, allowed tools and connectors, skills, template, budgets | configuration |
| Sandbox templates | `tqa-sandbox` image resolver | OCI image derived from the base, registered by digest; exported to a rootfs for Firecracker | inside; hardening posture is set by the backend, not the template |
| Model routes | `tqa-llm` registry and the gateway relay | `models.toml` or admin records | outside, keys in the vault |
| Decision engine | `tqa-core` `DecisionEngine` (§5.1a) | Default in the core; an external service over `decision/v1`, an engine record of its own (`[[decision_engines]]`, the admin API) reached through the gateway's relay | outside, credentials in the vault |
| Hooks (v1.1) | `tqa-core` lifecycle points | Script inside the sandbox, or HTTP call made by the gateway | either, by design of the hook |
| Runtime backends, provider protocols | `SandboxBackend`, `ModelProvider` traits | Rust implementation, contributed with an ADR | core |

Profiles are the unit of composition: a conversation is created from a profile, and an agent team's members (v1.1,
§16.2) inherit their lead's. No in-process plugin API exists; MCP is the plugin protocol.

### 15.1 Contributing an extension
Each extension point is a versioned contract with a conformance suite. `tools/ext-check <kind> <target>` (developer
tooling in the repository, not a product) runs the suite locally; CI runs it for contributions. Passing the suite is
the acceptance evidence.

| Kind | Contract | Conformance suite | Where it lives |
|---|---|---|---|
| MCP server (tool or connector) | The MCP specification plus this project's naming rules | Discovery, schema validity, error mapping (TOOL-I-012/013/018) | any repository; listed in `docs/catalog/` |
| Sandbox template | Derived from the base image; no secrets, no privileged settings, runtime binary intact | SBX-C-014 posture checks and boot to ready in both modes | any repository; registered by digest |
| Runtime backend | `SandboxBackend` trait | The whole SBX-C suite plus the security suite | this repository, with an ADR |
| Provider protocol | `ModelProvider` trait | LLM-C fixtures for request shaping, streaming and usage | this repository, with an ADR |
| Decision engine | `decision/v1` protocol | `ext-check decision`: the runtime's relay client asks canned consultations at both points and every exposure, and the loop's own checks (CORE-U-063) judge each answer (TK-U-030) | any repository; configured as a decision engine |
| Agent profile, skill | Profile schema; `SKILL.md` schema | Schema validation and a dry run | any repository; shared as files |

Contracts change only additively within a version; a breaking change bumps the contract version and keeps the
previous one for one release, like the wire protocol.
Consumers are forward compatible (24 Sep 2026): the web client and other API clients ignore response fields, event
fields and event kinds they do not know, and stay strict about the ones they do (required, types, bounds). Producers
validate what they accept strictly (unknown fields in requests and inbound frames are refused). So an additive field
never breaks an older client. The runtime is the exception: its configuration frames (`session.config`,
`session.update`) come across a trust boundary and are parsed strictly, so version skew there is handled by the
protocol revision (ADR-0026): the runtime sends `TQA-Protocol-Revision` on the link, and the gateway refuses a runtime
below its floor with `426 runtime_too_old` before any frame. Rule: a change that makes the gateway send something an
older runtime cannot parse raises `PROTOCOL_REVISION`, adds its row to `PROTOCOL_REVISIONS` and changes the runtime
image label in the same commit.

## 16. Agent communication: channels and agent teams
**Principle: agents communicate through a shared, ordered, persistent channel, a forum, never point-to-point.** A
channel is the same kind of thing as a conversation's event stream: an append-only log at the gateway with
`(channel_id, seq)`, a member list and a small set of structured message types. Humans read and write the same
channel from the web UI. Voting, decisions and task claiming fall out of that with no message broker, no new wire
protocol and no direct agent-to-agent connections. Agent teams (v1.1, §16.2) address each message (to the lead, one
member or all members) on the same storage; that is still delivery by the gateway, not a connection between agents.

### 16.1 Channel
- **Entities:** `channels` (id, owner, purpose, decision rule, budget, state), `channel_members` (a conversation or a
  user; role `lead`, `member` or `observer`), `channel_messages` (seq, author, type, body, parent seq, refs),
  `channel_claims` (task seq → claimant, atomic).
- **Message types** (schema in `tqa-types`, versioned like the wire protocol): `text`, `proposal`,
  `vote {proposal_seq, choice, reason}`, `decision {proposal_seq, outcome, rule}`, `task {title, spec, claims}`,
  `claim {task_seq}`, `report {task_seq, status, summary, refs}`, `done` (`custom {schema_id}` is post-v1.0).
  `parent` makes threads.
- **Agent side:** four builtin tools: `channel_post` and `task_claim(task_seq)` (not parallel-safe),
  `channel_read(since, filter, limit)` and `channel_wait(since, filter, timeout)` (the read class). A run may use a
  channel while its conversation is a member (ADR-0014); it gets the tools when it is a member of some channel at run
  start, unless the profile's `tools` allow-list leaves them out. They call `/runtime/channels/{id}/...` with the per-run token, bound to the sandbox's address; the
  gateway sets the author from the token, never from the body. `channel_wait` is a long poll of at most 300 s, so
  waiting costs no tokens; in v1.0 the run stays active and its sandbox busy while it waits (parking a waiting run is
  later). `task_claim` is atomic up to the task's `claims` limit (1 by default).
- **Human side:** the channel page shows threads, proposals with tallies, decisions, the task board and each
  member's status. Humans post, vote, veto or approve, and can end the channel. The page subscribes over
  `GET /channels/{id}/events?stream=1` (server-sent events) with the same `event` shape and `Last-Event-ID` replay
  as conversations.
- **Decisions:** a proposal opens a window that closes when every voting member has voted or a timeout passes. The
  gateway evaluates the channel's rule and posts the `decision`: `human` (a human approves or rejects), `majority`,
  `quorum(n)`, `lead` (the lead's vote decides). Rules are data. Agents waiting on the decision continue. A new
  rule is an extension: the decision engine seam (§5.1a) can evaluate proposals too.
- Everything is an event: persisted, replayable, budgeted and metered like a conversation.

### 16.2 Agent team (v1.1)
ADR-0034 decides the design; this is its outline. An **agent team** is one lead and its members, run the way a
technical lead runs a few developers: the lead writes each member's brief, members report only to the lead, and the
lead decides and announces. In the product, members are shown as **helpers**.
- **No setup by a person.** A person starts one normal task and asks it to lead. The lead starts members with
  `start_member(role, brief, workspace, profile?)`. Each member is a normal task, from an agent profile the lead names
  (its tools and connectors narrowed to the lead's) or with the lead's own settings, never wider than the lead, grouped
  under the lead in the task list. Limits are admin settings: members per lead (3 by default, at most 8, never more
  than the slots minus one), a token budget, a deadline and the idle times. Members can't start members (depth 1).
- **Enforced by the gateway.** Roles are stored when a member starts, and no agent can change them. Tools differ by
  role:
  - member: `send_to_lead` and `wait_for_lead`;
  - lead: `start_member`, `assign`, `ask_all`, `read_reports`, `decide`, `done`, `take_files`, `stop_member`,
    `ask_person` and `read_helper`;
  - a task that isn't a lead yet: `start_member` alone, with the other nine from its first successful call on.

  The whole feature sits behind the install switch `[agent_teams] enabled`, off by default.

  The per-run token says who calls, and the gateway delivers each message only to its addressee.
- **Messages on the channel's storage.** An agent team owns one channel of kind `agent_team`, with a seq per agent
  team and an addressee per message. A reader gets what was addressed to it after its stored last-read seq, so
  nothing is lost or doubled across restarts and sleeping sandboxes. A send is stored or refused with a reason. A
  message for a conversation whose run isn't active wakes it with a queued run, so waiting costs no model tokens.
  Every wait is a long poll of at most 30 s that gives way at once to a run queued for a slot, and the member cap
  stays below the admission slots, so a lead and its helpers can't deadlock on slots.
- **No votes between agents.** The lead decides (`decide`). People can read everything, overrule a decision by telling the lead, and stop a
  member or the whole agent team.
- **People.** Every member's transcript streams live on its own page, like any task. A person can type to the lead
  or to any member, and the lead is told of a message to a member. When the agent team needs a person, the lead asks
  (`ask_person`) and its task shows "Waiting for you". Members ask the lead, never the person directly.
- **Workspaces.** The write lease (§4.6) is unchanged. The lead chooses per member:
  - `own_copy` (default): a fork, worked in parallel; result files come back with the member's report through the
    gateway, and the lead places them with `take_files`, by the attachments path (FR-36, ADR-0030);
  - `my_files`: the lead's own workspace, taking turns under the lease.

  No git is required.
- **Reliability.** A budget and a deadline are enforced at the relay. The gateway sends the lead a notice when a
  member is idle, stuck or failed. The agent team stops on the lead's `done`, a person's Stop, the budget, the
  deadline or an idle time; member runs are cancelled and every task becomes an ordinary task again.
- **Orchestration** is data (`agent_teams`, `agent_team_members`) plus one reconciler loop over existing actions. No
  broker, no new protocol and no agent-to-agent links.

### 16.3 Phasing
Channels, the four tools, the `human` and `majority` rules and the channel page are v1.0 work (Phase 7). Agent teams
are v1.1 (DEV-PLAN Phase 10): they reuse the channel's storage, stream and untrusted marking, not its votes or
claims. Swarms are a later phase (Phase 12, §16.4).

### 16.4 Swarms (later, ADR-0038)
A **swarm** is agent teams working on one goal, coordinated one level up the way a lead coordinates its helpers.
ADR-0038 decides the design (accepted, built in DEV-PLAN Phase 12); this is its outline. The Product Owner set its scope
on 2 Oct 2026:
- **One person's own teams only.** A swarm is made of one person's lead tasks, and nobody else's.
- **Each part of a goal is a new lead task from a profile the person picks**, their own or one an administrator
  shared (ADR-0012). Each lead then runs its own team as in §16.2. Or the coordinator proposes the parts and the
  person confirms them.
- **One coordinator** talks to the leads; leads talk only to the coordinator, inside their goal, while it runs;
  helpers never leave their team. Only messages and their attached files cross teams. One budget and deadline cover
  the goal; there are at most two levels (coordinator, lead, helper) and no cycles.
- **No publishing and no sharing between people.** A swarm, its teams and their profiles aren't offered to other
  people or other installations.

It reuses the agent-team mechanics: messages are addressed to a conversation and delivered by the gateway, so a lead
writing to the coordinator is the same operation as a helper writing to its lead, on the same storage, with roles and
tools enforced by the gateway in the same way.

**Where a swarm's messages are seen** (owner thread, 2 Oct 2026; UI-GUIDELINES §13c). A swarm serves one **goal**. The
coordinator talks to a lead task per part. All of it is one stored, addressed log per goal, on the same storage as an
agent team's, and it is deleted with the goal. - **The goal page:** the goal, its parts, the budget used and the time
left. It has a row per team (its part, the   lead's state, the helper count, what it has used), and the goal's
conversation: every message between the   coordinator and a lead, in order, filterable by team. The person can type to
the coordinator there. On every page, the message box talks to that   page's agent: the goal page to the coordinator,
a lead's page to that lead (the coordinator is told), a helper's   page to that helper (its lead is told). - **Each
lead's page** shows the goal messages that lead sent or received, as quiet lines, as its team messages are   shown. -
**The task list:** the goal is one item, its teams are under it, and each team's helpers are under that.

## 17. What we deliberately do not build yet
This is v1.0's list. Since it was written, agent teams (experimental, off by default) and computer use were built and
are in release 17.0 (PRD §6).
- Agent teams (v1.1, §16.2) and swarms (later, §16.4); the channel primitive itself is in v1.0; cross-conversation memory; hooks; computer use (a later phase, §4.7; headless browser automation through the Workstation template's tools is in v1.0); built-in
  connector code; an in-process plugin API; billing; Kubernetes; OIDC; multi-organisation; GPU pass-through. Each is
  listed in PRD §6 and needs a PO decision.

## 18. Decision records to write (`docs/adr/`)
| ADR | Decision |
|---|---|
| ADR-0001 | Agent loop inside the sandbox with an LLM relay (choice A) versus loop in the gateway with remote tool execution (choice B) |
| ADR-0002 | Rust for the gateway (PRD §12 decision 1) |
| ADR-0003 | PostgreSQL only; sqlx with compile-time checked queries; forward-only migrations |
| ADR-0004 | Egress enforcement: policy proxy plus network rules; what is best-effort on Docker Desktop |
| ADR-0005 | Event sequence assigned by the runtime, verified by the gateway; replay from PostgreSQL |
| ADR-0006 | Firecracker rootfs pipeline (OCI → ext4), kernel pinning, and the snapshot stretch goal |
| ADR-0007 | Tool-call fallback grammar and repair rules |
| ADR-0008 | Prepared-action approval binding (superseded: approvals by ADR-0035, the push's binding by ADR-0037 §7) |
| ADR-0009 | Compaction strategy and the fixed summary prompt |
| ADR-0010 | Web protocol types generated from JSON Schema |
| ADR-0011 | Extension model: MCP as the plugin protocol, no in-process plugin API, the two trust positions |
| ADR-0012 | Agent profiles (accepted): one `agent_profiles` record (instructions, route, template, tools, skills, servers, connectors, policy, limits) that only narrows the install; personal or shared (administrators; files seed shared only); each conversation keeps a validated copy re-checked every session; instructions, tools and skills reach the runtime in protocol revision 8; the profile's policy and rules were removed by ADR-0035 |
| ADR-0013 | Decision engine seam: the two consultation points, the `decision/v1` protocol, fallback and permission narrowing |
| ADR-0014 | Agent communication through channels: message schema, decision rules as data, atomic claims, no point-to-point messaging |
| ADR-0015 | Client transport: server-sent events plus POST for browsers and programs; an outbound WebSocket runtime link; no listener inside the sandbox |
| ADR-0016 | Activeness defined by lifecycle events, never by processes; sandbox admission with slots, queue, eviction, parking and the keep-alive pin |
| ADR-0017 | Dependency licence policy: permissive by default, MPL-2.0 unmodified, CC-BY-4.0 and CDLA-Permissive-2.0 for data, no LGPL, notices regenerated per release |
| ADR-0018 | The per-run token is delivered to a sandbox as a read-only mounted file, never as an environment value or request-body field |
| ADR-0019 | The relay is a transparent authenticated proxy per route: model pinning, vendor settings and credentials applied at the gateway, no protocol translation |
| ADR-0020 | History is authoritative at the gateway; the runtime keeps a write-ahead log in a per-conversation state directory outside the workspace; the resume point rules |
| ADR-0021 | Sandbox networking: an internal network only, the gateway as the sole peer, no external DNS; a development-only host-gateway mode |
| ADR-0022 | The gateway is the single public origin: it serves the API and streams and forwards page requests, without credentials, to an internal web server |
| ADR-0023 | Pushing from the workspace: a dedicated `git_push` tool, create-new-branch only, allow-once on every push under every policy, the provider credential held by the gateway and used only by its Git relay under a short-lived single-operation authorization; only the run initiator may approve (ADR-0035 removed the approval and the relay's branch rules; the git host decides) |
| ADR-0024 | Run notes as builtin tools (`write_notes`, `read_notes`) rather than an MCP server: the notes are part of the authoritative history and the projection, pinned across compaction and resume |
| ADR-0025 | Fallback routes: an admin-declared ordered list per route; a run switches after its retries are exhausted or a long Retry-After, sticks for the run, records `route.fallback`, and adapts to the fallback route's capabilities (text tool protocol, no images, smaller window) |
| ADR-0026 | Runtime protocol revision: one integer raised with every configuration addition; the runtime sends it on the link; the gateway refuses a runtime below the floor (= the current revision) with `426 runtime_too_old`; the image label lets readiness refuse early |
| ADR-0027 | Opaque reasoning state: provider reasoning items stored byte for byte as `reasoning.state`, replayed only to the same route, protocol and model, dropped by compaction and on a cross-route fallback; clients never receive the item, only its size (revision 5). Amended: a Chat tool call's opaque `extra_content` is such a state, sent back on its call; a mid-turn fallback into an endpoint that requires it is refused |
| ADR-0028 | Runtime-mutable settings: routes, egress rules and limit overrides live in PostgreSQL behind one atomically swapped snapshot; files are seed and import/export and apply only when changed; changes reach new conversations, sessions and admissions, never a running step; versioned writes and an admin audit |
| ADR-0029 | The skill tool: a read-only builtin, `skill {name}`, that loads one skill of the run's byte-stable index on demand (its body and up to 100 helper paths); preferred to widening `read_file`'s roots (path confinement) and to an MCP server (skills are core FR-11, must work with no servers, and the index must be byte-stable); it lives in `tqa-prompt` beside the parser (G-07) |
| ADR-0030 | Workspace service over the runtime link (proposed): on block-image installs the gateway reads, lists, uploads and snapshots workspace files through correlated request/reply frames answered by the runtime, with gateway-side bounds, path validation and blob-hash checks; the pure manifest walker moves to `tqa-tools` so both sides build identical manifests |
| ADR-0031 | MCP connectors through the gateway proxy (accepted): the proxy is the enforcement point, fail-closed on the conversation's own connector set, with the method allow-list, the tool filter and the gateway's own policy; an `ask`-class call runs only for its journaled call with the owner's one-use allow-once or a session grant for that same tool; credentials per user (bound to the connector's URL origin) then shared, never in the sandbox; class `execute` by default, hints only tighten; pinned outbound https without redirects; OAuth out of v1 (ADR-0035 removed the `ask`-class approval and the connector class) |
| ADR-0032 | Custom sandbox templates (accepted): runtime-mutable settings (ADR-0028) registered by image digest, never pulled; checked in the background without running image code (derivation, protocol label, image policy, runtime binary hash, microVM rootfs by image ID); offered only when enabled and verified (or unchecked from the file); exactly one default that stays offered (`template_is_default`); a new image reaches idle bound conversations at their next sandbox |
| ADR-0033 | The built-in secret store is the only secret store (accepted): AEAD in PostgreSQL under an install key outside the database; the gateway is the only reader; anything the store needs later (read audit, KMS or HSM wrapping of the install key) is built into it, never borrowed; no external vault backend is built, bundled or connected |
| ADR-0034 | Agent teams (accepted 2 Oct 2026; built behind `[agent_teams] enabled`, off by default): a lead and its members, enforced by the gateway; roles stored at start and tools by role; members started by the lead with `start_member`, normal tasks grouped under it, with the lead's settings or narrower and depth 1; addressed messages on the channel's storage with per-reader seqs, delivered or refused, waking instead of waiting; no agent votes (the lead decides, people veto); per-member `own_copy` (files handed back through reports) or `my_files` (turns under the lease); budget and deadline at the relay; idle and stuck notices; room for swarms later |
| ADR-0035 | Guardrails at the network boundary (accepted 8 Oct 2026): no per-call permission policy and no approvals; the network allow-list is the only guardrail added; a refused connection names its host and port, and the prompt lists the allowed hosts; the git host's rules decide what a push may do; of the extras, recent refusals are built after 17.0, per-profile lists come when someone asks, and observe mode is dropped (PRD §12 item 23) |
| ADR-0036 | Managing skills in the product (proposed, amends ADR-0029): a draft for the Product Owner, slice 1 only, no code before the Product Owner's OK |
| ADR-0037 | Per-service keys (accepted 30 Sep 2026; part of 17.0): a member's or a shared key for one web address, given to a run as a per-run placeholder bound to its sandbox and run; the egress proxy opens TLS only for keyed hosts, swaps the placeholder for the value in the key's own header only, scrubs responses and audits every use before it (fail closed); an install CA rotated on demand; administrators make shared keys and choose who may use them; a member's key serves only that member's runs; `git_push` and the Git relay retire for plain git over HTTPS (amends ADR-0023, ADR-0033, ADR-0035) |
| ADR-0038 | Swarms (accepted 2 Oct 2026, built after agent teams): one goal across a person's own agent teams; the person sets up the goal and its parts (each a new lead task from a profile they pick) or confirms the coordinator's proposal; one coordinator talks to the leads, leads only to the coordinator, helpers never leave their team; only messages and attached files cross teams; one goal budget and deadline; two levels, no cycles; each page's message box talks to that page's agent; no publishing, sharing or outside agents |
