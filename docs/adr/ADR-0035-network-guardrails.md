# ADR-0035: Guardrails at the network boundary, with no per-call permission policy and no approvals

## Status
Accepted (8 Oct 2026). The owner decided the removal of the per-call policy and approvals (§1, §3 to §7), the git
push variant (§5) and, on 8 Oct, the network extras in §2: point 1 confirmed, point 3 built after c16, point 2 when
someone asks for it, point 4 dropped ("The owner's decision on the network extras", at the end).

## Date
2026-09-29

## Context
The product owner decided on 29 Sep 2026 that the product must not rely on a human approving or rejecting the
agent's actions. The decision is recorded in:
- PRD §6, the **Guardrails** bullet (amended 29 Sep): the sandbox is the wall and the network is the guardrail;
- PRD **FR-10**, *superseded*: there is no per-call permission policy and no approval step; FR-9, FR-18, FR-19,
  FR-23, FR-26, FR-27, G1, G3, M7 and the risks follow;
- DEV-PLAN **Phase 9 item 4**: the removal after candidate 7, with this ADR.

The owner's principles, which this ADR implements:
- **Inside the sandbox** the agent may read, write and run any command freely.
- **The only guardrail we add is the network:** which sites and ports it may reach. There are no approvals and no
  special rules for git or anything else.
- **A blocked connection** tells the agent which host and port were refused. That's information, not another
  guardrail.
- **TensorQuay Agent is a general-purpose agent harness**, for research, data, operations and writing tasks as well
  as code. Nothing here assumes a task writes code.

Today the loop consults a permission broker on every call:
- a baseline policy (`read-only`, `workspace-write`, `unrestricted`) plus admin and profile rules decide `allow`,
  `deny` or `ask` (ARCHITECTURE §5.4);
- `ask` sends a `permission.request` that a person answers, bound to a prepared action;
- a run that waits long parks and releases its sandbox;
- every agent `git_push` is asked once (the "push floor"), and the Git relay applies branch rules of its own.

The sandbox, the egress allow-list and the credential boundary already exist, and stay.

## Decision

### 1. Inside the sandbox: no per-call decision
Every tool the task has runs when the model calls it: no broker, no rule, no `ask`, no human in the loop.
- **The task's tools set what the agent can do**, through the profile's tool choice (ADR-0012), which stays. A task
  given no shell or write tools can't run commands or write, because it hasn't got those tools. This is not a
  per-call check.
- **The builtin file tools keep their path contract.** Their paths mean `/workspace` (TOOL-S-002); that is what the
  paths mean, not a permission. The shell reaches the whole sandbox, as today.
- **The decision engine's role is unchanged** (ADR-0013). It can narrow the step's tools or stop, and it can't widen
  the task's tools or the network. Its `act` runs like a model call: schema, then dispatch.
- **Git in the sandbox runs without repository hooks** (the security review; the technical lead, 29 Sep). The runtime
  sets environment configuration for every process in the sandbox (`GIT_CONFIG_COUNT`, `GIT_CONFIG_KEY_n`,
  `GIT_CONFIG_VALUE_n`, which outranks a repository's own config) with `core.hooksPath=/dev/null` and
  `core.fsmonitor=false`. So every `git`, the shell's included, ignores hooks and a file-system monitor planted in
  `.git`. This isn't a rule the agent meets: it keeps one member's shared workspace from running code as another
  member (below). Other repository-local config that runs commands (filters, aliases, `core.sshCommand`, diff
  drivers) can't be switched off this way; the residual risk is accepted in the security analysis.
- **Spawned agents get at most the caller's reach** (ADR-0034, v1.1). ADR-0034 made a spawned member's policy the
  stricter of its profile's and the caller's; with no policy, the no-widening rule rests on what's left: a spawned
  agent's tools, connectors and network list are each at most the caller's (intersected, or the spawn is refused).
  Otherwise per-profile lists would make `agent_spawn` the way to a wider list. This amends ADR-0034.

### 2. The network is the guardrail
Every connection from a sandbox is decided outside it, at the gateway's egress proxy, by host and port. The
private-address guard, gateway-side DNS and live settings (ADR-0021, ADR-0028) stay as they are. **Ingress stays
closed:** nothing listens inside a sandbox (FR-17), since the runtime dials out.

**The gateway's own outbound paths keep their destination checks**, and the task's effective network list applies to
them too:
- the egress proxy checks every keyed host, git hosts included, against the list when a key is minted and at every
  use (ADR-0037 §7, which removed the Git relay);
- the MCP proxy keeps the conversation's connector set, each connector's own tool allow or deny filter (a property
  of the connector, not a per-call policy) and the private-address guard (SEC-024);
- with per-profile lists (point 2 below), a task can use only the git hosts and connector hosts its own list allows.

**In v1.0, after candidate 7** (the finer control proposed by the technical lead; points 1 to 4 were **decided by the
owner on 8 Oct 2026**, at the end):
1. **A host:port allow-list with wildcards, default deny.** This is what exists today, kept and documented:
   - exact hosts or `*.domain` for any subdomain (never the domain itself);
   - explicit ports, up to 16 per rule;
   - up to 256 rules, default deny except the gateway.
2. **Per-profile network lists.** The install's list is the default for every task.
   - **A shared profile** (made by an administrator) may carry its own list, which replaces the default for the
     tasks made from it. That's how an administrator gives one kind of task hosts others don't get.
   - **A personal profile** may only narrow the default: its list is intersected with it, so a member can never
     widen what the administrator allowed.
   - A task keeps the list its profile had when it started, as it keeps the rest of its profile.
3. **A refusal the agent sees, and "Allow this site" for the administrator.**
   - The agent's side is §3.
   - For the administrator, **Settings › Internet access** shows the recent refusals: host, port, reason, how often,
     when last, and which list refused them. A bounded, in-memory record of the last 200 refused destinations is
     enough, since the log keeps the rest.
   - Each refusal has **Allow this site**, which adds a rule for that host and port to the list that refused it.
     Nothing waits for it: the task has already moved on, and a later connection is decided by the new list. Each
     change is recorded in the settings audit log.
4. **An observe-only mode for new rules.** A list, the install's or a profile's, can be put in **observe** mode for a
   bounded time, 24 hours at most.
   - Connections that aren't on the list go through and are recorded as "would be refused". That lets an
     administrator build a list for a new kind of task from what it actually reached.
   - The private-address guard still refuses in observe mode.
   - When the time runs out, the list goes back to enforcing.
   - Observe mode is an administrator's setting only, and every change of it is audited.
   - On the install's list, observe mode would lift egress for every task on the install. The technical lead
     recommends allowing it **on a profile's list only** (a new kind of task). The owner dropped point 4 (8 Oct
     2026).

**Later, v1.1, opt-in (named here, not decided):**
5. **HTTP method and path rules**, which need TLS interception at the proxy. That's a separate ADR, because it
   changes what the gateway sees of a task's traffic.
6. **A per-task outbound data cap:** bytes sent through the proxy, counted per task, beside the existing budgets.

### 3. A blocked connection names the host and port: the simplest path that works
**What happens today** (checked in `crates/tqa-gateway/src/egress/proxy.rs`):
- the proxy answers a refused connection with `HTTP/1.1 403 Forbidden` and an **empty body**;
- the host, port and reason reach only the gateway log and the `egress_denied` notice for the person watching;
- `fetch_url` already names the target in its refusal;
- other clients (`curl`, `pip`, `npm`, `git`) see only "403", so the agent can't tell a blocked host from a broken
  server.

**What changes (two small pieces, no new protocol):**
1. **The proxy's refusal says it.** The status line's reason phrase and a one-line `text/plain` body carry the host,
   port and reason. For example:
   `HTTP/1.1 403 Blocked by the network allow-list: api.example.com:443`, with the body "Blocked by the network
   allow-list: api.example.com:443 is not allowed. Ask an administrator to allow it, or use an allowed host." The
   reasons are those there are today (not on the list, a private or reserved address, egress disabled), in plain
   words. Clients that report the reason phrase show it as it is: Python's `http.client` (so `pip` and `requests`)
   prints "Tunnel connection failed: 403 Blocked by the network allow-list: …". Clients that show the body, such as
   a plain-HTTP fetch, show the sentence.
2. **The agent knows the rule before it tries.** On by default: the environment section of the system prompt says
   that network access goes through the proxy and lists the task's allowed hosts and ports (its profile's list, or
   the default). The list is bounded, with the rest named as "and N more". It's administrator configuration, not a
   secret, but it goes to the model provider with every prompt, so **the prompt lists host names only**: entries
   that are literal addresses (often an internal network's, allowed on purpose) are left out and counted in "and N
   more" (the technical lead, 29 Sep). Host names are shown as they are; there is no per-entry "not shown" flag for
   now.
   - The gateway sends the list to the runtime in `session.config`, a field added in the same revision as the
     removal (below), and the runtime renders it into the prompt. The field carries at most 32 host names; the
     protocol refuses an address in it.
   - The list is the one the task started with, as the rest of its profile is. A list changed while the task runs
     applies at the proxy at once and reaches the prompt at the task's next session.

**Is a link frame to the runtime needed? No, not for this.** Some clients print only the status code; libcurl
prints "CONNECT tunnel failed, response 403", so `curl` and `git` over https do. Even then, the agent sees a 403
from a proxy for a host that isn't in the list its prompt gave it, which is enough to act on. A frame that pushes
each denial into the running command's result would add a protocol revision, a runtime change and tests for little
gain, so it's left out. If E2E runs show agents misreading bare 403s, it can be added later as its own change.

**Connections that bypass the proxy** (raw TCP, SSH on port 22, a tool that ignores `HTTPS_PROXY`) have no route
from the sandbox and never reach the gateway, so nothing can name them. The prompt says to use the proxy. Capturing
such connections is not part of this decision.

### 4. Outside systems apply their own rules; credentials stay in the gateway
- **Git hosts, APIs and connectors decide** what the credentials the gateway adds may do: branch protection, token
  scopes, connector permissions. Their refusals go back to the agent as they are.
- **Credentials never enter the sandbox** (ADR-0033, FR-19, FR-24), unchanged.
- **The admin guide tells administrators** to protect important branches on the git host, to use narrowly scoped
  tokens, and to give connectors least-privilege credentials.

### 5. Git push: the host decides (the owner's choice)
A push is plain git with a host key (ADR-0037 §7, which removed the Git relay), and the git host's rules decide.
- **The key stays in the gateway:** the sandbox holds an inert, per-run placeholder, and the egress proxy puts the
  value in for a keyed git host (ADR-0037).
- **It gains no approval and applies no branch rules of its own:** no protected branches (`main`/`master` or the
  workspace owner's patterns) and no new-branch-only check.
- **A push without a stored key fails**, as the host refuses it.

### 6. What stays
- **The sandbox:** container or microVM, read-only root, unprivileged user, dropped capabilities, seccomp, resource
  limits.
- **The network:** the allow-list and proxy, the private-address guard, gateway-side DNS, closed ingress.
- **The credential boundary:** gateway-held credentials; the model relay, the MCP proxy and the egress proxy add them.
- **The profile's tool and connector choice.**
- **Budgets and limits:** steps, tokens, wall clock, relay rate, live sandboxes, the engine's budget (ADR-0013).
- **The administration audit log of settings changes:** a record, not a gate. There is no per-action audit or
  confirm.
- **Channel decisions between members** (ADR-0014: proposals, votes, veto): how members agree, not a check on tool
  calls.

### 7. What is removed
- **Policy and rules:**
  - the baseline policies (`read-only`, `workspace-write`, `unrestricted`);
  - admin and profile `allow`/`deny`/`ask` rules, and the rules' origins (revision 9);
  - the deny floor on writes to git internals.
- **Asking:**
  - the broker's `ask`, prepared-action bindings and session grants;
  - `permission.request`, `permission.decision` and `permission.forward`;
  - `permission.resolved` and `permission.expired`;
  - the `denied` field of `tool.result` (revision 9).
- **Waiting:**
  - parking while waiting for approval: `run.parked` and the runtime's and gateway's approval parking;
  - the Limits setting **Park a waiting run after** (`sandbox.park_after_seconds`), its floor and its guide row.
- **Git:**
  - the human floor on the agent's `git_push`;
  - the push's approval link;
  - the Git relay's branch rules: protected branches, new-branch-only, and the `git_push_protected_branch` and
    existing-branch refusals;
  - a workspace's push settings (`push_protected_branches`, and `push_enabled`, since the stored key decides).
- **Connectors:** the ask-class connector approval (ADR-0031 decision 2, GW-I-073). The profile's connector choice
  stays.
- **Configuration and interfaces:**
  - `[permissions]` in `gateway.toml` (`policy`, `rules`, `approval_timeout_seconds`);
  - a profile's `policy` and `rules`;
  - the approval API;
  - the web approval prompt, the connector approval card, the profile rules editor, the Limits park row and the
    workspace push settings.
- **The E2E harness's approval answering** (`tqa-testkit` e2e approvals).

## Amendment (1 Oct 2026): internal servers by name

**The owner's decision (1 Oct 2026), asked by the lead in plain words:** yes, an administrator may allow a *named*
internal server per rule: administrators only, off by default, with protection against DNS rebinding. Found while
planning the host-keys stack suite: a rule naming a host never reaches a private address (§6, the private-address
guard), so an internal git server known by name had no way in, by keys or the Git relay.

- **The switch:** a rule's `internal` (API and `[[egress.allow]]`), off unless set, only on one exact host name (never a
  wildcard, the any-host rule or an address: an exact-address rule already reaches its address). Refused otherwise
  with `internal_name_only`; the database refuses it too.
- **What it reaches:** the private ranges (RFC 1918, RFC 6598 shared space, IPv6 unique-local) beside public addresses.
  Never loopback, link-local (cloud metadata), unspecified, multicast or broadcast; and never the gateway's own ground:
  the container engine's networks and their gateways (read from the engine and renewed every minute), the sandbox pool,
  the gateway's own addresses, and this machine's own addresses.
- **This machine's addresses (the lead, 1 Oct, ENGINEERING-GUARDRAILS §1.4):** a gateway in a container can't see them,
  so `[egress] internal_never` must name them while internal rules exist; until it does, internal rules refuse, and the
  admin list, the task's notice and `tqa-gateway check` say so in words. A gateway in the host's network namespace (it
  holds an engine bridge's gateway address) finds them itself, and the setting is optional.
- **Rebinding:** each connection resolves once, every address it got is checked, and only that checked set is
  connected to; one forbidden address refuses the whole answer. An engine inventory unknown or older than three minutes
  refuses (fail closed).
- **Nothing else changes:** a rule without the switch keeps the guard of §6; an exact-address rule keeps reaching its
  address.

Tests: SEC-050 to SEC-054.

## Security analysis
| Removed check | What it protected | What covers it now, or the risk the owner accepts |
|---|---|---|
| `ask` on network and communication tools | A person saw each outbound request first | The allow-list, per profile, decides every connection outside the sandbox, and the sandbox holds no credentials. **Accepted:** data can go to an allowed host without a person seeing it, so keep lists narrow; observe mode helps build them |
| Admin deny/ask rules on commands and paths | Stopped chosen commands or paths | The sandbox is disposable and its only lasting state is its workspace; nothing outside the sandbox is reachable. **Accepted:** the agent can change or delete its own workspace's files; forks and anything already sent out elsewhere recover them |
| `read-only` baseline and a profile's stricter rules | Tasks meant only to read | A profile gives such tasks only reading tools, and a narrow network list |
| The deny floor on git internals | Hooks or config planted through file tools | The gateway never runs git or any tool against a workspace on the host (ARCHITECTURE §12), so anything planted runs only in a sandbox. In a **shared workspace** (ADR-0034, GW-I-031, SEC-017) the repository is also another member's: every git in the sandbox runs with hooks and the file-system monitor switched off (§1), so a planted hook can't run in another member's task with that member's runtime token, connectors or push authorization. **Accepted:** the agent can damage its own repository metadata; and **sharing a workspace means trusting the other member's agent** as far as the repository config git can't switch off (filters, aliases, `core.sshCommand`, diff drivers): those can run code in your tasks |
| The push floor and the relay's branch rules | Work leaving under review; no overwrite of protected or existing branches | The credential stays in the gateway (ADR-0037's host keys), and the git host's branch protection and token scope apply. **Accepted:** where the host protects nothing, the agent can push to any branch its key reaches. The admin guide says to protect branches and scope tokens |
| The spawned member's stricter policy (ADR-0034) | A spawn reaching more than its caller | A spawned agent's tools, connectors and network list are each at most the caller's: intersected, or the spawn is refused (§1) |
| Ask-class connector approval | A person confirmed destructive connector actions | The connector credential's scopes and the remote system's permissions, and the profile's connector choice. **Accepted:** an allowed connector acts without a person |
| Prepared-action binding (SEC-003) | A forged or replayed approval | Moot: there are no approvals |
| Approval parking | A sandbox's resources held for a person's answer | Moot: nothing waits for a person |
| Session grants | The scope of "allow for this task" | Moot |

The new network controls carry risks of their own:
- **Observe mode opens egress for its time**, so it's bounded to 24 hours, audited, administrator-only, and still
  refuses private addresses.
- **A shared profile's list can widen** past the default, so only administrators set it, and each change is audited.

The residual risk is exfiltration or unwanted action through what the administrator allows: allowed hosts, and the
reach of the keys and connector credentials. The admin guide says so plainly.

## Protocol and upgrade path
"Released" means a tagged release (v1.0), not a candidate or a push to main (the technical lead, 29 Sep). v1.0 isn't
tagged, so nothing is deprecated for a release first: what goes is removed outright.
- **The revision.** Revision 9 is unreleased by that definition, even after candidate 7 reaches main, so the removal
  and the prompt's allow-list change revision 9 itself, with no new revision:
  - `session.config` loses the policy (baseline, rules with their `origin`, the approval timeout) and the
    workspace's push settings, and the MCP snapshot loses the connector's permission class;
  - it loses `denied` on `tool.result`, and the approval frames and events: `permission.request`,
    `permission.decision`, `permission.forward`, `session.park`, `permission.resolved`, `permission.expired` and
    `run.parked`;
  - it gains the allow-list in `session.config`: `network`, at most 32 entries of host and ports in the proxy's
    order, with `more` counting the rest;
  - `gateway_revision` stays, since the gateway announcing its revision is useful on its own;
  - **the floor rises to 9** (the technical lead, 29 Sep): a revision-8 runtime can't read a `session.config` without
    the policy, so the gateway refuses it as too old rather than keep sending it a policy nothing uses.

  Runtimes built from candidates before this change are pre-release: environments from those images are rebuilt from
  the new runtime image, as the upgrade notes say.
- **Stored histories keep loading** (the technical lead, 29 Sep). The gateway and the runtime read every stored frame
  back through the protocol's parser, and a stored frame can sit in a blob no migration rewrites. So the types that
  histories written before this change can hold stay as **retired, read-only markers**: they parse the old stored
  payload with its fields, keep none but the sequence, are never sent, are refused from a runtime, and the web
  ignores them on replay. The retired types are:
  - the server frame `permission.request` (it keeps its `seq`, which the store checks);
  - the events `permission.resolved`, `permission.expired` and `run.parked`;
  - the sandbox state `parked`, in `sandbox.state` and in `session.ready`'s summary;
  - the error code `permission_binding`, in stored `error` frames and failed runs.

  A stored `tool.result`'s `denied` needs no marker, since events ignore unknown fields. The approval card of a
  connector call was a `permission.request` too, so nothing else is stored. Client frames are never stored.
- **Runs waiting for approval when the change is deployed** end as failed with a notice ("approvals were removed;
  send a message to continue"). Their waiting call never runs unapproved.
- **Stored data and configuration:**
  - the approval tables are dropped in the removal's migration: `permission_bindings`, session grants, the
    connector approval records and the git push operations' approval link;
  - the `sandbox.park_after_seconds` setting row and the workspace push settings (`push_protected_branches`,
    `push_enabled`) are dropped;
  - stored profiles lose `policy` and `rules` in the same migration;
  - `[permissions]` in `gateway.toml`, and `policy` or `rules` in a profile file, are refused at start with a message
    that names the file and key and says what replaced it: "[permissions] was removed: inside the sandbox the agent
    works freely, and the network allow-list ([egress]) is the guardrail (ADR-0035)". There is no silent ignore and no
    warning period.
  - **Existing allow-lists are unchanged:** the install's list becomes the default list, and no profile has a list of
    its own until an administrator gives it one.
- **The API:** the approval endpoints are removed, so a request to them gets the gateway's ordinary 404. Clients that
  answered approvals (FR-26) no longer need to, and the OpenAPI document drops them.
- **Documents:**
  - the upgrade notes, for installs of pre-release candidates: rebuild environments; `[permissions]` and profile
    `policy`/`rules` must be deleted before start; the other stored settings go by themselves;
  - the admin guide: permissions and approvals out; the network section (lists, profiles, recent refusals, observe
    mode), host-side branch protection and token scopes in;
  - the user guide: no approval prompts.

## Documents this changes
- **ARCHITECTURE:**
  - §5.4 Permissions, replaced by "Guardrails: the network boundary";
  - §4.2 egress, for per-profile lists, recent refusals and observe mode;
  - §5.5 the resume point, dropping its approval case;
  - §4.4 parking for approvals;
  - §4.7 computer use: the line that sends risky actions (typing into a password field, submitting a form) through
    the Phase 3 approval flow goes;
  - §12 Security model: the exfiltration, self-approval and git rows;
  - §14 the proxy's refusal and the prompt's list;
  - §15.1 the decision engine's line.
- **TEST-PLAN:**
  - the CORE, TOOL, PROTO, RT, GW, WEB, TK, SBX and SEC sections;
  - the Gate 9 and FR-10 traceability rows.
- **ADR-0034** (team runs): a spawned member's reach is at most the caller's tools, connectors and network list, in
  place of "the stricter policy" (§1).
- **PRD:** already amended (1b7f7f6b). If the owner agrees to §2's points 2 to 4, FR-15 and FR-22 gain per-profile
  lists and observe mode.

## Tests that go or change
- **Go:**
  - CORE-U-013, CORE-U-014, CORE-S-015 (ask, expiry, binding);
  - TOOL-U-014 (rules), TOOL-S-021 (the push floor);
  - SEC-003 (forged approval);
  - GW-U-050, GW-I-085 (profile rules);
  - GW-U-077, CORE-U-061, PROTO-U-025 (policy origins and `denied`);
  - GW-I-036 (parking), GW-I-073 (connector approval);
  - WEB-U-003 (approval prompt), TK-U-001 (test client approvals).
- **Change:**
  - RT-I-016 and GW-I-014 (a push without approval or branch rules; the host's refusal passes through);
  - GW-I-128 (revision 9's `session.config` additions: `gateway_revision` stays, the policy origins go, the allow-list
    comes) and PROTO-U-008 (the revision table);
  - SEC-019 (unchanged in intent: the credential appears nowhere outside the gateway; other-repository, reused and
    expired authorizations are still refused);
  - CORE-U-038 (resume point without the approval case);
  - CORE-U-033, CORE-S-034, SEC-036 (the engine can't widen the task's tools or the network);
  - CORE-U-056, TOOL-U-028 (no prepared-action approval or rule matching);
  - GW-U-038, GW-U-044 (the MCP proxy without the ask-class context);
  - GW-U-049, GW-I-025 (profiles without `policy` and `rules`, with a network list);
  - WEB-U-045 (the connector card without approval);
  - TK-U-004, TK-U-006 (the harness without approvals);
  - E2E-TEAM-011 (its two pushes without approvals);
  - SBX-C-004, GW-I-012 (the refusal's reason phrase and body name host and port);
  - SEC-037 (a spawned agent can't widen: its tools, connectors and network list are at most the caller's, where it
    tested a profile looser than the caller's policy).
- **New:**
  - the refusal's reason phrase and body for each reason;
  - the prompt's bounded list, with literal addresses left out and counted;
  - a planted hook in a shared workspace: member A's agent writes `.git/hooks/post-checkout` and `core.fsmonitor`,
    member B's agent runs `git status` and `git checkout` in the shell, and nothing planted runs (nothing of B's is
    reached);
  - a shared profile's list replacing the default, and a personal one narrowing it;
  - the recent refusals and "Allow this site", with its audit row;
  - observe mode: allowed and recorded, private addresses still refused, back to enforcing at its time, audited;
  - the upgrade: `session.config` carries the allow-list and the prompt shows it bounded; a waiting run ended with its
    notice; `[permissions]` and profile `policy`/`rules` refused at start with the message; the migration drops the
    tables and settings; the approval endpoints answer 404; revision 9's table without `denied` and the origins,
    with the allow-list.
- **Stay:** SEC-001, SEC-002, SEC-004, SEC-005, SEC-006, SEC-018, SEC-022, SEC-024 and the rest of SEC except SEC-019,
  SEC-036 and SEC-037 (above): the boundary.

## Consequences
- **Less code:** the broker, approvals, approval parking, the relay's branch rules and their UI go. The network
  controls grow in one place, the allow-list, with no approval step anywhere.
- **No stalls:** a task no longer waits for a person, and a refused connection explains itself in the proxy's answer
  and against the list the agent was given.
- **Where the risk moves:** the administrator's lists and the reach of the keys, both covered by the admin guide.
- **Order:** after candidate 7:
  1. the proxy's refusal text and the prompt list, which don't depend on the removal;
  2. the removal, with the revision bump, the migrations and the docs;
  3. per-profile lists, recent refusals with "Allow this site", and observe mode, once the owner agrees.

## The owner's decision on the network extras (8 Oct 2026)
Asked by the lead with a one-page brief. Every option kept the owner's principles: the network is the only guardrail,
nothing asks a person to approve, and it stays simple. §2's points 1 to 4:
1. **The host:port allow-list with wildcards, default deny: confirmed** as v1.0's design, as built.
2. **Per-profile network lists: when someone asks for them**, not planned now. They would add a second place where the
   rules live. Until then, an administrator who wants one kind of task to reach a site allows it for every task.
3. **Recent refusals with "Allow this site": built after c16** (DEV-PLAN §4b), as point 3 describes: an
   administrator's tidy-up of the list, never a gate.
4. **Observe mode: dropped.** It would switch the only guardrail off, if only for a while; point 3 builds a list from
   real refusals without opening anything.

Points 5 and 6 stay later, as named in §2.
