# The administrator's Sandboxes page and the leak verdict

**State:** proposed, 17 Nov 2026, for the release after 18.0. **Decides:** the Product Owner (what the page shows and
the privacy rule, both already given on 7 Nov 2026), the technical lead (the API, the sweep accounting, the test rows).
**Builds on:** ARCHITECTURE §4.4 (lifecycle, activeness, admission), ADR-0016 (activeness and admission), ADR-0021
(sandbox networking and its sweeps), ADR-0039 §10 (a Desktop keeps its place), the Settings menu's three groups
(WEB-U-075), SEC-005 (another person's task is metadata only), and the 1 Oct 2026 contract for this page (its four
decisions stand: Stop is allowed on a kept-on task with a warning; the task's owner is told; waiting tasks are a
count only; totals against the host come later).

## The problem

The gateway recycles sandboxes on its own: idle retire after the idle window, a lifetime with a grace, start-up
reconciliation of what the engine holds (GW-I-011, SBX-C-008), a five-minute sweep of stray containers (GW-I-191) and
of orphaned networks (SBX-U-006), helpers labelled and retired with their team (SBX-F-024), and the 24-hour soak's
"no orphan sandboxes or volumes" (SOAK-001). What an administrator cannot do is see it. Nothing lists the sandboxes
running across all people, nothing says whether a sweep left something behind, and the only way to free a slot held by
someone's idle sandbox is to wait for its window. The owner asked for three things (7 Nov 2026): a page that shows
every sandbox on the engine with its owner, environment, age, state and resources; a Stop on each row for an
administrator; and an explicit leak verdict on Status, with the same numbers as metrics. The page shows sandboxes
only: no task title, no link, no content, no message counts, because the administrator never reaches other people's
tasks (People shows counts and never content; the same line holds here).

## 1. Where it lives

**Settings › Operations › Sandboxes**, address `/settings/sandboxes`, between Usage and Status, administrators only (a
member who opens the address lands on Account, as Overview does). Not a tab of Environments, for three reasons:
Environments is configuration (the templates, their images and sizes), while this page is what runs right now, which
is what the Operations group holds (Usage, Status); the leak verdict lives on Status, two lines away, so an
administrator reading "oldest 7 h" on Status finds the sandboxes under the same heading; and Environments' page already
carries a per-environment detail view and dialogs, where a cross-people table would be a second page inside a page.
The Settings menu grows from 14 items to 15 (the earlier contract checked 13 to 14 at 900 px high; the capture is
rechecked in phase 2).

## 2. The API

Two endpoints, administrators only (`require_admin`, by session or personal token, as every `/admin/` route), both in
`openapi.json` with `additionalProperties: false` on every object so nothing joins the answer by accident, and both in
`web/src/settings/operations.ts` so the parity check reads them.

### 2.1 `GET /api/v1/admin/sandboxes`

```jsonc
{
  "backend": "docker",                 // the install's backend; a conversation never changes backend
  "limits": {
    "slots": 4,                        // admission.slots (Tasks running at once)
    "per_user": 4,                     // admission.per_user
    "desktops": 2,                     // the Desktop environments at once limit
    "idle_seconds": 600,               // sandbox.idle_seconds, for the page's one-line explanation
    "max_lifetime_seconds": 28800
  },
  "totals": {
    "in_use": 3,                       // sandboxes holding a slot: starting + working + idle + stopping
    "starting": 0, "working": 1, "idle": 2, "stopping": 0,
    "desktops": 1,
    "waiting": 2,                      // tasks waiting for a slot: a count, never who (decision 3 of 1 Oct)
    "reserved": { "cpu_millicores": 6000, "memory_bytes": 12884901888 },   // the sum of the listed sandboxes' sizes
    "usage": { "cpu_millicores": 1400, "memory_bytes": 3328599654, "measured_at": "2026-11-17T01:26:05Z" } // or null
  },
  "items": [
    {
      "conversation_id": "0195…",      // needed by Stop; the page never renders it and never links it
      "owner": { "id": "0195…", "email": "ben@example.org" },
      "environment": { "name": "minimal", "display_name": "Minimal", "desktop": false },
      "state": "idle",                 // starting | working | idle | stopping
      "started_at": "2026-11-17T00:12:00Z",         // sandbox_instances.created_at
      "last_activity_at": "2026-11-17T01:20:00Z",   // sandbox_instances.last_action_at
      "stops_at": "2026-11-17T01:30:00Z",           // idle: the window's end; kept on: the lifetime's end; else null
      "kept_on": false,                // conversations.pinned, or keep_sign_in on a Desktop
      "size": { "cpu_millicores": 1000, "memory_bytes": 2147483648, "pids": 512 },
      "usage": { "cpu_millicores": 20, "memory_bytes": 412090368, "pids": 7 },   // or null until measured
      "can_stop": true,
      "stop_refusal": null             // "starting" | "working" | "stopping" | "work_waiting" when can_stop is false
    }
  ],
  "next_after": null                   // a page token past 200 items (max_live goes to 4096)
}
```

- **One consistent read** of the store under the gateway's `io_timeout`: totals and rows come from the same query, so
  "3 of 4 in use" matches the rows. The read never calls the engine; usage comes from the sampler's last values (§3.4).
- **Privacy rule, as a test (GW-U-220, SEC-064):** the answer carries no title, message, file or workspace name or
  path, branch, model route, agent profile, channel or message count, for other people's rows and the administrator's
  own alike (one rule is simpler to keep than two). The test plants those strings in the fixture and searches the body.
  The same rule covers the audit row, the log lines and the page's status line: owner email and conversation id only.
- **Order:** working, starting, stopping, then idle by soonest `stops_at`; the people most affected first.
- **Failures:** typed, as everywhere: `401`, `403 forbidden` (not an administrator), `503 store_unavailable`. A broken
  sampler never fails the read: `usage` is null.

### 2.2 `POST /api/v1/admin/sandboxes/{conversation_id}/stop`

- `Idempotency-Key` required, as the other actions; a repeat of the key answers the first result.
- Body: none. `202` with `{ "stopping": true }` means the retire is requested.
- Refusals, decided under the admission lock so a run cannot start between the check and the retire: `409 working`
  (a run is active, or the sandbox is `starting`), `409 work_waiting` (a message is queued or sending for it),
  `409 stopping` (already on its way: answered `202` again, not an error), `404 not_found` (no live sandbox).
- A kept-on sandbox is stoppable (decision 1): the page shows the warning; the API treats `kept_on` as information.
- Audited (§3.5) and counted (`tqa_sandbox_retired_total{reason="administrator"}`).

## 3. Where each field comes from in the gateway

### 3.1 The rows: the admission records
`sandbox_instances` (migrations 0014, 0016, 0063) joined to `conversations`, `users` and `templates`:

| Field | Source |
|---|---|
| `conversation_id`, `started_at`, `last_activity_at` | `sandbox_instances.conversation_id`, `created_at`, `last_action_at` |
| `state` starting / stopping | `sandbox_instances.phase` = `starting` / `destroying` |
| `state` working / idle (phase `live`) | `events::run_activity` (`RunActivity::Active` = working), the same call eviction makes: activeness is the lifecycle events, never processes (ARCHITECTURE §4.4) |
| `owner` | `conversations.owner_id` → `users.email` |
| `environment` | `conversations.template` → `templates.display_name`, `'desktop' = ANY(capabilities)` |
| `kept_on` | `conversations.pinned`, or `keep_sign_in` on a Desktop (18.0), the same predicate the worker uses |
| `size` | `sandbox_instances.cpu_millicores`, `memory_bytes`, `pids` (0063: each sandbox keeps the size it was admitted with) |
| `stops_at` | idle and not kept on: `last_action_at + idle_seconds` (the row's own `idle_seconds`, else the current limit); kept on: `created_at + max_lifetime_seconds`; working or starting: null |
| `totals.waiting` | the admission queue: `idempotency.execution_status IN ('queued','sending')` with no slot held, as `admission::waiting()` counts it (GW-I-173) |
| `limits` | `settings/limits.rs` keys `admission.slots`, `admission.per_user`, `sandbox.idle_seconds`, `sandbox.max_lifetime_seconds`, the Desktop limit |
| `backend` | `config.backend_name()`, as `/readyz` reports it |

`stops_at` is computed from the record, while the worker's authority is its own idle clock (`manager/idle.rs`, a
monotonic clock fed by `action()` and the frames it observes). The two can disagree by one worker pass; the page
shows "stops in about 6 min", never a countdown to the second. Publishing the worker's clock to the read is the
alternative (open question 2).

### 3.2 The state: the manager's idle and lifetime state
Starting and stopping come from the phase; working and idle from activeness; the window and the lifetime from the
row's `idle_seconds` and `max_lifetime_seconds`, which the worker reads the same way (`worker.rs`, the SELECT at its
activity refresh). Nothing new is persisted for the page.

### 3.3 Stop reuses the existing stop path
A new `Teardown::Administrator` variant beside `Eviction`, `Idle`, `Lifetime`, `EnvironmentSwitch`,
`WorkspaceDeleted` and `TeamStopped`, delivered through `SandboxManager::retire_for_switch(id, reason)`, exactly as
eviction, an environment switch and a workspace delete deliver theirs: the slot's `switch` flag is set, its worker
wakes, `lifecycle::retire` destroys the sandbox, the open streams get `session.ready` with `destroyed`, the record goes,
and the owner's next message starts a new sandbox on the same workspace (the files and history were never in the
sandbox). The handler's checks are eviction's predicate without the pinned and keep-sign-in exclusions (decision 1),
taken under `admission.evicting`'s lock as eviction does, so a start that lands between the check and the retire is
refused with `409 working` instead of being killed. A Desktop under Take over or holding a sign-in follows ADR-0039 §10
as eviction does (`screen::may_retire`): the stop is refused with `409 working` and the reason "a person has its
screen". No protocol frame changes; an older runtime sees the same retire it sees today.

**Telling the owner (decision 2):** the retire reason reaches the task as a notice in its transcript, the way 18.0's
kept-sign-in notices do (a gateway-written notice naming who made the change, worded by the web without a name):
"An administrator stopped this environment at 14:32. It starts again when you send a message." The open stream already
hears `session.ready destroyed`; the notice is what a person reads after a reload (open question 3 names the
mechanism to reuse).

### 3.4 CPU and memory: the backend's stats (FR-13)
`SandboxBackend::stats(handle)` exists on both backends and returns cumulative CPU milliseconds, resident memory, a
process count and the uptime; nothing calls it today. A **sampler** in the manager calls it for every live sandbox
every 10 s, each call bounded by the backend's timeout and at most four in flight, keeps the last two samples in memory
and derives the CPU rate from the delta (so the first value appears after the second sample, and the page says
"Measuring…" for 10 s). It never writes the store and never fails the read. The cost is one engine call per live
sandbox per 10 s; past 64 live sandboxes the interval stretches with the count, the same bound `live_usage.rs` puts on
the workspace quota walk. On Firecracker the memory figure is the VM process's resident set, which grows toward the
VM's size; the page labels it "Memory held by microVMs" there.

### 3.5 The audit record
`admin_audit` keeps every administrator's change by area, item and action (migrations 0031 and on; the areas and
actions are CHECK constraints). A migration adds the area `sandboxes` and the action `stop`: actor = the
administrator, item = the conversation id, no title, no owner email (the row's owner is found by the id when needed).
The stop also logs at info with the conversation id and `reason = administrator`, and counts in
`tqa_sandbox_retired_total{reason="administrator"}` (the counter exists; the label value is new).

### 3.6 The leak verdict: the stray-container and stray-network sweeps
The sweeps run every five minutes in one task (`manager/networks.rs`, `NETWORK_SWEEP`): `sweep_containers` (GW-I-191:
this instance's containers older than the create budget that no record names, except a start in progress) returns
`ContainerSweep { removed, failed }`; `DockerBackend::reconcile_networks(false)` (SBX-U-006: this instance's networks
with no sandbox) returns `NetworkSweep { removed, failed, … }`. Today both results are logged and dropped. The verdict
keeps them:

- the manager stores the last pass's time and counts, and the previous pass's, in memory;
- **unowned** = this instance's containers and networks that no record accounts for, as the two sweeps define them;
  **volumes**: in `workspace.mode = volume` the named workspace volumes carrying this instance's label with no
  workspace record (a new listing beside the network one; in bind mode the count is 0 by construction and the line
  says so);
- **survived one sweep** = a thing counted `failed` in two consecutive passes, or found unowned again after a pass
  tried it; the engine's error code from the failed removal is the reason (`error.code()`, as the warn line carries it);
- a verdict is **green** when nothing unowned survived, **amber** when something did, with the reason and the age of
  the oldest survivor; it is never red, because the product keeps running: the reaper's rules protect the slots, the
  verdict protects the host's disk and address pool.

It is served in `/readyz?detail=1` beside `runs_active` and `git_credentials_kept`:

```jsonc
"environments": {
  "live": 3, "oldest_seconds": 25200,
  "unowned": { "containers": 0, "networks": 0, "volumes": 0 },
  "survived_sweep": { "containers": 0, "networks": 1, "volumes": 0, "oldest_seconds": 660, "reason": "engine_timeout" },
  "swept_at": "2026-11-17T01:25:00Z"
}
```

and as metrics on `/metrics`: `tqa_sandboxes{backend,state}` (the gauge ARCHITECTURE §13 already names and nothing
emits yet), `tqa_sandbox_oldest_seconds`, `tqa_sandbox_unowned{kind}` and `tqa_sandbox_unowned_survived{kind}`, so an
alert can be set without the page. The amber condition is the same rule in both places, in one function
(`verdict(previous, last) -> Verdict`), tested with a scripted engine (GW-U-219).

## 4. The page

### 4.1 Layout at 1280 px (the settings column, standard 40 px rows, UI-GUIDELINES §5c)

```
Sandboxes
Each task runs in its own environment. An idle one sleeps after 10 minutes; the task keeps its files and
starts again when its owner sends a message.

3 of 4 in use                                                     Updated 14:26
Working              1
Idle                 2     next sleeps at 14:32 (in 6 min)
Starting             0
Waiting for a slot   2     tasks, not yet running
Desktops             1 of 2
CPU                  1.4 cores in use · 6 set aside
Memory               3.1 GB in use · 12 GB set aside

Owner               State                      Environment   Size        Started   Last activity
ana@example.org     ● Working                  Workstation   2 CPU·4 GB  12 min    now
ben@example.org     ○ Idle · sleeps in 6 min   Minimal       1 CPU·2 GB  1 h       4 min            ⋯
You                 ○ Idle · kept on until 18:00  Desktop    2 CPU·4 GB  3 h       40 min           ⋯
cai@example.org     ◌ Starting…                Workstation   2 CPU·4 GB  20 s      —
```

- The summary is a block of label/value lines under a heading-face headline; at 4 of 4 the headline takes the warn
  tone and reads "4 of 4 in use · full", and "New tasks wait until one sleeps." appears only while something waits.
- Owner is the email, "You" for the administrator's own. State is a dot in the status tone (§7 Status) plus the word
  and a muted detail. Environment is the display name. Size is the sandbox's own size. Started and Last activity are
  relative, exact in a tooltip. The ⋯ is a RowMenu (§7 Action menu) with **Stop now**; on a working or starting row
  it stays, dimmed, with the reason under it ("It's working. It sleeps on its own after 10 idle minutes.").
- From 8 rows: a search box filters by owner and a segmented control by state (§7 List tools). Rows beyond 200:
  "Showing 200 of 812" and **Show more**.
- The page reads the list every 10 s while visible (paused while the tab is hidden; relative times tick each minute
  without a read); "Updated 14:26" says how fresh it is. No manual refresh button.

### 4.2 Stop now
ConfirmDialog (`ui/ConfirmDialog.tsx`, 480 px): title "Stop ben@example.org's environment?"; body "Their task keeps
its files and history. Anything still running in it, such as a server, stops. It starts again when they send a
message."; for a kept-on one an extra line, "They kept this task on until 18:00."; for a Desktop with a kept sign-in,
"Their kept sign-in ends with it."; buttons Cancel and **Stop environment** (filled danger, §7). After `202` the row
reads "Stopping…" until the next read, the summary updates, a `role=status` line says "Stopped ben@example.org's
environment.", focus returns to the next row's ⋯ or the table heading. Refusals read: `409 working` "It started working
again, so it wasn't stopped."; `409 work_waiting` "A message is about to start in it, so it wasn't stopped."; `404` "It
had already stopped." (and the list is read again); anything else "That didn't work. Try again."

### 4.3 States

| State | What shows |
|---|---|
| Loading | Skeleton (`ui/Skeleton.tsx`): the summary block and three rows; never a spinner |
| Failed read | "Couldn't read the environments." with **Try again**; the last good numbers stay, with their Updated time (`LoadFailure.tsx`'s pattern) |
| Empty | `ui/EmptyState.tsx`: "No environment is running. One starts when someone sends a message." No action: the one action an empty state offers would be invented here (a deliberate exception, as the 1 Oct contract noted) |
| Not measured yet | CPU and Memory read "Measuring…" for the first 10 s |
| Not measurable | "Not available on this server" (muted) when the backend returns no stats |
| Full | the warn headline; with waiters, the waiting line |
| A member opens it | redirected to Account; the section is absent from their menu |
| 390 px | each sandbox is a card (as Workspaces' cards): owner and ⋯ on the first line, state and detail on the second, environment and size muted on the third; the summary's labels never wrap in en; touch targets 44 px; no horizontal scroll at any width |
| Dark | tokens only (§3), checked in the e2e at 1280 and 390, light and dark, with axe |
| Ten languages | every string under `settings.sandboxes.*` in all ten catalogues (`web/src/i18n/locales`; `scripts/i18n.mjs --report` must list no missing key); numbers and units through `Intl.NumberFormat`; the state words and the summary labels are checked at 390 px in the longest language (th or ta) in the e2e |

### 4.4 Status gains one section
Between the checks and **Before a restart**: **Environments**: "3 live, oldest 7 h · 0 unowned containers, networks or
volumes." in the ready tone; amber: "3 live, oldest 7 h · 1 unowned network has survived a sweep (the engine timed
out). The gateway tries again every 5 minutes; if it stays, see the admin guide." with **Open Sandboxes**. The line
is read with the rest of `/readyz?detail=1` every 30 s, as **Before a restart** is (WEB-U-292).

## 5. What changes in the documents

- **PRD FR-22:** add "a Sandboxes page (every running environment across people: owner, environment, state, age,
  resources; Stop, audited; never a task's title or content) and a leak verdict on Status with the same numbers as
  metrics". **FR-13** gains nothing: `stats` is already in the backend contract.
- **ARCHITECTURE §4.4:** "The UI shows slots in use" becomes a sentence on the Sandboxes page and the administrator's
  stop as a fourth retire reason; **§13:** the four metrics named in §3.6, and `/readyz` detail's `environments`.
- **Admin guide (`web-settings.md`):** a **Sandboxes** section under Operations (what each column means, what Stop does
  and doesn't, the privacy line, the 200-row page), and in **Status** the Environments line with what amber means and
  what to do (read the engine's reason, remove by hand with the commands given, or restart the engine).
- **ADR:** an amendment to ADR-0016 (a fourth retire reason, by an administrator; the owner is told) rather than a new
  ADR; ADR-0021 gains the volume listing if the lead wants volumes in the verdict (open question 4).
- **TEST-PLAN:** the rows in §6. **CHANGELOG:** one Added line under the Unreleased section that follows v18.0.0.

## 6. Test rows (proposed ids; claimed when the work starts, after v18.0.0 is tagged)

Row format as TEST-PLAN §12 to §16. Ids continue each series as of main 4a2f1abbd (GW-U-215, GW-I-325, WEB-U-477,
WEB-E2E-126, SEC-063); the real-stack case takes a new E2E series, `E2E-OPS`, for Operations pages on the real stack.

| ID | P | Test | Steps and inputs | Expected |
|---|---|---|---|---|
| GW-U-216 | P1 | The administrator's sandbox listing (FR-22; ARCHITECTURE §4.4) | `admin/sandboxes_tests.rs` with a scripted store: four sandboxes of three people: `starting`; `live` with an active run; `live` idle since 4 min with `idle_seconds` 600; `live`, pinned, 3 h old with `max_lifetime_seconds` 8 h; one `destroying`; two queued messages without a slot; limits slots 4, per_user 2, desktops 2; a member's session; a page of 201 | States starting, working, idle (`stops_at` = last activity + 10 min), idle kept on (`stops_at` = started + 8 h), stopping; totals in_use 5, waiting 2, reserved = the sum of the sizes, usage null before a sample; `can_stop` true only on the two idle rows, `stop_refusal` names the reason on the others; the order working, starting, stopping, idle by soonest stop; the member gets `403`; 200 rows and a `next_after` that returns the 201st |
| GW-U-217 | P1 | An administrator's Stop goes through the retire path and refuses a working sandbox under the lock | `admin/sandboxes_tests.rs`: Stop on an idle sandbox; on a working one; on one with a queued message; on a `starting` one; on one already `destroying`; on a pinned idle one; on a Desktop whose screen a person holds (`screen::may_retire` false); on an unknown id; the same key twice; a run that starts between the check and the retire (the scripted store flips activeness under the lock) | `202` and `retire_for_switch(id, Teardown::Administrator)` called once; `409 working`, `409 work_waiting`, `409 working`, `202` (already stopping), `202` for the pinned one, `409 working` for the held Desktop, `404`; the repeated key answers the first result and retires nothing twice; the late start is refused, nothing retired |
| GW-U-218 | P1 | The stop is audited, counted and told to the owner | The stop of GW-U-217's idle sandbox; the audit table; the metrics text; the owner's conversation | One `admin_audit` row: area `sandboxes`, action `stop`, actor the administrator, item the conversation id, no other field; `tqa_sandbox_retired_total{reason="administrator"}` up by one; a notice in the owner's task naming the administrator's role and the time, and no notice in any other task; no title in the audit row, the log line or the notice |
| GW-U-219 | P1 | The leak verdict from a scripted engine (GW-I-191, SBX-U-006 as inputs) | `manager/verdict_tests.rs`: passes with (a) nothing unowned, (b) one stray container removed in the pass, (c) one network that fails to go in one pass then goes, (d) one that fails in two consecutive passes with `engine_timeout`, (e) a stray found again after a pass tried it, (f) bind mode, (g) volume mode with an unowned volume; three live sandboxes, the oldest 7 h | (a) and (b) green with `unowned` all 0 and `live` 3, `oldest_seconds` 25,200; (c) green (one pass is not survival); (d) and (e) amber with the kind, the count, the oldest survivor's age and the reason; (f) volumes 0 and the mode said; (g) amber on volumes; `/readyz?detail=1` carries the same object the metrics gauges show, and `tqa_sandboxes{state}` sums to `live` |
| GW-U-220 | P0 | The listing, the audit and the notice carry nothing of a task but its id (the privacy rule; SEC-005 extended) | The GW-U-216 fixture with strings planted in every task's title, last message, a file name, a workspace name, a branch, a route name and a profile name; the administrator's own task among them; the OpenAPI document | None of the planted strings appears in the listing's body, the audit row, the log capture or the notice; the response schema's objects all have `additionalProperties: false`; a field added to the item type without a schema change fails `tests/openapi.rs` |
| GW-I-326 | P1 | The listing on the real engine, with usage | Three sandboxes of two people on Docker (the testkit's fake model): one working in a 25 s call, two idle; the sampler's first and third passes | Rows as GW-U-216 says for the three; `usage` null on the first pass and a CPU rate and resident memory on the third, the working one's CPU above the idle ones'; `reserved` = the sum of the three sizes; on Firecracker the same rows with the memory label's `backend` = `firecracker` (SBX-C half, where the suite runs on it) |
| GW-I-327 | P1 | An administrator's stop on the real engine: the task resumes on its files | An idle sandbox whose run wrote a file; Stop from the API; the owner's next message asks the agent to read the file | The container is gone within 15 s; the conversation and workspace stay; the next message starts another container where the agent reads the file; the owner's transcript has the notice; the audit row and the counter as GW-U-218; a stop during the next run's start is `409 working` and that run is unharmed |
| GW-I-328 | P1 | The verdict on the real engine: a planted stray is swept, one that cannot be removed turns the verdict amber | A container and a network carrying this instance's labels and no record, older than the create budget; then a labelled container the engine refuses to remove (paused, so `remove` without force fails) | After one sweep: both planted things gone, verdict green, `removed` counted; the paused one: amber after the second pass with kind `containers`, count 1 and the engine's reason; unpaused, the next pass removes it and the verdict is green again; `/readyz?detail=1` and `/metrics` agree at each step |
| SEC-064 | P0 | The Sandboxes page is administrators only and shows sandboxes, not tasks | A member's session on `GET /api/v1/admin/sandboxes` and the stop; an administrator's listing of a member's running task; the page with the fixture of GW-U-220 | `403` for the member on both; the administrator's answer and page carry the owner's email and the environment, and none of the planted task strings; no element on the page links to another person's task |
| WEB-U-478 | P1 | Settings › Operations › Sandboxes: the page's states | `src/settings/SandboxesSettings.test.tsx`: loading; the GW-U-216 fixture; empty; a failed read after a good one; full with waiters; usage null then measured; 201 rows; a member | Skeleton; the summary's eight lines and the rows in order with the right words and tones; the empty state with no action; "Couldn't read the environments." with Try again and the last numbers kept; the warn headline and the waiting line; "Measuring…" then "1.4 cores in use"; "Showing 200 of 201" and Show more; the member is redirected to Account and the section is absent from their menu |
| WEB-U-479 | P1 | Stop now: the menu, the confirm and every result's words | The same fixture: ⋯ on an idle row, a working row, a kept-on row, a Desktop with a kept sign-in; confirm; the gateway answers `202`, `409 working`, `409 work_waiting`, `404`, a network failure | Stop now enabled on the idle row and dimmed with its reason on the working row; the dialog's title names the owner, the kept-on line and the kept sign-in line appear only where due; after `202` the row reads Stopping…, the status line names the owner, focus moves to the next ⋯; each refusal's sentence as §4.2; the list is read again after a `404` |
| WEB-U-480 | P0 | The page never renders a task's title, link or count (the privacy rule) | The GW-U-220 fixture with its planted strings, and a row of the administrator's own | No planted string in the rendered page; no anchor on any row; the administrator's own row reads "You" and nothing more than the others |
| WEB-U-481 | P1 | Settings › Status says whether anything unowned survived a sweep | `src/settings/StatusSettings.test.tsx`: `environments` green with 3 live and oldest 7 h; amber with one surviving network and reason `engine_timeout`; absent (an older gateway); a read that fails after a good one | The Environments line in the ready tone; the amber line with the kind, the reason, the five-minute retry and Open Sandboxes; the section left out when absent; the last read kept with "The latest read failed…" as Before a restart does |
| WEB-E2E-127 | P1 | The Sandboxes page and the Status line on the fake front, in the browser | `e2e/settings-sandboxes.spec.ts` against the fake front with the two endpoints: 1280 and 390, light and dark; stop an idle sandbox; the busy refusal; the empty state; the amber Status line; axe at each state; the summary at 390 in th | The screens as §4; the dialog and the status line read as §4.2; no horizontal scroll at 390; axe clean; the th labels do not wrap into a second line |
| E2E-OPS-001 | P0 | The Sandboxes page and the leak verdict on the real stack | `deploy/tests/demo.py --suite sandboxes` on the real Docker stack with the fake model: three tasks by two people (one working in a long call, two idle); the administrator opens Sandboxes, stops one idle sandbox from the page and confirms; the stopped task's owner sends the next message; then a planted unowned container (this instance's labels, no record, past the create budget), the Status page read before and after the sweep (the sweep interval shortened by the stack's settings) | The page lists the three with the right owners and states; after the stop the row goes, the task's files are intact and its next message runs in a new sandbox with the notice in its transcript; the audit row exists; Status reads amber with the container counted and the engine's reason while it survives, and green once it is swept; screenshots at 1280 and 390 in the evidence folder; nothing of any task's title on the page or in the API capture |

## 7. Open questions for the technical lead

1. **Operations page, not an Environments tab:** agreed as §1 argues, or do you want the tab?
2. **`stops_at`'s authority:** computed from `last_action_at + idle_seconds` (one worker pass of drift, no new
   plumbing), or the worker publishes its idle clock's deadline to a shared map the read uses? I'd take the first and
   word the page "in about 6 min".
3. **The owner's notice:** reuse the 18.0 notice mechanism (`screen_notices`'s pattern: a gateway-written transcript
   line carrying who made it, worded by the web) generalised to a sandbox notice, or a new lifecycle event kind? The
   first avoids a protocol change; it needs a small table or a `kind` column (one migration beside the audit one).
4. **Volumes in the verdict:** add the named-volume listing (volume mode only) now, or ship containers and networks
   first and say "volumes: not counted in bind mode" on the line? The soak's orphan audit already covers volumes
   nightly.
5. **Stopping a Desktop with a kept sign-in:** allowed with the extra warning (my proposal, under decision 1), or
   refused like a held screen?
6. **Ids and the `E2E-OPS` series:** the proposed numbers above, or do you want the stack case under `E2E-ENV`?
7. **Translations:** the nine non-English catalogues: do I write them (as the team did for the settings pages) and
   you review, or does the owner want a native check first?
8. **The audit migration:** extend `admin_audit`'s CHECK constraints with area `sandboxes` and action `stop` (0106
   after 18.0's 0105), or record the stop under an existing area? I'd add the area: the audit page filters by it.
9. **Sampler bounds:** 10 s, four calls in flight, the interval stretching past 64 live sandboxes. Fine for the
   5090 box; is there a Firecracker install where `stats` is slow enough to matter?
10. **Metrics names:** `tqa_sandboxes{backend,state}` as ARCHITECTURE §13 already promises, plus the three in §3.6.
