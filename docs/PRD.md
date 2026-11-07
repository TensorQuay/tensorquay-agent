# TensorQuay Agent: Product Requirements

Status: **v0.2** (1 Oct 2026): v0.1 (22 Sep 2026) revised with the Product Owner's decisions to date, listed in §12. Roles: Product is the TensorQuay Product Owner (PO);
Engineering is TensorQuay engineering (AI-assisted). Companion documents: [ARCHITECTURE](ARCHITECTURE.md),
[DEV-PLAN](DEV-PLAN.md), [TEST-PLAN](TEST-PLAN.md), [DEV-GUIDELINES](DEV-GUIDELINES.md).

## 1. Mission
**Our mission is to provide an open, free and efficient on-premise AI agent platform that is easy to use, for any
company and any individual.**

### 1.1 What we believe
- **Developers and the open-source community deserve to use AI with the best options available to them,** not locked
  into one vendor's platform: any model, any provider, any hardware they choose.
- **End users deserve every level of the harness,** above all the agent environment: isolated sandboxes, team
  management and, in time, agent teams that work together on one task.
- **Open source is what keeps this trustworthy.** Anyone can read, run, verify and improve the whole stack, and nobody
  has to ask permission to use it.
- **We are building an environment and a communication system for agents, not for humans.** Humans set goals
  and watch; they are not asked to approve the agent's steps. Agents live in the environment: they talk to tools, to services and, in time, to each other
  through one protocol. Every capability is available to a program, not only to a browser.

TensorQuay Agent is that platform. A company or an individual installs it on one machine, signs in from a browser,
and each conversation gets its own isolated sandbox where the agent reads, edits, runs and ships code. Models run
locally on an OpenAI-compatible server (vLLM, SGLang or TensorQuay Engine) or, when the user chooses, through a cloud
provider. Version 1.0 delivers the sandboxed agent, team management and the shared channel through which agents
and humans communicate, vote and decide; agent teams (a lead and its helpers) follow in v1.1 (§6).

### 1.2 Why another harness, when the market is full of them
Coding agents are everywhere, and several are excellent. We build this one because the shape the mission calls for
does not exist yet:

1. **The team box is unserved.** Today's harnesses come in three shapes: a CLI on one developer's laptop, a hosted
   service in someone else's cloud, or a chat window without a workspace. A team of eight sharing one GPU workstation
   needs a fourth shape: a server on the local network that gives every person their own sandboxed agent, keeps their
   conversations, and needs one administrator. No open project ships that in a form a small team can install in an
   afternoon.
2. **Local models need a different harness.** Most harnesses are tuned for frontier cloud models. They assume reliable
   native tool calling, very long context windows and vendor-specific caching. An open model on two GPUs needs help: a
   tool-call fallback, budgets sized for its window, prompts shaped so the server's prefix cache hits, and honest
   per-model measurement. Getting this right is the difference between a demo and a tool the team uses every day.
3. **A shared box needs real isolation.** When many people's agents run on one machine, each conversation needs its
   own container or microVM, secrets must stay outside the sandbox, and network egress must be a policy rather than a
   hope. Harnesses built for a personal laptop skip this because they can.
4. **Teams want to own the whole stack.** Hardware, inference engine and agent from one place, open under Apache-2.0,
   small enough to read, with a documented protocol. No vendor account, no telemetry, no licence per seat, and no
   dependence on a company's roadmap for a feature the team needs.
5. **It closes TensorQuay's loop.** TensorQuay Engine makes new open models fast on consumer GPUs; TensorQuay Agent
   turns that speed into a team's work, and gives the engine a real workload to measure against. The two are designed
   and benchmarked together, so a customer's box is proven on the agent workload it will actually run.

What this is not: not a new model, not another single-user terminal agent, not a hosted service, and not a general
platform for every AI use case. It is the agent platform for people who run AI on their own hardware.

### 1.3 First targets
- **Hardware:** one TensorQuay box (2 × RTX PRO 6000, GLM-5.3-Flash class model) behind an OpenAI-compatible
  endpoint, serving a team of about 8.
- **Surfaces:** a multi-user web UI is the only human surface. Programs and other agents use the same HTTP API and
  wire protocol with an API token. No CLI product, no terminal UI.
- **Two runtime modes:** `docker` (default; Docker Engine or Docker Desktop) and `firecracker` (KVM microVMs on a
  Linux host) behind one runtime interface. Same image, same protocol, same tests.

## 2. Goals (why this project exists)
IDs are stable and referenced elsewhere; the order is the priority.

| Priority | # | Goal | What it means in practice |
|---|---|---|---|
| **1 (primary)** | G1 | **A team-grade agent on one on-premise box** | Many users, persistent conversations, one sandbox per conversation, resume after disconnect or restart. Nothing leaves the network unless the admin allows it |
| 2 | G2 | **Local-model first** | Works well with open models served by vLLM, SGLang or TensorQuay Engine: OpenAI-compatible protocols, prompt-cache-aware context, tool-call fallback for models with weak native tool calling, budgets sized for smaller context windows |
| 3 | G3 | **Strong isolation by design** | Every conversation runs in its own container or microVM. Secrets never enter the sandbox. Network egress is allow-listed. Inside the sandbox the agent works freely; the network is the guardrail |
| 4 | G4 | **Minimal core, open extension points** | A small Rust kernel (agent loop, protocol, sandbox, gateway) with contracts and tests per component, a documented wire protocol, reproducible images, Apache-2.0. Every other capability arrives through a documented extension point, by configuration rather than code (§6.1) |
| 5 | G5 | **Power TensorQuay's products and benchmarks** | The same harness drives the agent-wave benchmarks for TensorQuay boxes and the engine; results are reproducible |

## 3. Why build it when open agents exist
Coding agents are plentiful. Verified 22 Sep 2026, the gaps for our target are:

| Existing option | What it is | Gap for a team box |
|---|---|---|
| Terminal coding agents (Codex CLI, Claude Code, OpenCode, aider) | Excellent single-user CLIs on the developer's machine | Single user, no shared server, no sandbox per conversation, UIs tied to the vendor or to a local terminal; some are not Apache-2.0 |
| OpenHands (MIT) | Web UI + per-conversation Docker sandbox, Python | Closest prior art. Python runtime, heavy image, Docker only (no microVM), multi-user is enterprise-only, tuned for frontier cloud models |
| Hosted agent platforms (Codex Cloud, Devin, Cursor cloud agents) | Cloud sandboxes with web UIs | Not installable on-premise; data leaves the network |
| Self-hosted chat UIs (Open WebUI, LibreChat) | Chat over local models | Chat, not an agent with an isolated workspace and tools |

**What TensorQuay Agent does instead:** a Rust harness and gateway, one image that runs as a Docker container or a
Firecracker microVM, multi-user from day one, local-model-first prompting and budgets, and a first-class path to
TensorQuay Engine. OpenHands remains the reference we compare against for user-facing capability (M6).

**Clean-room rule.** Everything in this repository is written from this PRD and the architecture document. No code,
prompt text, schema or document is copied or ported from any other system, open or proprietary; third-party open-source
software is used only as a declared dependency under its licence. See §11.

## 4. Users
| User | Need |
|---|---|
| Person on a team box (any kind of work) | Start a task, give it files or let it fetch what it needs, watch it work, get the result in the workspace |
| Team admin / IT | Install with Docker Compose, add users, configure models and keys, set the egress allow-list, see usage |
| Community: self-hosters with a GPU | The same product on one machine, Docker mode, one user or a few |
| TensorQuay engineering | A scriptable harness for agent-wave benchmarks against the engine and vLLM baselines |
| Contributors | Small crates with contracts and tests, a documented protocol, a clear runtime interface to add backends |

## 5. Success metrics
All are measured by the TEST-PLAN procedures; PERF numbers are on the named reference hardware.

| ID | Metric | Target (v1.0) |
|---|---|---|
| M1 | **Task completion** on a fixed local-model task set (TEST-PLAN §16, `E2E-WAVE`) | ≥ 90 % of tasks complete with a passing check when the model is GLM-5.3-Flash class; the remaining failures are model errors, not harness errors (classified per run) |
| M2 | **Harness overhead** per model turn, excluding model time | ≤ 50 ms p50, ≤ 200 ms p99 (PERF-001) |
| M3 | **Sandbox start** (create to first tool ready) | Docker on Linux: ≤ 3 s p50 warm image. Firecracker: ≤ 2 s cold boot p50; ≤ 500 ms from a warm snapshot (Phase 4 stretch) (PERF-010/011). **As the user feels it** (PO, 1 Oct 2026): from sending the first message on a new task to the agent starting work, ≤ 3 s p50 on both backends, measured for a new task (first boot) and a returning one |
| M4 | **Concurrency** | 16 live conversations on one gateway host with 2 vCPU / 4 GB and no dropped events (PERF-020). The model server, not the harness, is the limit |
| M5 | **Reliability** | 24 h soak at 8 conversations: no crash, gateway RSS growth < 5 %, zero lost or duplicated events across forced reconnects, every run resumable after a gateway restart (SOAK-001…004) |
| M6 | **Capability parity** with the open reference on the same tasks and the same model | Task completion within 5 points of OpenHands on the shared subset (E2E-CMP) |
| M7 | **Isolation** | The security test set passes in both runtime modes: no secret readable from inside the sandbox, no egress outside the allow-list, no write outside the workspace (SEC-*) |
| M8 | **Openness** | Every published number has its configuration, raw data and a reproduction script. Coverage ≥ 90 % lines on the pure crates (G-04) |

## 6. Scope
**v1.0 (in):**
- **Harness:** a single-agent loop with streaming, native tool calling and a text-protocol fallback, tool result caps,
  context compaction, iteration and token budgets, error recovery with back-off, a durable run journal and resume.
- **Models:** three wire protocols, OpenAI Chat Completions (primary, for local servers), OpenAI Responses and
  Anthropic Messages. **Any endpoint that speaks one of them can be configured by the user**, local or remote,
  without code: a model registry with logical names, authentication scheme, capabilities, token limits, reasoning
  mapping and a vendor-knob escape hatch; a "test route" probe that verifies a route before use. **Administrators
  add and manage models; members choose from them** (no personal models: PO, 1 Oct 2026). Prompt-caching-aware request shaping.
- **Tools:** `read_file`, `write_file`, `edit_file` (exact search-and-replace), `glob`, `grep`, `shell` (timeouts, output caps; background
  processes stop when the command that started them returns, ADR-0016 amendment), `fetch_url` (egress-gated). MCP client for everything else. Workspace instruction files (`AGENTS.md`)
  and a skills directory.
- **Agent communication (foundation):** channels, a shared, ordered, persistent forum at the gateway where agents
  and humans post, read, wait, propose, vote and claim tasks; decision rules `human` and `majority`; a channel page
  in the web UI. Agent teams on the channel's storage are v1.1.
- **Extension points (§6.1):** skills, MCP tools inside the sandbox, connectors as gateway-proxied MCP servers with a
  credential vault, agent profiles (customised agents), sandbox templates and model routes.
- **Guardrails (amended 29 Sep 2026, PO):** the sandbox is the wall and the network is the guardrail. Inside the
  sandbox the agent reads, writes and runs any command freely; nobody approves its steps. What it may reach over the
  network (sites and ports, egress and ingress) is the administrator's allow-list, and a blocked connection tells the
  agent which host and port were refused so it can adapt. Outside systems (git hosts, APIs, connectors) apply their own
  permissions to the credentials the gateway adds; credentials never enter the sandbox.
- **Sandbox runtime:** the agent environment exists only inside a sandbox; the loop never runs on a host directly.
  One OCI image; backends `docker` (default) and `firecracker`; one workspace mounted per sandbox; disposable sandbox; activeness-driven reaper; egress allow-list; LLM traffic relayed through the
  gateway so keys never enter the sandbox.
- **Gateway (control plane):** local accounts with roles (admin, member), invitations, workspaces, conversations,
  event persistence, server-sent event streams with exact replay, sandbox admission (slots, queue, eviction, pin),
  model registry, the built-in encrypted secret store (ADR-0033), usage metering, admin
  settings, Prometheus metrics.
- **Workspaces and storage:** a workspace is an owned, isolated directory or block image on a local disk of the host
  where the agent environment is installed, mapped into the sandbox as `/workspace`. A conversation binds to one
  workspace; several conversations may reuse a workspace under a single-writer lease; workspaces can be forked and
  shared. No object storage.
- **Web UI:** sign-in, conversation list, new conversation (model and environment pickers in the conversation bar), environment switching (§4.7 of ARCHITECTURE), streaming transcript
  with reasoning and tool cards, cancel, workspace file and diff view, settings,
  admin pages.
- **Programmatic access:** the HTTP API and the wire protocol with API tokens (FR-26). The benchmark and CI driver
  is test tooling inside the repository, not a product.
- **Deployment:** Docker Compose for one box (gateway, PostgreSQL, web, model endpoint configuration); a documented
  Firecracker host setup for Linux KVM.
- Linux x86_64 hosts for production; macOS with Docker Desktop for development and Docker mode.

**Non-goals for v1.0:** computer use (an agent operating a virtual desktop) and desktop streaming (a later phase; headless
browser automation through the Workstation environment's tools is in v1.0), mobile apps, billing and credits,
built-in connector code (connectors are MCP servers added by configuration, §6.1), an in-process plugin API,
agent teams (v1.1) and swarms (later; the channel primitive is in), cross-conversation memory, canvas or app builders,
fine-tuning, Windows hosts, a
Kubernetes operator, a CLI product, a terminal UI or REPL of any kind, running the agent loop outside a sandbox,
object storage for workspaces or events.

**Later (each needs a PO decision):**
- **v1.1: agent teams** (ARCHITECTURE §16.2, ADR-0034; §12 item 20): a person asks one task to lead; the lead starts
  its helpers, writes their briefs, collects their reports and decides. Roles, tools by role, delivery, budgets and
  limits are enforced by the gateway. Helpers work on their own copy or take turns on the lead's files and hand
  results back as files. No votes between agents.
- **Later: swarms** (ARCHITECTURE §16.4, ADR-0038; §12 item 21): one goal across a person's own agent teams. The
  person sets up the goal and its parts, each a lead task from a profile they pick, or confirms the coordinator's
  proposal; one coordinator talks to the leads. No publishing, sharing or outside agents.
- OIDC single sign-on; multiple organisations per install.
- Warm-snapshot pools for Firecracker; custom images per team; GPU pass-through for in-sandbox inference.
- Cross-conversation memory; hooks (pre- and post-tool scripts or HTTP).

### 6.1 Extension model: minimal core, everything else is an extension
The core stays small: the agent loop, the wire protocol, the sandbox runtime and the gateway. Capabilities are added
through the extension points below. Most need no code in this repository, and none of them widens the trust boundary:
an extension runs either inside the sandbox without secrets, or outside it behind the gateway with credentials in the
vault.

| Extension point | What a user adds | How | Core code needed |
|---|---|---|---|
| **Skills** | Instructions and helper scripts for a kind of task | A folder with `SKILL.md`; per install, team or user | none |
| **Instruction files** | Repository-specific rules | `AGENTS.md` in the repository | none |
| **Tools** | Any capability the agent can call | An MCP server over `stdio` inside the sandbox, for local, secret-free tools (language servers, formatters, local databases) | none (the MCP client is in the core) |
| **Connectors** | External services: issue trackers, chat, mail, storage, databases | A remote MCP server registered at the gateway. Credentials live in the gateway vault and are injected by the gateway's MCP proxy, so the sandbox never sees them | none |
| **Agent profiles** (customised agents) | A named agent: instructions, model route, allowed tools and connectors, skills, sandbox template, budgets | A TOML file or the admin UI; shared by a team or personal. An agent team's helpers inherit their lead's (v1.1) | none |
| **Sandbox templates** | Toolchains, runtimes, pre-installed packages | A Dockerfile `FROM` the runtime base image, registered by digest. The same template serves Docker and, once exported to a rootfs, Firecracker | none |
| **Model routes** | Any endpoint speaking OpenAI Chat Completions, OpenAI Responses or Anthropic Messages, local or remote, with its auth scheme and vendor knobs | `models.toml` or the admin UI; a probe verifies it | none |
| **Decision engine** | How the next action is chosen | Default: the model decides. Optional: an external decision service that pre-selects tools, acts on routine steps without a model call, picks the model route per step, decides when to compact, or stops early. It cannot widen what the network allow-list and the task's tools allow | a trait and the default in the core; engines outside |
| **Hooks** (v1.1) | Scripts or HTTP calls at lifecycle points: before and after a tool call, session start and end | Configuration | small |
| **Runtime backends, provider protocols** | New isolation technology or a new wire protocol | A Rust trait implementation contributed to the project | yes, with an ADR |

**The core ships no connectors.** Not mail, not chat, not storage, not issue trackers. Each of those is an MCP server
that a user registers, and the project documents tested ones. There is no in-process plugin API. **MCP is the plugin
protocol:** it is a public standard, servers can be written in any language, and extension code stays out of the
harness process. A catalogue of tested MCP servers and templates is documentation (`docs/catalog/`), not code in the
core.

**Contributing an extension is a first-class path.** Every extension point is a versioned contract with a conformance
suite that a contributor can run locally (`tools/ext-check <kind>`, developer tooling in the repository); a runtime backend, a provider protocol or a
decision engine that passes its suite is accepted on that evidence (ARCHITECTURE §15).

## 7. Product surfaces
| Surface | v1.0 content |
|---|---|
| Web UI | The pages in §6 |
| HTTP API | Auth, users, conversations, messages, events (paged), models, admin settings, usage. Documented with an OpenAPI file |
| Event streams and actions | Server-sent events per conversation and channel, `Last-Event-ID` replay by sequence number; actions as POSTs with idempotency keys; the same typed frames the runtime link carries (ARCHITECTURE §9) |
| Images | `tensorquay/tqa-gateway`, `tensorquay/tqa-web`, `tensorquay/tqa-runtime:{base,workstation}` (the Minimal and Workstation environments); Firecracker rootfs built from the same runtime image |

## 8. Requirements
### 8.1 Functional
| ID | Requirement |
|---|---|
| FR-1 | **Agent loop.** Stream model output; parse native tool calls; execute tools; append results; repeat until a final answer, a budget or a cancel. Every step emits typed events with a monotonic sequence number |
| FR-2 | **Tool-call fallback.** When the selected model route declares `native_tools = false`, the harness uses a documented text protocol for tool calls and repairs common malformed outputs; the fallback is measured per model in the E2E set. One exception, by design: when the model server shows it parsed a call natively (it withheld the reply's chunks, or the reply is empty with tokens counted), that step is asked again once with the tools declared natively, and after native calls come back every later request of the run declares them; a server that refuses declared tools keeps the route text-only for the run (ADR-0007, CORE-U-065) |
| FR-3 | **Protocols.** OpenAI Chat Completions, OpenAI Responses and Anthropic Messages with streaming, tool calls, reasoning content, usage and stop reasons. Any endpoint speaking one of them is configurable; the harness never branches on a vendor name. Capability flags (`native_tools`, `parallel_tools`, `vision`, `reasoning`, `stream_usage`, `cache_hint`) are declared per route; token limits are marked derived or guessed |
| FR-4 | **Model registry.** Routes map logical names to: protocol, base URL, model id, authentication scheme (`none`, `bearer`, custom header, query parameter), a secret-store key reference, limits, capabilities, reasoning mapping, `extra_headers` and `extra_body` merged verbatim, and named adapter quirks. Only administrators add, change or remove routes; members choose from the routes administrators deploy (PO, 1 Oct 2026). A "test route" probe records observed capabilities. The gateway serves the allowed list per user; `models.toml` is the import and export format |
| FR-5 | **Context.** Token counting per route; deterministic slot order so prefix caches hit; structured compaction when the budget is reached (fixed summary fields; the last N turns and their tool results kept; the task statement, instruction files and run notes pinned); duplicate tool results replaced by stubs; tool results capped by bytes with spill to a workspace file; the compaction event is visible in the transcript |
| FR-6 | **Budgets and recovery.** Iteration budget, output-token budget and wall-clock budget per run; exponential back-off with jitter on transient provider errors; stream cut detection and retry; repeated-failure and no-progress detection end the run with a clear reason |
| FR-7 | **Run journal and resume.** The event log is the single source of truth; the message list is a pure projection of it. Every run appends to a journal in the workspace; a runtime restart resumes an interrupted run from the last complete step; the UI shows it as resumed. A run notes file written by the agent is pinned into the context and survives compaction and sandbox recreation |
| FR-8 | **Tools.** The builtin set in §6 with JSON schemas, path validation confined to the workspace, and shell execution with timeouts, output caps, environment scrubbing and background process management |
| FR-9 | **MCP.** Two tiers: `stdio` servers inside the sandbox (no secrets), and remote servers reached through the gateway's MCP proxy (§6.1 connectors). Configured per install, per profile and per conversation; discovered tools appear with a namespace prefix; connector calls are subject to the remote system's own permissions |
| FR-10 | *Superseded (29 Sep 2026, PO).* There is no per-call permission policy and no approval step: the sandbox is the wall and the network allow-list (egress and ingress) is the guardrail (§6 Guardrails). A blocked connection returns a plain reason naming the host and port |
| FR-11 | **Instructions and skills.** Load `AGENTS.md` from the workspace root and parents; load skills from a configured directory; both are shown in a "context" panel |
| FR-12 | **Sandbox lifecycle.** States `creating → ready → busy ↔ idle → destroyed`, plus `parked` for runs waiting on a human or a channel. Activeness is defined by the runtime's lifecycle events (`run.started`, terminal events, heartbeats), never by processes or traffic; the reaper destroys sandboxes by idle window, max lifetime or missed heartbeats and parks long waits; the workspace survives the sandbox and the next message wakes it |
| FR-13 | **Runtime backends.** `docker` and `firecracker` implement one `SandboxBackend` interface: create, start, exec-health, network attach, destroy, list, stats. Backend choice is per install; a conversation never changes backend |
| FR-14 | **LLM relay.** The sandbox calls the gateway relay with a per-run token; the gateway resolves the route, injects credentials, enforces the user's allowed models, meters usage and streams back. The sandbox never holds provider keys |
| FR-15 | **Egress policy.** Default deny from the sandbox except the gateway relay; an admin allow-list of hosts (for example package registries and git hosts); the policy is enforced by the backend, not by the agent |
| FR-16 | **Gateway API and persistence.** Users, roles, invitations, conversations, messages, events, usage in PostgreSQL. Events are append-only with `(conversation_id, seq)` unique |
| FR-17 | **Event streaming and reconnect.** Clients receive events as server-sent events and act with POSTs; on reconnect `Last-Event-ID` (the sequence number) yields the missing events exactly once; the runtime keeps an outbound link to the gateway and nothing listens inside the sandbox; a run continues while no client is connected |
| FR-18 | **Web UI.** The pages in §6, including the workspaces page and the workspace picker in the new-conversation dialog; streaming text and reasoning deltas; tool cards with command, output and diff; cancel; resumed-run indicator; file tree and diff of the workspace; settings; admin pages |
| FR-19 | **Outside services, git included.** Nothing in the web UI or the setup is specific to git (PO, 2 Oct 2026). Git is a tool inside the sandbox like curl or pip: the agent clones, commits and pushes with plain `git`. A git host is an outside service like any other: the person's key or token for it lives in key management (FR-28, ADR-0037) and is swapped in at the network edge, never entering the sandbox, the model context, events or logs; the host's own rules (branch protection, token scopes) decide what is accepted. The `git_push` tool and the Git relay retire over two releases (§12 item 12) |
| FR-20 | *Superseded (22 Sep 2026).* The headless CLI was removed: there is no CLI product. Programmatic use is FR-26; the benchmark driver is test tooling that answers approvals by a stated policy and records a deterministic event log |
| FR-21 | **Usage and metrics.** Per-run and per-user token usage; Prometheus metrics for runs, sandboxes, relay latency, provider errors, event lag |
| FR-22 | **Admin settings.** Model registry editing, the secret vault (provider keys and connector credentials; write-only, encrypted at rest), egress allow-list, runtime backend status, sandbox limits per user (max live, idle window, max lifetime), connectors, templates and shared profiles |
| FR-23 | **Agent profiles.** A profile defines instructions, model route, allowed tools and connectors, skills, sandbox template and budgets. Profiles are validated files or admin-UI records, shared or personal; a conversation is created from a profile and keeps it |
| FR-24 | **Connectors.** A connector is a remote MCP server registered at the gateway with credentials in the vault (shared or per user). The sandbox reaches it only through the gateway MCP proxy with the per-run token; the proxy injects credentials and enforces the profile's allow-list |
| FR-25 | **Sandbox templates.** Custom images derived from the runtime base image, registered by digest, selectable per profile and per conversation; the hardening posture is unchanged by a template; the Firecracker rootfs is built from the same image |
| FR-26 | **Agent-first API parity.** Every capability of the web UI is reachable through the documented HTTP API and the wire protocol with an API token, so a program or another agent can open a conversation, send messages and read events. The web UI uses only the public API; there are no private endpoints. Exception (24 Sep 2026): creating or revoking API tokens and administering users require a signed-in browser session, so a token can never mint tokens or change accounts |
| FR-27 | **Decision engine.** The loop asks a `DecisionEngine` before each step and after each model response. The default engine lets the model decide. An external engine speaks the documented decision protocol and may narrow tools, act without a model call, choose the model route, request compaction or stop; it cannot widen what the network allow-list and the task's tools allow; if it is unavailable the loop falls back to the default with a visible event. Its effect on tokens, steps and completion is measured, never assumed |
| FR-28 | **Secret store.** One built-in store (ADR-0033): values encrypted at rest (AEAD in PostgreSQL) with an install key held outside the database. The gateway is the only reader; values are write-only from the UI and API; rotation takes effect without a restart; a store that cannot be read fails new runs closed. Anything the store needs later is built into it, never borrowed from another product |
| FR-29 | **Workspace storage on the host's local disk.** Every workspace is a directory (Docker: bind mount or named volume) or a block image (Firecracker) under a configured local directory on the host, mapped into the sandbox as `/workspace`; per-workspace quota; retention policy; diagnostics warn on network or slow filesystems |
| FR-30 | **Workspace model.** A workspace is owned by one user, isolated from every other user's, and is the unit a conversation binds to at creation. Many conversations may bind to one workspace; one conversation at a time may run on it (write lease, released on run end, sandbox death or expiry; the next waits or is refused per setting). Workspaces can be created empty or by forking, and files arrive by upload or by the agent fetching them; they can be shared with named users or a team as read or write; admins see metadata, not contents |
| FR-31 | **Channels.** A channel is an append-only, ordered, persistent message log at the gateway with members (conversations and users; roles lead, member, observer) and structured message types (text, proposal, vote, decision, task, claim, report, done). Agents use four tools (`channel_post`, `channel_read`, `channel_wait`, `task_claim`) through the gateway with the per-run token; the author is set by the gateway. Claims are atomic. Decision rules `human` and `majority` are evaluated by the gateway. Humans use the same channel from a web page and can veto. v1.1's agent teams (FR-32) build on the channel's storage and stream, not on its votes |
| FR-32 | *(v1.1, designed now, ADR-0034)* **Agent teams.** A person asks a normal task to lead; the lead starts members (helpers) itself with a brief it writes (`start_member`), each a normal task grouped under the lead, from an agent profile the lead names (the person's own or a shared one; its tools and connectors never wider than the lead's) or with the lead's own settings, and with no members of its own. Members talk only to the lead; the lead asks, collects reports and decides, with no votes between agents; people read everything, watch each helper's transcript live, type to the lead or any helper (the lead is told), overrule a decision by telling the lead, and stop; when the agent team needs a person, the lead asks and its task shows "Waiting for you", and helpers ask the lead, never the person directly. Roles and tools by role, addressed delivery (stored, delivered once or refused with a reason, waking instead of waiting), a token budget, a deadline, idle and stuck notices and the member limit are enforced by the gateway (limits are admin settings). Per helper, the lead chooses its own copy of the workspace (results handed back as files) or turns on the lead's files under the lease |
| FR-33 | **Admission on limited hosts.** A resource budget (a configured share of host memory and cores) yields sandbox slots, with a per-user cap; when slots are full the gateway evicts the longest-idle warm sandbox or queues the run with per-user fairness and shows the position; busy and pinned sandboxes are never evicted before their max lifetime; a keep-alive pin keeps one conversation's sandbox live between messages |
| FR-34 | **Environments.** Each conversation runs in an environment described by a template with declared capabilities. The default Workstation template is a complete working computer for an agent (coding toolchains, a headless browser, common CLI tools); a Minimal template exists for tests and locked-down installs; custom templates derive from them by digest. A user picks the environment with the environment picker in the conversation bar and may switch it while no run is active; files and history carry over and the change is recorded in the transcript. Computer use (a desktop the agent operates, streamed to the UI) is a later phase |
| FR-35 | **Languages.** The web UI ships in English, Simplified Chinese, Japanese, Korean, Vietnamese, Thai, Malay, Tamil, French and Spanish; the language follows the browser and can be changed per user; catalogues are plain files so the community can add languages with one pull request (UI-GUIDELINES §11b). | WEB-U-019 |
| FR-36 | **Attachments and images.** Files attached or dropped in the composer are stored in the conversation's workspace (`/workspace/uploads/`, size-limited and checked by the gateway) and passed to the agent as attachments; images and pasted screenshots go natively to a model whose route has the `vision` capability, and otherwise stay files with a plain notice suggesting a vision model. | WEB-U-020, GW-I-045 |

### 8.2 Non-functional
| ID | Requirement |
|---|---|
| NFR-1 | **Security.** No telemetry. Secrets only in the built-in secret store, encrypted at rest. Sandboxes run unprivileged with a read-only root filesystem, no host sockets, dropped capabilities, a seccomp profile (Docker) or the microVM boundary (Firecracker). Prompt and tool content is not logged by default |
| NFR-2 | **Privacy.** All data stays on the box unless an admin configures a cloud provider; the UI shows which model route is local and which is remote |
| NFR-3 | **Reproducibility.** Pinned Rust toolchain, Node, Next.js, Firecracker and kernel versions; image digests recorded in the release notes; every benchmark stores its environment |
| NFR-4 | **Lean code.** The ENGINEERING-GUARDRAILS standard: files ≤ 1000 lines; downward-only crate dependencies; pure crates build and test on macOS; no unsafe outside audited modules; lints clean |
| NFR-5 | **Licensing.** Apache-2.0. Dependencies are permissive (MIT, Apache-2.0, BSD, ISC); a `cargo deny` and an npm licence check gate the CI. Third-party notices are kept |
| NFR-6 | **Operations.** One `docker compose up` for Docker mode; a documented host script for Firecracker mode; health endpoints; graceful shutdown that does not kill running sandboxes; schema migrations that are forward-only and tested |
| NFR-7 | **Accessibility and i18n.** The web UI is keyboard-operable and passes basic contrast checks; strings are externalised for later translation (English only in v1.0) |
| NFR-8 | **Documentation.** Install guide, admin guide, protocol reference, contributor guide, and this document set |

## 9. Milestones and exit criteria
| Milestone | Exit criteria |
|---|---|
| **MS0 Documents approved** | PO approves PRD, ARCHITECTURE, DEV-PLAN, TEST-PLAN |
| **MS1 Walking skeleton** (Phase 1) | An agent run visible end to end in the browser on Docker mode: loop, four tools, runtime in a container, gateway with persistence and relay, streaming transcript; the skeleton subset of P0 tests (TEST-PLAN §17 gate 1) |
| **MS2 Team MVP** (Phases 2–5) | Robustness, safety, team and local-model quality on Docker mode; FR-1…FR-8, FR-10…FR-19, FR-21, FR-22, FR-26, FR-29, FR-30; all P0 tests through gate 5; SEC P0 in Docker mode; metrics M2, M3 (Docker), M4 measured; E2E-WAVE-001 on the TensorQuay box |
| **MS3 Extensions and channels** (Phases 6–7) | FR-9, FR-23…FR-25, FR-28, FR-31; gates 6 and 7 |
| **MS4 MicroVM mode** (Phase 8) | Firecracker backend passes the same SBX and SEC suites; metric M3 (Firecracker) measured on the reference host |
| **MS5 v1.0 public** (Phase 9) | FR-27; metrics M1–M8; SOAK passes; docs complete; PO sign-off |

## 10. Risks
| Risk | Mitigation |
|---|---|
| Local models call tools unreliably | FR-2 fallback protocol and repair; per-model E2E measurement; registry flag per route |
| OpenHands or another open project closes the gap first | Keep the differentiators sharp (microVM, multi-user, local-first, Rust, Apache-2.0); compare honestly in M6 |
| Firecracker host setup is hard for customers | Docker is the default; Firecracker is a documented host script on tested distributions; TensorQuay boxes ship pre-configured |
| Scope creep toward a general-purpose agent platform | §6 non-goals; every addition needs a PO decision |
| Small team | Docs, tests, CI, small crates; coding agents do the bulk under review |
| An external decision engine saves nothing, or hurts quality | It is optional and measured (E2E-DECISION-008); the default stays the model; the engine cannot widen the task's tools or the network allow-list |
| Prompt injection through repository content and tool output | Instruction files are shown, not trusted; the sandbox holds no credentials; egress allow-list; a red-team test set (SEC-3xx) |
| Protocol churn between web, gateway and runtime | One versioned protocol with a JSON Schema and contract tests on both sides (PROTO-C-*) |
| Event persistence becomes the bottleneck | Batched appends, sequence per conversation, measured in PERF-020; events are compact and payloads over a size cap spill to object storage |

## 11. Provenance and prior art
Public prior art studied for ideas, never for code: OpenAI Codex (Rust, Apache-2.0; its app-server protocol and
sandbox policies), OpenHands (MIT; its conversation and runtime model), Firecracker and its jailer (Apache-2.0), Kata
Containers, the Model Context Protocol specification and the OpenAI and Anthropic public API references.
Rules:
- Public projects are cited by name, factually and respectfully. No other code base is opened while writing code for
  this repository (DEV-GUIDELINES §2.5).
- Third-party open-source software enters only as a declared dependency under a permissive licence, never pasted.
- Every crate's README states which specification or contract it implements.

## 12. Decisions for the PO
**Decided by the technical lead (22 Sep 2026), recorded for information:** server-sent events plus POST for browsers
and programs with an outbound WebSocket runtime link (ADR-0015); activeness by lifecycle events and sandbox admission
with slots, queue, eviction, parking and pin (ADR-0016).

1. **Backend language for the gateway:** **Decided (PO, 22 Sep 2026): Rust.** One toolchain shared with the harness
   and the engine team; protocol types shared without code generation. Go was the alternative.
2. **Database:** PostgreSQL only (recommended: one engine, one migration path) or SQLite for single-user installs.
3. **Firecracker in v1.0 or v1.1:** the plan builds it in Phase 8 before the public release; deferring it shortens
   v1.0 by 3–5 weeks.
4. **Public repository name** (`TensorQuay/tensorquay-agent` proposed) and when to open the repository (at M2 or at v1.0).
5. **Reference hardware for the published numbers:** the TensorQuay box (2 × RTX PRO 6000) for E2E and a
   4-vCPU Linux KVM VM for Firecracker PERF.
6. **Crate and image prefix:** `tqa` (proposed).
7. **Secret store:** **Decided (PO, 27 Sep 2026): the built-in store is the only secret store** (ADR-0033). No
   external vault backend is built, bundled or connected.
8. **Busy workspace policy default:** `wait` (recommended: the second conversation queues until the lease is
   released) or `refuse`.
9. **Channels in v1.0:** **Decided (technical lead, on the PO's delegation, 27 Sep 2026): yes, in v1.0** (ADR-0014),
   built now as Phase 7 and time-boxed: if Gate 7 is not reached by 11 Oct 2026, v1.0 ships without channels and they
   move to v1.1. A task may allow more than one claimant, for comparing results. Agent teams are v1.1 and no longer
   use the channel's votes (item 20).

**Decided by the PO since v0.1 (recorded 1 Oct 2026):**

10. **Guardrails at the network boundary (29 Sep 2026, ADR-0035):** no per-call permission policy or approvals; the
    agent works freely inside its sandbox, and the network allow-list is the guardrail. ADR-0035's network extras
    (§2, points 1 to 4) are still pending.
11. **General purpose, not only coding (29 Sep 2026):** the product serves any team (research, finance, HR,
    engineering); user-facing words avoid developer and git jargon.
12. **Per-person keys for external services (30 Sep 2026, ADR-0037, decisions 1 to 6):** a member's own keys are used
    only for that member's runs and are swapped in at the network edge; the sandbox and the model see placeholders.
    The `git_push` tool retires in favour of plain `git` with a key, over two releases (ENGINEERING-GUARDRAILS §6).
13. **Licences (30 Sep 2026):** only Apache-2.0-compatible components, never GPL or LGPL in any form, even with an
    exception; where none exists, the project writes its own. Operating-system packages inside images are an
    aggregate (ADR-0017 point 7), and each release publishes the package list and source locations.
14. **No personal models (1 Oct 2026):** only administrators add models; members choose from them (FR-4).
15. **Sandbox start as the user feels it ≤ 3 s (1 Oct 2026):** M3 above.
16. **Quality before dates (1 Oct 2026):** every change follows TEST-PLAN and ENGINEERING-GUARDRAILS; a release
    goes to `main` only after the full integration (all suites, performance), a 24-hour soak on the same build, a
    first-hour new-user check, the technical lead's own browser pass, and the PO's OK.
17. **Named internal servers (1 Oct 2026):** an administrator may allow a named server on the company's private network
    per allow rule (admin-only, off by default, protected against names that point inside the network unexpectedly);
    an exact-IP rule keeps working as before. Part of the per-person keys release (ADR-0035 amendment).
18. **Delete means gone (1 Oct 2026):** deleting a task removes that task's data (its conversation, events, stored outputs and
    metadata) within minutes, and never the files it made in its workspace; deleting a workspace removes its files.
    Only a content-free audit record (who deleted what, when) remains. No undo.
19. **Web search without a search service (7 Oct 2026):** no paid search API and no separate search tool. When a
    task's internet access allows it, the agent searches the way a person at a shell would: a search page fetched with
    the shell, or loaded in the Workstation's headless browser if that is refused, then the pages it picks. The
    Workstation's instructions say how. The same day the PO restated the rule behind it: keep to basic tools, since
    the shell and Python cover most needs; a capability is a hint in the instructions before it is a new tool.
20. **Agent teams, not a forum (2 Oct 2026, ADR-0034 revised):** an agent team is a lead and its members, run the way
    a technical lead runs a few developers. Members talk only to the lead; agents never vote (the lead asks, members
    answer, the lead decides and announces); people can read everything, watch each member live, type to the lead
    or any member (the lead is told), overrule a decision by telling the lead, and stop; when the agent team needs a person, the lead asks and its task
    shows "Waiting for you", and members never ask the person directly. The gateway enforces roles, tools by role and delivery. A person starts one normal task and asks it to
    lead; the lead starts members itself, and limits (members per lead, a token budget, a deadline) are admin
    settings, with depth 1. Members are normal tasks grouped under the lead. Per member the lead chooses its own copy
    (results handed back as files) or turns on the lead's files; no git requirement. The `quorum` and `lead` rules
    and agent votes leave the v1.1 plan. A **swarm** (agent teams talking to agent teams) is a later phase.
    **Decided since (2 Oct 2026; the owner left the open points to the technical lead):** a person can type to any
    member directly, like typing into any developer's terminal window, and the lead is told (proposal G). It is built
    now, test-first, slice by slice, behind an install switch (`[agent_teams] enabled`, off by default), and merges
    into dev behind it; the 13h release and the next keep first call on testing time. Members per lead: 3 by default,
    at most 8, never more than the slots minus one. Deleting the lead's task removes the team's messages, the
    helpers' tasks and their copies; the lead's workspace stays. v1.0's channel stays as built, voting included. Of
    the proposals (ADR-0034), A, B, C, D, E, F, H and J are taken, and I is out of scope.
21. **Swarms: one goal across a person's agent teams (2 Oct 2026, ADR-0038, built after agent teams):** only one
    person's own teams, acting as that person, with no publishing, no sharing between people and no outside agents.
    The person names the goal and its parts and picks a profile for each part (their own or an administrator's
    shared one); each part runs as a new lead task. Or the coordinator proposes the split and the person confirms it.
    One coordinator task talks to the leads, leads only to the coordinator, and helpers never leave their team; the
    gateway enforces it as for agent teams. Only messages and attached files cross teams; one budget and deadline;
    at most two levels, no cycles; everything logged. Each page's message box talks to that page's agent.
    **Decided since (2 Oct 2026):** a Pause goal button (each team finishes its current step and waits until
    Resume); the word "Coordinator" is kept; deleting a goal deletes its teams as deleting a lead's task does.

22. **Nothing specific to git in the UI or setup (2 Oct 2026, restated by the PO):** git hosts are outside services
    like any other: a person's key or token for one is kept in key management and used through the network edge
    (FR-19, ADR-0037). The remaining git-specific parts go: the agent profile's "project link" switch, project keys in
    Settings › Keys, the "Git credentials" label, and the API's git address on new workspaces and tasks (over two
    releases: first deprecated and unused by the web, then removed).
23. **The network extras (8 Oct 2026, ADR-0035 §2):** the host:port allow-list with wildcards, default deny, is
    confirmed as v1.0's design, as built. "Recently refused" with "Allow this site" in Settings › Internet access is
    built after c16. Per-profile network lists come when someone asks for them. Observe mode is dropped: it would
    switch the only guardrail off for a while. None of them asks a person to approve anything.
24. **A desktop sign-in lasts one run (8 Oct 2026, ADR-0039 §6):** in c16, a sign-in a person makes during Take over
    lasts for the agent's next run only, then the desktop's browser forgets it, so work inside an account means a Take
    over and a sign-in before each message. Keeping a sign-in for the whole task is for a later release.
25. **The 24-hour soak's growth rule (9 Oct 2026, TEST-PLAN SOAK-001):** the gateway's memory (RSS) keeps M5's
    rule, under 5 % growth over the 12 h lifetime after its first hour. Open descriptors and database connections no
    longer take that rule. Descriptors other than the gateway's database connections (each is a socket) take the 5 %
    rule. The gateway's database connections are judged against the pool: never above its maximum plus the gateway's
    listeners, at the maximum in under 90 % of the last hour, and their floor up by at most 2 from the first three
    settled hours to the last three. The probe's own database session is no longer counted as the gateway's. Why: the
    pool's normal swings (4 to 16 connections within an hour) failed the 5 % rule on both soaks it judged, c15's of 6
    and 8 Oct 2026, while the other descriptors stayed flat; and the probe counted itself (the 17 seen was the pool's
    16 plus the probe). The stated blind spot: a leak of 3 or fewer held connections per 12 h can pass, and a run with
    no gateway lifetime of 11 h or more (an 8 h run) doesn't judge the floor and says so; the 24 h run fails if it
    has none. **Proposed amendment (9 Oct 2026, held for the owner's wording):** a fourth check closes that blind
    spot for a connection taken and never used again. After the first hour, no more of the gateway's database
    connections may stay idle for over 25 minutes than its listeners (none in the soak). A healthy pool puts its
    connections back into use; a connection taken and never used again idles for good, so it fails about 25 minutes
    after being taken, in any lifetime, the 8 h run's included. A leaked connection still used now and then is left
    to the floor check. The 25 minutes are the pool's own bound with no traffic at all, and a later soak than 15b's
    calibrates it.
