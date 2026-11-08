# ADR-0034: Agent teams (a lead and its members)

## Status

Accepted (2026-10-02; the open points were decided that day, under "Decisions (2 Oct 2026)"). Built now, test-first
and slice by slice (DEV-PLAN Phase 10), behind an install switch, `[agent_teams] enabled`, off by default; it merges
into the development branch behind that switch, and the internal releases then in testing keep first call on
testing time. Point 14a is in v1.0.
Revised on 2026-10-02 for the owner's decisions of that morning, which replace the 2026-09-28 design ("team runs on
channels": team profiles, `quorum` and `lead` votes, git bundles and `agent_spawn`). What changed, and why, is under "Changes from the
28 Sep version". The file name is kept so that links stay valid.

## Date

2026-09-28; revised 2026-10-02

## Context

The owner wants agents to work as a team the way our own technical lead runs five developers: one lead in charge,
each member on its own piece of work, members reporting to the lead, the lead deciding. That should be built into the
product, not left to prompts.

**Words.** An **agent team** is one lead and its members. (The product already has teams of people, for sharing
workspaces; these docs always say "agent team" in full.) A **swarm** is the step after that: agent teams talking to
agent teams. Swarms are a later phase, not designed here. Point 13 says how this design leaves room for them. In the
product, members are shown as **helpers**.

**What v1.0 built** (ADR-0014): channels. A channel is an ordered, stored message log with members, eight message
types, votes evaluated by the gateway, claims, a page and a stream. The 28 Sep version of this record put teams on top
of that forum, with every member reading the channel and the team deciding by vote.

**What we learned since:**
- **From our own team.** Five developers coordinated through a terminal multiplexer by one lead: the lead writes each
  member's brief, members report only to the lead, the lead decides and announces. What went wrong in practice
  became requirements (point 9):
  - a message typed into a busy window was half-sent;
  - a member waited on something that never came, and a waiter stalled another member for hours;
  - nobody noticed an idle member until the lead looked;
  - waits had no bound.
- **From published work** (the technical lead's research, 2 Oct 2026):
  - **The pattern that holds up is one lead with isolated workers.** One lead owns the context and starts workers
    with a structured brief. Workers return summaries and files. Group-chat designs, where peers talk freely, proved
    less predictable and lost ground.
  - **Teams are expensive.** A single agent uses about 4× the tokens of a chat, and a multi-agent system about 15×.
    At an equal token budget a single agent often matches a team. Teams win on parallel, read-heavy, separable work
    (research, review, competing hypotheses), and not on sequential work or work on the same files.
  - **Known failures:**
    - too many workers for a simple job;
    - duplicated work from vague briefs;
    - a lead doing the work itself instead of waiting, or stopping early;
    - task status lagging behind reality;
    - members overwriting each other's files.
  - **Isolated workers make conflicting implicit decisions.** A widely read essay argues against multi-agent
    designs for this reason, and its remedy is to share decisions, not only messages.
  - **Results should travel as files,** not only through the lead's summary, so nothing is lost.
  - **An agentic coding tool's agent teams** use a lead, teammates, a task list and a mailbox. Humans can message any
    teammate, teams are not nested, and 3–5 teammates are recommended.
  - **A2A (Agent2Agent)** is the open standard for agents across systems: agent cards, a task lifecycle and streams.
    It is a fit for swarms later (point 13).

## Decision

1. **A lead and its members, not a forum.**
   - Members talk only to the lead, and the lead sees all its members' messages.
   - Agents never vote. The lead asks a question, each member answers the lead, and the lead decides and announces
     the decision to every member.
   - People can always read everything. They can message the lead, step into any member's task, overrule the lead's
     decision by telling the lead, and stop a member or the whole agent team (point 10).
   - **People and the agent team (owner, 2 Oct 2026):**
     - every member's transcript streams live on its own page, like any task;
     - a person can type to the lead or to any member, and the lead is told of a message to a member (a
       `person_message` with the text);
     - when the agent team needs a person, the lead asks (`ask_person`), and the lead's task shows "Waiting for you"
       while a question is unanswered. `ask_person` returns at once, so the lead may keep working; the person's
       answer arrives in the lead's inbox as a message (and wakes it). A lead with nothing else to do waits with its
       normal wait;
     - members ask the lead, never the person directly. A member has no tool that reaches a person.

2. **Enforced by the gateway, not by prompts.**
   - **Roles are stored by the gateway.** When a member is started, the gateway stores each conversation's role
     (`lead` or `member`) and its agent team. No route an agent can call changes a role, and no API changes the role of
     a running agent team's conversation.
   - **Tools differ by role.** The gateway offers each run the tools of its role:
     - **member:** `send_to_lead` and `wait_for_lead`;
     - **lead:** `start_member`, `assign`, `ask_all`, `read_reports`, `decide`, `done`, and four this design adds:
       `take_files` (point 6), `stop_member` (point 9), `ask_person` (point 1) and `read_helper` (point 7);
     - **a task that isn't a lead yet:** `start_member` only. The lead's other nine appear from the model call after
       its first successful `start_member` in the same run, and from run start afterwards. This is the only change to
       a run's tools during the run. Tool definitions are part of the prompt's cached prefix, so it costs one prefix
       change, once per lead (TOOL-U-037). A task that never leads carries one small tool schema, not ten, and isn't
       nudged towards a team (proposal F).
   - **The per-run token says who calls,** as for channels: the gateway sets the author from the token, never from
     the body, and checks the role on every route.
   - **The gateway delivers a message only to its addressee** (point 5). A member can't address another member, and
     can't read what was sent to one.
   - **Depth is 1.** Members never get `start_member`, and a lead can't become a member of another agent team.

3. **No setup by a person.**
   - A person starts one normal task and asks it to lead ("split this into parts and get helpers"). There is no team
     profile and no team form.
   - **The lead starts members itself** with `start_member(role, brief, workspace, profile?)`:
     - `role` is the member's job in words ("researcher", "reviewer"). It is shown in the task list and is not a
       permission.
     - `brief` is what the lead writes: an objective, the expected output and the boundaries, all required, and
       optionally the sources or tools to use (the S1 contract; proposal A, decided). The gateway refuses a brief
       missing any of the three. The lead's instructions carry the sizing guidance: one helper for simple work, two to
       four typically, never more than the cap, and none when the work doesn't split (proposal F, decided);
     - `workspace` is `own_copy` (the default) or `my_files` (point 6);
     - `profile` optionally names an agent profile ("Legal reviewer") that the person may use: their own, or one an
       administrator shared (ADR-0012).
   - **A member is a normal task** owned by the same person. Its settings:
     - **with a profile:** the profile's instructions, model route, environment and skills. Its tools and connectors
       are the profile's, narrowed to what the lead's task may use: never wider (ADR-0035). The network allow-list is
       the lead's or narrower, and keys are the person's own (ADR-0037). The profile's own run limits apply, below the
       team budget;
     - **without a profile:** the lead's effective settings (environment, profile, network allow-list and keys), and
       the model route the administrator set for helpers, `agent_teams.member_route`, typically the local model; unset,
       the lead's own route (proposal E, decided). A strong model can lead while helpers run locally.

     Either way, a member is never wider than its lead.
     - **The team tools are the exception (decided 2 Oct 2026, slice 3).** They come from the run's role, which the
       runtime asks the gateway at run start (`GET /runtime/teams/role`: `member`, `lead`, `may_lead`, `ended` or
       `none`; `ended` is a lead whose team has ended and who hasn't read its end note, which gets `read_reports`
       alone to read it, so a team that ended between its runs is still told once; afterwards `none`: a lead leads
       one team, decided 2 Oct 2026), and
       they bypass the profile's tools allow-list, unlike the channel tools: a member must always be able to report
       to its lead and read what it sends, whatever its profile allows, and the gateway checks the role on every call
       anyway. A profile listing no tools still gets `send_to_lead` and `wait_for_lead` (RT-I-042).
   - **The member's first turn** is the gateway's fixed frame plus the brief, after the profile's instructions when
     there is a profile. The frame names the lead, the member's
     role and workspace mode, and the rule "you report to your lead with `send_to_lead`". The brief is marked
     untrusted, as channel content is (ADR-0014 point 10).
     - **As built in slice 2 (2 Oct 2026):** the turn goes on the member task as its person's message would, with
       the frame as its text and the brief beside it as `lead_brief` (protocol revision 12, ADR-0026's note): the
       runtime gives the model the brief after the frame inside the untrusted-content marker (source `lead`), and a
       client shows it as the brief from the lead, never as the person's words. A member's runtime below 12 is
       refused at the link. The turn has one idempotency key per member, so sending it again never sends twice: on
       the lead's files it goes when `start_member` commits and waits for the lead's sandbox, whatever the busy
       policy; on an own copy it goes when the copy is ready, and a copy that fails fails the member with a
       `member_failed` notice to the lead; at start-up every member still starting gets its turn if it is due. A
       member's first turn always goes as a turn of its own, never joined with a message waiting behind it.
     - **Review of slice 2 (2 Oct 2026):** a member whose first turn isn't stored (a failed send, a restart) gets it
       from a sweep every few seconds; one still without it ten minutes after it was started fails with a
       `member_failed` notice. An own copy taken while the lead's sandbox had its files open, as during the lead's run,
       is `consistent: false` in the answer, and the member's frame says a file the lead was changing may be partial.
       A member on the lead's files is answered `queued`: it waits for the lead's sandbox. The frame names no title.
   - **The install switch:** `[agent_teams] enabled` in `gateway.toml`, **off by default**. Off, no task gets a team
     tool and every team route answers `teams_disabled`. On, the limits below apply.
   - **Limits are admin settings,** runtime-mutable (ADR-0028), not per agent team:
     - `agent_teams.max_members`: members per lead at a time, **default 3, at most 8**. The effective cap is never
       above the admission slots or the per-user cap, minus one for the lead (`admission.slots - 1`,
       `admission.per_user - 1`). Writing a higher value is refused, and lowering admission lowers the effective cap;
     - `agent_teams.token_budget`: tokens for an agent team, lead and members together (point 8; default 2,000,000);
     - `agent_teams.deadline_minutes`: wall time from the first member's start, at most 24 h (default 120);
     - `agent_teams.member_route`: the model route for helpers started without a profile (above), unset by default;
     - `agent_teams.acceptance_check`: an optional check command for `done` (point 10), unset by default;
     - `agent_teams.idle_stop_minutes` (default 30), `member_idle_minutes` (default 10) and `member_stuck_minutes`
       (default 15) (point 9);
     - **Built in two steps** (decided 2 Oct 2026): until the settings page, these are `[agent_teams]` keys in
       `gateway.toml` with the defaults and bounds above (slice 2: `max_members`, `token_budget`, `deadline_minutes`,
       `member_route`; the others with their slices). The administrator's runtime row for each, and the refusal of a
       `max_members` written above the slots, come with the settings page in slice 6. The effective cap applies from
       slice 2: a tie names the admission limit, slots first (GW-U-128).
   - **Every member's run still goes through ordinary admission** and the per-user cap (ADR-0016). Members queue when
     slots are full, so an agent team takes no more of the host than its person could by hand. A queued member is
     shown as queued, with its position, to the lead and on the team panel.
   - **No slot deadlock.** v1.0 never evicts a busy sandbox, and a run sitting in a wait is busy. So agent-team waits
     are short and give way:
     - `wait_for_lead` and `read_reports` wait at most 30 s;
     - a wait answers at once, with `hint: yield_slot`, while any run on the installation is queued for a slot;
     - the tools tell the model, when nothing has arrived, to end its turn: it will be woken by its next message.

     A run whose turn has ended leaves its sandbox idle, and admission evicts the longest-idle sandbox when a run
     needs a slot. The woken run starts a new sandbox on the same workspace and resumes from its history. This is
     ADR-0016's parking by other means, with no resume inside a tool call. With the cap above, a lead and its members
     always fit, and helpers holding waits can't starve the run they wait for, whoever else is using the slots
     (GW-I-196).

4. **The agent team in the task list.**
   - Member tasks are normal tasks in the person's task list, grouped under the lead's task ("Report on Q3 ▸ 3
     helpers"). Each opens as a normal task page with a line naming its lead, and its transcript streams live
     there, like any task's. Typing there sends the member a message, as in any task, and the lead gets a
     `person_message`.
   - **"Waiting for you":** while a lead's `ask_person` question is unanswered, the lead's task shows "Waiting for
     you" in the task list and on its page, with the question and an answer box. The answer is stored as a
     `person_answer` message to the lead, read like any other, and it wakes a lead whose run isn't active.
   - The lead's task page gains a team panel:
     - the helpers with their state (working, waiting, idle, stuck, done, stopped);
     - the agent team's messages in order;
     - tokens spent against the budget, and the deadline;
     - Stop.
   - **The guides** describe the app as it ships: the helper texts waited in `docs/planned/` and moved into the guides
     in slice 6 (DEV-PLAN Phase 10), with their labels in the app's copy: the
     [user guide](../user-guide/helpers.md) and the [administrator's settings](../admin-guide/web-settings.md#helpers-v11).

5. **Messages are stored, addressed and delivered once.**
   - **Storage is the channel's** (point 12). An agent team owns one channel of kind `agent_team`, created with the
     first member. Its messages are channel messages with one seq per agent team, plus an addressee: the lead, one
     member, or all members.
   - **Message types:**
     - `assign` (lead to one member) and `question` (lead to all members, `ask_all`);
     - `answer`, `report` and `ask` (member to lead; a report can carry files, point 6; `ask` is a member's own
       question);
     - `decision` (lead to all members);
     - `person_question` (lead to the person, `ask_person`), `person_answer` (a person to the lead) and
       `person_message` (a copy, for the lead, of what a person typed to a member, or to the lead while its team works,
       point 9);
     - `notice` (the gateway to the lead: a member idle, stuck, failed, stopped or deleted, the budget at 80 %, the
       deadline near);
     - `done` (lead to all members).

     None of these is a vote. Wire shapes are in the slice S1 contract below.
   - **Delivery means the receiver's transcript holds the message: exactly once.** Each reader's last-read seq is
     stored. A read returns what was addressed to the reader after that seq, and a repeated read answers the same.
     The gateway writes the seq's advance and the read's `tool.result` event in **one transaction**. A crash before
     the commit leaves both unwritten, so the read comes again; a crash after it has both, so nothing can come twice
     (GW-I-195). Messages survive gateway restarts and sleeping sandboxes, and nothing retries
     in a loop.
   - **A send is one transaction:** stored with its seq and answered with it, or refused with a reason
     (`not_a_member`, `member_gone`, `team_stopped`, `body_too_large`, `team_budget`; the full list is in the
     contract). It is never half-sent. A retry with the
     same `Idempotency-Key` returns the same seq.
   - **Waking costs no model tokens.** A message for a conversation whose run isn't active queues a run on that
     conversation, through admission, with the fixed notice "new messages from your lead (or helpers) since seq N".
     Several messages that arrive while the wake waits make one wake.
     - **How (slice 3, 2 Oct 2026):** the wake is a turn sent like the member's first turn, waiting for the workspace
       whatever the busy policy, keyed by the reader's place in the log (`team-wake:<reader>:<last read seq, or the
       last seq of its outstanding answers>`). So several messages before it reads, and two posts at once, make one
       wake, and a woken run that ends without reading isn't woken again for the same messages: nothing retries in a
       loop (the liveness notices cover a helper that doesn't respond). After a failure the key also names it
       (`…:failed-<n>`: a helper's count of failures, `agent_team_members.failures`, which every stored run.failed and
       every give-up raises once; a lead's run.failed seq while its latest run failed), since a run that failed before
       reading leaves the place where it was: the next message wakes it once more (E2E-AGENTS-021, 7 Oct 2026; a key
       on the newest message instead was tried and woke a reader twice when a message came between its wake's
       delivery and its run's start). Only a running team wakes anyone, a member
       still starting gets its brief instead, and a reader with an active run or a turn already queued isn't woken.
       A post wakes its readers just after its commit; the reconciler's pass does the same from stored state, so a
       wake lost to a restart still comes.
     - **The wake is the gateway's, and says so (protocol revision 13, 5 Oct 2026; the page pass found a helper's page
       showing the wake as the person's own message).** The wake's `user.message` carries `team_wake`: the reader's
       place in the log, after which new team messages wait. It names no writer, since what waits may be several
       writers' and the gateway's, and the log shows who wrote. The runtime journals it
       on `message.user`, and a client shows the gateway's notice in its own words and language, never the person's
       message; the turn's text, which the model reads, is unchanged. A wake is always a turn of its own, never joined
       with a person's held message, so the marker never covers the person's words. A team's lead or member whose
       runtime announces less than 13 is refused at the link (`runtime_too_old`, `needs` 13), as a member below 12
       was, and a task linked below 13 is never offered the lead; every other task links from the floor, 10 (ADR-0026's note of 5 Oct 2026).
     - The gateway wakes the lead when a report or a notice arrives. So the lead's way to wait is to end its turn
       ("waiting for my helpers"), not to sit in a call.
     - `wait_for_lead` and `read_reports` are bounded long polls of at most 30 s, and give way to queued runs
       (point 3).
   - **Delete means gone** (PRD §12 item 18).
     - Deleting the lead's task stops its agent team and removes the team's messages and any files waiting to be
       taken.
     - It also deletes the helpers' tasks and their own copies of the workspace (owner, 2 Oct 2026); the confirmation
       names them. The lead's own workspace stays, with any files placed in it.
     - Deleting a member task removes that member from the agent team, with its messages and the gateway's stored
       copies of the files it handed back. The lead gets a `notice`.
     - Files already placed in a workspace stay, as PRD §12 item 18 says for any task.

6. **Workspaces: the lease stays; the lead chooses per member.**
   - The write lease (one live sandbox per workspace, FR-30, ARCHITECTURE §4.6) is unchanged.
   - **`own_copy` (default): the member works in parallel.** It gets a fork of the lead's workspace, made at
     `start_member` with the existing fork machinery (marked `consistent: false`, since the lead is running). The fork
     is an ordinary workspace owned by the person.
   - **Result files come back through the gateway.** `send_to_lead(text, files)` reads the named files from the
     member's workspace with the report (on Docker from its directory, on the microVM through the workspace service):
     streamed, path-checked, size- and count-capped as attachments are, and stored under the storage root per agent
     team. The report lists each file's path, size and SHA-256.
   - **The lead places them as attachments.** `take_files(report_seq, files?)` puts them into the lead's `uploads/`
     exactly as a person's attachment goes there (FR-36; ADR-0030 on block-image installs), so it needs no new
     workspace operation and no runtime revision (2 Oct 2026). Each is named `<member>-<path>`, its `/` turned into
     `-` and sanitised as an upload's name (`researcher-src-report.md`); a name already taken gets the next free one,
     so nothing in the workspace is ever overwritten. The answer maps each file to where it went, and the lead moves
     it where it belongs with its own shell. Taking a file again places another copy.
   - **A helper stopped before it reported is handed back by the gateway** (GW-I-311, 18 Oct 2026, a live run before 17.0:
     two helpers stopped at their budget share mid-run, never having seen the 80 % ask, and their files stayed in
     their own copies). When a helper stops, at its share of the budget or by its team's end, with no report since its
     last assignment, the gateway writes a report for it in the team's log, in the gateway's words ("The gateway hands
     back <name>'s work: <name> stopped before it reported (<why>). …"), never the helper's voice. It attaches the files
     the helper changed in its own copy since its last assignment (modified after the latest of its first run's start, a
     fork leaving no older time, its last assignment and its own last report: what it reported before, which the lead
     may have taken and changed, is never attached again), within the attachment limits, and names the rest with the
     limits. Repository and cache directories (`.git`, `node_modules`, `target`, `__pycache__`, `.venv`) are never
     walked, at most 1000 files are looked at (the report says when that cap was hit), and the report says files it
     deleted aren't listed. The lead takes them with `take_files` as any report's, after the team's end too (an ended
     team's lead may still take its reports' files), and the end note lists them. A `my_files` helper has nothing to
     attach: its writes are already in the lead's files. The model isn't needed for any of it; a hand-back doesn't count
     as the helper's report (it stays `stopped`, never `done`).
   - **`my_files`: the member works on the lead's own workspace, taking turns.** It binds to the lead's workspace,
     so it runs only while the lease is free: in practice, while the lead's turn has ended and the lead is waiting.
     Two `my_files` members take turns as well.
     - So that no wait can hang (point 9), a lead's `read_reports(wait)` answers at once, not after the wait, when a
       `my_files` member is queued for the lease. It names the member and says to end the turn.
     - This mode suits work that has to happen in place, one step after another.
   - **No git requirement.** The product is general purpose (PRD §12 item 11): files move through reports, whatever
     they are. The 28 Sep bundle and merge flow is dropped. A lead on a git workspace can still use ordinary git in its
     own sandbox on files it has taken.

7. **Decisions are the lead's, and people can overrule them.**
   - `decide(text, question_seq?)` stores a `decision` addressed to every member and shows it on the team panel;
     `question_seq` may name the lead's `question` or a member's `ask` (4 Oct 2026).
     Members see it with their next read, or are woken with it.
   - A person who disagrees tells the lead, in the lead's task ("No, use format Y"), and the lead reads it like any
     person's message and decides again. There is no separate veto control or message type: one way to say it (the
     designer review, 2 Oct 2026). The owner's "people can veto" is met this way.
   - **Team notes** (proposal B, decided): the lead's decisions so far are the team's notes. A new member gets all of
     them in its first turn, after the brief, and every member gets each new one as it is made. So helpers don't make
     conflicting choices on their own, and there is no second document to keep in step.
   - **Reading a helper's work in full** (proposal C, decided): `read_helper(member, since?)` returns that helper's
     transcript (its messages, tool calls and results, in order) to the lead, read-only and marked untrusted, for when
     a report isn't enough. Only the lead may read, only its own helpers, and the read changes nothing (GW-I-197).
   - **Reviewing reports** (proposal D, decided): the lead's instructions say to check each report before relying on
     it, and to answer with `assign` (more work) or `decide` (accepted). The administrator's optional acceptance check
     is point 10's.
   - Nothing is counted and no rule is evaluated: v1.1 adds no decision rule. The `quorum` and `lead` rules and
     agent votes are dropped.

8. **The budget is enforced by the gateway, at the relay** (kept from the 28 Sep version).
   - **What counts:** the agent team's tokens are prompt plus completion tokens across the lead's and the members'
     runs from the first `start_member` on, as the relay meters them. The runtime reports nothing the budget trusts.
     The relay adds each response's usage to `agent_teams.spent_tokens` in the transaction that records the usage.
     A response that ends before the server reports usage (a run stopped mid-reply: `stop_member`, a person's Stop,
     the deadline) adds the relay's own estimate instead, from what streamed and the request sent, marked `estimated`
     where usage is shown (12 Oct 2026, the teams eval's night 2: a helper's 13974 tokens of thinking had counted 0;
     GW-I-275).
   - **The refusal:** before forwarding, the relay checks `spent < budget` and the deadline, then refuses with
     `team_budget` (`run.failed` code `budget`). The status is one the runtime's model client never retries
     (`retryable: false`).
   - **Overshoot** is bounded by the requests in flight: at most the members' runs at once times the route's
     `max_output`, plus their prompts.
   - **Notices and the stop:** at 80 % the lead gets a `notice` once, asking it to collect the work, and each working
     member gets one asking it to report now with what it has (GW-I-304, 17 Oct 2026, the c16k campaign's B9b: a
     budget end handed nothing back). While a member is queued, starting or working, the lead's notice doesn't wake
     it: the members' reports do, and it reads the notice with them, so it doesn't finish before they come (18 Oct
     2026, found in review). Members already past their share aren't asked: they can't answer. At `agent_teams.helpers_stop_percent` (default 90, 1–99, `gateway.toml` only)
     the working members are stopped through the stop path (their runs cancelled, `stopped`, never `failed`; a failed
     member past the share isn't woken, since each run would fail again: a live run's `member_failed` every 15 s),
     each is handed back (point 6, GW-I-311), a call already on its way is refused `team_budget_helpers`, and the lead
     is told (`budget_helpers_stopped`); the rest is the lead's, to write up. At 100 %, or at the deadline, the agent team stops (point 10).
   - Each run's own limits still apply. The team budget is a ceiling above them.
   - **While the team stops, and after (decided 4 Oct 2026, E2E-AGENTS-020):**

     | The team | The lead's calls | A member's calls |
     |---|---|---|
     | running, within budget and deadline | forwarded | forwarded |
     | running, budget or deadline spent | `team_budget` | `team_budget` |
     | stopping for `budget` or `deadline` | `team_budget` | `team_budget` |
     | stopping for `done`, a person's stop, idle, a deleted lead or a disabled person | forwarded | `team_stopped` |
     | ended, for any reason | forwarded, an ordinary task | forwarded, an ordinary task |

     `done` exists for the lead's closing answer, and the stop finishes on the reconciler's next pass, so the lead
     calls the model while its team is still stopping. The lead's calls until the stop finishes (the reconciler's next
     pass) count toward the spend, even past a budget spent meanwhile. An ended team is charged nothing: its lead and
     members are ordinary tasks again (GW-I-108), metered as any task is.

9. **Reliability: what our own team taught us, as requirements.**
   - **Delivered or refused, never half-sent:** point 5.
   - **No waits that can hang:**
     - every wait is a bounded long poll of at most 30 s, which answers at once while a run is queued for a slot;
     - waiting for a person or a member happens by ending the turn and being woken;
     - the `my_files` lease case answers at once (point 6).
   - **The gateway notices an idle or stuck member and tells the lead** with a `notice`:
     - *idle:* the member's run ended with no report since its last assignment, and nothing is waiting for it, for
       `member_idle_minutes`;
     - *stuck:* its run is active, but no model call or tool call has finished for `member_stuck_minutes`;
     - *failed:* its run failed (code and reason).

     The lead may `assign` again or `stop_member(member, reason)`. A stopped member's task stays a normal task.
   - **The thresholds (decided 2 Oct 2026, slice 4):** `member_idle_minutes` (default 10), `member_stuck_minutes`
     (default 15) and `idle_stop_minutes` (default 30), each 1 through 1440, like `deadline_minutes`.
     - Each notice goes **once per episode**: again only after the member made progress or got a new assignment.
     - A stuck member is marked `stuck`, and `working` again as soon as it makes progress.
     - **Member states (decided 4 Oct 2026):** the panel's states are stored as they happen. Its first turn, once
       stored, makes a member `queued` while the turn waits (for a slot, the lead's files or its copy), else
       `working`. Its runs do the rest, in the transaction that stores each run's start or end: a run started or
       resumed makes it `working`; a run that ends makes it `done` when it reported since its last assignment, else
       `waiting` for its lead. A done helper keeps its seat (it counts toward `member_limit`) and is woken by a new
       message, since an assignment is how the lead gives it more work; `stop_member` stops it and frees the seat.
       `stuck` stays the reconciler's and `stopped` the stop's. `failed` is the first turn's when a helper couldn't start (it frees its seat and takes no message); a failed run, or a sandbox lost too often to replace, makes it `failed` too but with `failed_run` set (E2E-AGENTS-021, 7 Oct 2026, migration 0094): it keeps its seat, the lead's next message wakes it, `stop_member` stops it, and its next run makes it `working`.
     - **Progress** is the run's latest stored event: a stand-in for "a model call or a tool call finished", which
       the store can't tell apart. A long model stream therefore counts as progress.
     - `stop_member`'s `member_stopped` notice is the gateway's fixed sentence. The lead's reason stays in the lead's
       own tool call, as notices carry no agent text.
   - **A budget and a deadline:** point 8. The lead is told once, with a `deadline_near` notice, when min(10 minutes,
     20 % of `deadline_minutes`) remains: 60 minutes warns at 10 left, 20 at 4, 5 at 1 (the lead's decision of 2 Oct
     2026).
   - **An idle stop:** the agent team stops after `idle_stop_minutes` with no message and no active run.
   - **A lead's turn that ends without `done`** (GW-I-306, 17 Oct 2026, the c16k campaign's B2): while its team runs, a
     completed lead run posts a fixed `lead_unfinished` notice to the lead, which wakes on it: call done (the finish
     check runs then) or keep leading. Only when nothing else can wake the lead: no helper queued, starting or working
     (each will run and report, and the read tool tells a lead to end its turn and wait for that; an integration run of
     17 Oct 2026, where a reminder came first and the lead finished early), no question to its person still open (the
     answer wakes it), no turn on its way to it (queued, sending, or taken and not yet started), and nothing unread but
     its reminders. So it is also checked when a helper's run ends after the lead's last run did (not after a person's
     cancel). At most two since the lead's own last post. The lead task's header says the team waits on the lead, not
     Done (WEB-U-398).
   - **What the person types to the lead while its team works** (GW-I-307, 17 Oct 2026, the c16k campaign's B5): a
     lead's one run spans its team, so a message held "until the agent finishes" waited for the whole team. While the
     team runs and the lead's run is active, the message is held as any other (listed, cancellable) and a
     `person_message` naming no member is copied into the team's log, linked from the message (migration 0103). The
     lead reads it at its next check of the team, in fixed words ("The person wrote, while the team works: ...",
     TOOL-U-065), and the message is then delivered: it starts no run and can't be cancelled. Cancelled, dropped with
     the run, or sent as the lead's next turn first, the copy is never read, nor counted as unread for a wake or a
     reminder, nor shown on the page: once, never twice. The page's held line says the lead gets it at its next check
     of the team (WEB-U-402).

10. **Stopping.** An agent team stops when one of these happens:
    - the lead calls `done(summary)`. When the administrator set `agent_teams.acceptance_check` (a command, for
      example a test suite), `done` first runs it in the lead's workspace; if it fails, `done` is refused with
      `acceptance_failed` and the check's output, and the team keeps working (proposal D, decided; GW-I-198).
      - **Who runs it:** the lead's runtime, inside the `done` tool, through the run's shell tool: the workspace root
        as its directory, the shell's environment, its process group stopped with it, and Stop ends it. The gateway
        can't hold `done` open over a link call for a check that takes minutes, as a team call has 30 s. The tool asks
        `GET /runtime/teams/acceptance` for the command and sends `done` with its report:
        `check: {command, exit_code, timed_out, output}`.
      - **Bounds:** at most 10 minutes, a fixed limit rather than the shell's own; the output's last 4 KiB.
      - **The gateway decides:** inside `done`'s transaction, against the check set at that moment. Unset, `done` goes
        on and any report is ignored. Set, `done` goes on only for a report of that command that exited 0 within its
        time. A missing report (a runtime older than the gateway, which happens only mid-upgrade) or one for another
        command (changed while it ran) is refused in words that say what to do; a failed check is refused with
        `details: {exit_code, timed_out, output}`. A refusal stores nothing, and a retried `done` answers as its first
        did: its key holds the summary and the command, not the output, which a rerun after a restart can change.
      - **Trust:** the gateway believes the runtime's report, as it believes any link reply from that sandbox. The
        check is the administrator's quality gate on the team's work, not a security boundary: a lead that could
        forge its report could equally change what the check tests;
    - a person presses Stop on the lead's task (the whole agent team), or on a member's task (that member only, and
      the lead is told);
    - the budget or the deadline runs out;
    - the idle stop;
    - the lead's task is deleted, or its person is disabled.

    Stopping cancels every active member run through the existing cancel path, drops queued wakes, and ends the
    channel (read-only). It then posts the gateway's end note into the lead's task: the reason, tokens per member and
    in all, each member's reports and the files handed back. After the stop, every task is an ordinary task again,
    without team tools: a person can continue any of them. **A lead leads one team** (decided 2 Oct 2026): after its
    end, `start_member` answers `team_ended`.
    - **One stop path (slice 4):** whatever starts a stop only sets `stopping` with its reason. The reconciler carries
      a stopping team to `ended`:
      - it cancels the members' active runs, marks the members still at work `stopped`, and marks the team turns
        still queued for the team's conversations `cancelled`: their wakes, and a helper's first turn not yet started
        (one still waiting for a slot, the lead's lease or its copy), so no helper starts after the stop;
      - each task whose turns it dropped leaves the queue on its open pages, as a person's cancel withdraws it, and a
        helper whose first turn it dropped and that never ran has its sandbox retired (`team_stopped`), started or
        still starting, instead of holding a slot and the lead's lease for its idle time (amended 18 Oct 2026 after a
        live check, GW-I-312);
      - it stores the end note and sets `ended` in one transaction.

      Each step is idempotent, so a restart part way through finishes on the next pass with one note. A log over its
      day's messages still ends, without the note.
    - **The end note is a team message of type `ended` to the lead** (the lead's decision of 2 Oct 2026): no new
      transcript event, no protocol revision and no wake.
      - Its body has the reason, the summary for `done`, the tokens, the reports, the files handed back
        (`[{member, seq, name, size}]`) and `text`, the whole note in the gateway's fixed sentences.
      - It is stored and read once, like any message to the lead. The lead's model gets it from its next team tool
        call (the `team_ended` refusal row) or, when the team ended between its runs, from the role `ended`, which
        offers `read_reports` alone. The person sees it on the lead's page (slice 6).

11. **Orchestration is data plus a reconciler** (kept from the 28 Sep version).
    - An agent team is a row (`agent_teams`: lead conversation, channel, state `running → stopping → ended`, budget,
      deadline, spent, stop reason) plus `agent_team_members` (conversation, role, workspace mode, state, last-read
      seq). It has no process of its own.
    - One supervisor loop, like the sandbox manager's worker and the channel decision sweep, derives each agent
      team's next action from stored state: wakes, idle and stuck notices, the idle stop, the deadline and stopping.
      Every action is idempotent, so a gateway restart resumes where it was.
    - Every action is an ordinary one on an existing resource: create a conversation, fork a workspace, queue a run,
      store a message, cancel a run. These are audited the same way, with no agent-to-agent link, no broker and no
      new wire protocol.

12. **v1.0's channel stays as built** (Gate 7), voting included (owner, 2 Oct 2026).
    - Agent teams reuse the channel's storage (messages, seqs, the stream with `Last-Event-ID` replay), its untrusted
      marking and its page components.
    - They don't reuse its votes, claims or forum reading.
    - An agent team's channel is not listed among channels. It shows on the lead's task page.

13. **Room for swarms (later; designed in ADR-0038).** Addressing is by conversation, and the gateway delivers by
    addressee.
    So "a lead writes to another agent team's lead" is the same operation with one more kind of addressee.
    - **The owner's scope (2 Oct 2026):** one person's own teams only; each part of a goal is a new lead task from an
      agent profile the person picks (their own or one an administrator shared); no publishing and no sharing between
      people (ARCHITECTURE §16.4, DEV-PLAN Phase 12).
    - Depth stays 1 for `start_member`. A swarm links agent teams; it doesn't nest them.
    - Proposal I (A2A for other systems' agents) is outside that scope. It stays listed as a later option, not part
      of the swarm phase.

14. **Compatibility.** The team tools reach the gateway over HTTP routes (`/runtime/teams/...`), not session frames,
    and the gateway offers them per role at run start. So under ADR-0026's rule, an older runtime simply gets no
    team tools, and `start_member` refuses an environment whose runtime is too old, with a plain reason. Whether the
    role in the session configuration needs the revision raised is settled at build time, by that rule.
    - **(a) Kept, v1.0, landed (TOOL-U-036):** the runtime's channel client passes a message it can't parse through
      as marked raw JSON.

## Security

- **Amplification.** Depth 1, the admin's member cap, per-user admission and the shared budget. The relay enforces
  the budget, and every member run queues like any run.
- **No widening.** A member gets its lead's effective settings or narrower: tools, connectors, network list and
  keys (ADR-0035, ADR-0037 R6). A profile chooses its instructions, model, environment and skills, but its tools and
  connectors are narrowed to the lead's. It never gets lead tools.
- **Isolation between members.** Each member mounts exactly one workspace (§4.6). A member reads nothing of another
  member's: messages are delivered to their addressee only, and files reach the lead only, through its own
  `take_files`. The gateway stores handed-back files opaquely, with caps, and never runs them.
- **Injection between agents.** Briefs, answers and reports are marked untrusted. A misled member can only write to
  its lead, never to another member. The lead decides, and people can read everything, overrule the lead by telling it, and stop.
- **Budget integrity.** Usage comes only from the relay's metering. On stop, member runs are cancelled and their run
  tokens stop working with them.
- **Ownership.** Everything an agent team creates is its person's, under the usual retention and PRD §12 item 18.

## Proposals from the technical lead's brainstorm, as decided (2 Oct 2026)

| | Proposal | Decision |
|---|---|---|
| A | `start_member` takes a structured brief (objective, output, boundaries, model, workspace mode), and the gateway refuses an empty or vague one. The lead's instructions carry sizing guidance: 1 member for simple work, 2–4 typically, the admin's cap at most | **Taken.** Objective, output and boundaries are required, and the gateway refuses a brief missing one; the lead's instructions carry the sizing guidance (point 3) |
| B | Shared **team notes**: decisions and conventions the lead writes, given to every member at start and on change | **Taken.** The lead's decisions are the team notes: all of them in a new member's first turn, each new one to every member (point 7) |
| C | Reports carry evidence (files plus a short summary), and the lead can open a member's full transcript on demand | **Taken.** Reports carry files (point 6); `read_helper` gives the lead a helper's transcript on demand (point 7) |
| D | An acceptance step: the lead checks each report before marking work done, with an optional admin acceptance rule (for example "tests must pass") | **Taken.** The lead reviews each report; the administrator's optional `agent_teams.acceptance_check` runs before `done` (points 7 and 10) |
| E | Model mix: a strong lead, with members on the local model by default; cost shown live on the team panel | **Taken.** `agent_teams.member_route` puts helpers on, for example, the local model; the team strip shows the budget used live (point 3) |
| F | Default: no agent team. Members are started only when the work splits; the panel shows tokens so far against a single task; "Make it a team" is suggested only for long or parallel work | **Taken,** as the lead's guidance: no team unless the work splits (point 3). The in-product "Make it a team" suggestion is left out, because UI-GUIDELINES §13b adds no control for starting a team; the agent suggests it in words when it fits |
| G | People may message any member directly, and the lead is told | **Decided by the owner on 2 Oct 2026:** a person can type to any member directly, like typing into any developer's terminal window, and the lead is told (point 1) |
| H | Liveness: the gateway tells the lead when a member is idle, stuck or failed | **Taken;** point 9 requires it |
| I | A2A for leads, for swarms and outside agents | **Out of scope** (owner): no outside agents (ADR-0038 point 1) |
| J | Evaluation before v1.1 ships: about 20 representative team tasks (research, report, code) with judged results, single agent against agent team at the same token budget | **Taken.** The evaluation runs before v1.1 ships (DEV-PLAN Phase 10, slice 7) |

## Decisions (2 Oct 2026)

The owner left the open points to the technical lead, who decided them on 2 Oct 2026:
1. **Build now,** test-first and slice by slice, behind `[agent_teams] enabled` (off by default); it merges into the
   development branch behind the switch, and the internal releases then in testing keep first call on testing time.
2. **Members per lead:** 3 by default, at most 8, and never more than the slots minus one (point 3).
3. **Deleting the lead's task** removes the team's messages, the helpers' tasks and their copies; the lead's workspace
   stays (point 5). A goal deletes its teams the same way (ADR-0038).
4. **v1.0's channel** stays as built, voting included (point 12).
5. **Proposals:** A, B, C, D, E, F, H and J taken; G decided earlier; I out of scope (the table above).

## Alternatives considered

- **The 28 Sep design: a team on a forum.** Every member reads the channel, and the team decides by majority or
  quorum. Dropped: it is less predictable (the group-chat finding), agents voting adds nothing the lead can't decide,
  and people found it harder to follow than one lead.
- **No multi-agent at all.** The published critique holds for sequential work. The design keeps a single task as the
  default and lets a lead split work only when it splits (proposal F), with a budget.
- **Teams set up by a person** (team profiles). Dropped by the owner: the lead writes each brief, as our technical
  lead does.
- **Git as the hand-back** (bundles and merges). Dropped: the product is general purpose, so files travel through
  reports. A lead on a git workspace can still use git on what it takes.
- **Members on one shared workspace at once.** That would break the lease and invite overwrites (a known failure).
  `my_files` takes turns instead.
- **A protocol of our own between agent teams.** Not now. Swarms later, possibly over A2A.

## Changes from the 28 Sep version

- **Dropped:**
  - team profiles and `POST /api/v1/team-runs`;
  - the `quorum` and `lead` decision rules and agent votes;
  - `fork_per_member` and `shared` as team policies;
  - git bundles, `team_fetch` and the bundle store;
  - `agent_spawn` (any member adding members): now only the lead starts helpers, and helpers can't start helpers
    (one level);
  - decision D1 (merge by majority vote).
- **Kept:**
  - orchestration as data plus a reconciler;
  - budget enforcement at the relay;
  - stopping and its end note;
  - waking instead of waiting;
  - the tolerant channel decode (TOOL-U-036, v1.0);
  - the security principles (no widening, isolation, budget integrity).
- **New:**
  - roles stored by the gateway, and tools by role;
  - addressed delivery with per-reader seqs;
  - `start_member`, written by the lead;
  - per-member workspace mode;
  - file hand-back through reports;
  - idle and stuck notices;
  - admin limits.
- **Superseded:** ADR-0035's amendment about `agent_spawn` and ADR-0037's mention of spawned runs. Their rule (never
  wider than the caller) now applies to members and their lead.

## Test rows reserved (v1.1)

TOOL-U-036 is v1.0 work (point 14a, landed); the rest are v1.1. IDs from the 28 Sep version are reused for the
revised subjects, since none was built. E2E-SWARM-009 and E2E-SWARM-017 move to the swarm phase.

| ID | Pri | What |
|---|---|---|
| TOOL-U-036 | P1 | (v1.0, landed) A channel message this runtime doesn't know still reads, as one block marked `type="unknown"` |
| TOOL-U-037 | P1 | The team tools' specs: strict-valid schemas, offered by role (member: two; lead: ten; a task that isn't a lead yet: `start_member` alone, one prefix change when it becomes one), a member never gets a lead tool, none when agent teams are off or the runtime is too old |
| PROTO-U-023 | P1 | The agent-team message types (`assign`, `question`, `answer`, `report` with files, `ask`, `decision`, `person_question`, `person_answer`, `person_message`, `notice`, `done`) round-trip, with addressees; a person's messages render as a person's; an older runtime reads them per TOOL-U-036 |
| GW-U-069 | P1 | Roles and routes as a pure table: which role may call which route and address whom; a stored role can't change; depth 1 |
| GW-U-125 | P1 | The brief and message bodies validated naming the field and the reason (S1 contract) |
| GW-U-126 | P1 | The read cursor as a pure function: recorded per key, advanced only with the stored result (S1 contract) |
| GW-I-195 | P1 | Exactly-once delivery at the crash point, through the batched persistence path: the `tool.result` event and the seq's advance commit together (S1 contract) |
| GW-I-196 | P1 | No slot deadlock: the member cap below the slots, waits that yield to queued runs, idle helpers evicted and woken (S1 contract) |
| GW-I-197 | P1 | `read_helper`: the lead reads its own helper's transcript, read-only and marked untrusted; a member, another lead and a helper of another team are refused (proposal C) |
| GW-I-198 | P1 | The acceptance check: with `agent_teams.acceptance_check` set, the lead's `done` runs it in the lead's workspace and is refused `acceptance_failed` with its exit and output when it fails; unset, `done` stops at once (proposal D, point 10) |
| GW-I-106 | P1 | `start_member`: the agent team created with the first member; the member a normal task with a profile's settings narrowed to the lead's, or the lead's own, the brief marked untrusted, the fixed frame; `own_copy` forks and `my_files` binds; the member cap; a member's `start_member` refused; a restart mid-start resumes with no step done twice |
| GW-I-107 | P1 | The agent team's budget at the relay: lead and members summed, the 80 % notice once, refusal with `team_budget`, overshoot within the bound, the deadline; the refusal is final |
| GW-I-108 | P1 | Stopping: `done`, Stop on the lead and on a member, budget, deadline, idle stop, the lead's task deleted, its person disabled; runs cancelled, wakes dropped, the end note; every task usable afterwards without team tools. Deleting the lead's task deletes the team's messages, the helpers' tasks and their own copies, and keeps the lead's workspace |
| GW-I-109 | P1 | Delivery: addressee only; per-reader last-read seq; nothing lost or doubled across a gateway restart and a sleeping sandbox; a send stored or refused with its reason; an idempotent retry; bounded waits; waking (one wake for several messages) |
| GW-I-110 | P1 | File hand-back: a report's files read from the member's workspace with caps and path checks; `take_files` by the lead only, into its `uploads/` as attachments, on Docker and on the microVM; never an overwrite; the stored copies go with a deleted member task |
| GW-I-111 | P1 | Liveness: idle, stuck and failed notices to the lead at their thresholds; `stop_member`; a `my_files` member queued on the lease makes the lead's wait answer at once |
| SEC-037 | P0 | No widening or amplification: a member never gets more than its lead's settings, never lead tools; depth 1; the member cap; per-user admission |
| SEC-038 | P0 | Isolation: a member can't read or address another member, read another member's files, or change a role; the author comes from the run token |
| SEC-039 | P1 | The budget can't be bypassed: relay requests after stop or past the budget are refused whatever the runtime reports |
| WEB-E2E-024 | P2 | The lead's task page and the task list: helpers grouped under the lead, the team panel (states, messages, budget used, time left), a person's message to a helper, Stop on a member and on the lead |
| E2E-AGENTS-020 | P2 | An agent team end to end on the fake model: a lead starts two helpers (`own_copy` and `my_files`), asks both, decides, takes one helper's files, and calls `done`; the end note lists both |

## Slice S1 contract (written before code, DEV-PLAN §1)

What the first slices build against: the brief, the stored messages, the twelve tools' wire shapes and every refusal,
each with its TEST-PLAN row. Later slices add behaviour (wakes, liveness, files, budget, the web) without changing
these shapes; a change here is a change to this record first.

### Conventions
- **Routes** are under `/runtime/teams/`. They accept only the runtime token of a run, bound to its sandbox's address,
  as `/runtime/channels/` does. The gateway takes the caller's conversation and role from the token, never from the
  body.
- **Writes** carry an `Idempotency-Key`, the calling tool call's own key, as channel posts do. A repeat with the same
  key and body answers as the first did; the same key with another body is `idempotency_key_reused`.
- **Errors** are the gateway's usual `{"error": {"code", "message", "retryable", "details"?}}` (ENGINEERING-GUARDRAILS
  §8). A tool shows the model `<code>: <message>`, as the channel tools do (TOOL-U-019).
- **Exactness:** the gateway decodes request bodies strictly (unknown fields refused). The runtime reads answers
  tolerantly (TOOL-U-036): an unknown message type or field becomes a marked unknown block.
- **Sizes:** text fields count UTF-8 bytes; NUL is refused everywhere; a request body is at most 64 KiB.

### The brief (`start_member`)
```json
{"role": "researcher",
 "brief": {"objective": "Find the Q3 revenue by region in the attached reports.",
           "output": "A table: region, revenue, source file and page.",
           "sources": "Only the files in reports/; no web.",
           "boundaries": "Don't edit the reports; write only results.md."},
 "workspace": "own_copy",
 "profile": "legal-reviewer"}
```
| Field | Rule | Refusal (`invalid_request`, `details[].reason`) |
|---|---|---|
| `role` | 1–60 bytes, one line, letters, digits, spaces, `-` and `_` | `value_empty`, `value_too_long`, `name_invalid` |
| `brief.objective`, `brief.output`, `brief.boundaries` | required, 1–4,000 bytes each (proposal A) | `value_empty`, `value_too_long` |
| `brief.sources` | optional, at most 4,000 bytes | `value_too_long` |
| `brief` as a whole | at most 12 KiB | `value_too_long` (field `brief`) |
| `workspace` | `own_copy` (default) or `my_files` | `parameter_invalid` |
| `profile` | optional; a profile ID (its name) the person may use: their own, or shared by an administrator, the person's own first when both have the name | `unknown_reference` when unknown or not the person's to use; `profile_unusable` (409) when it names a model, environment, connector or skill that can't be used now |
| any text | no NUL | `value_nul` |

**What the helper gets from a profile.** Its instructions, model route, environment and skills come from the
profile. Its tools and connectors are the profile's intersected with what the lead's task may use, so a profile can
narrow but never widen (ADR-0035); a profile tool or connector the lead may not use is left out, and the answer lists
what was left out. Its network allow-list is the lead's or narrower, and its keys are the person's own. The model sees
the profile's instructions first, then the gateway's fixed frame and the lead's brief. With no `profile`, the helper
takes the lead's own effective settings.

The member's name is `role`, made unique within the agent team by `-2`, `-3` and so on. The brief is stored as the
agent team's next message, an `assign` to the new member, so the brief is in the log like everything else.

### Stored messages
A message is a row of the agent team's channel (kind `agent_team`): `seq` (one sequence per agent team), `type`,
`author` (`lead`, `member` with its conversation, `person` with the user, or `gateway`), `to` and `body`. `to` is
`lead`, `member` (with its conversation), `members` (every member) or `team` (the lead and every member).

Read shape (what `read_reports` and `wait_for_lead` answer; PROTO-U-023):
```json
{"seq": 7, "type": "answer", "from": {"kind": "member", "name": "researcher"}, "to": {"kind": "lead"},
 "at": "2026-10-02T09:14:03Z", "text": "North: 4.1 M …", "question_seq": 5}
```
| `type` | From → to | Body beyond `text` | Text limit |
|---|---|---|---|
| `assign` | lead → `member` | (the brief's four parts, for the first one) | 12 KiB |
| `question` | lead → `members` | — | 8 KiB |
| `answer` | member → lead | `question_seq` (a `question` in this agent team) | 16 KiB |
| `report` | member → lead | `files: [{name, size, sha256}]` (filled in slice 5; empty until then) | 16 KiB |
| `ask` | member → lead | — | 8 KiB |
| `decision` | lead → `members` | `question_seq?` (a `question` or an `ask` in this agent team) | 8 KiB |
| `person_question` | lead → (the person) | — | 4 KiB |
| `person_answer` | person → lead | `question_seq` (a `person_question`), `by` | 16 KiB |
| `person_message` | person → lead (a copy of what the person typed to a member; `member` empty for what was typed to the lead while its team works, GW-I-307) | `member`, `by` | 16 KiB |
| `notice` | gateway → lead | `kind`: `member_idle`, `member_stuck`, `member_failed`, `member_stopped`, `member_deleted`, `member_queued`, `budget_80`, `deadline_near`; `member?`; `position?` (for `member_queued`); `text` | 4 KiB |
| `done` | lead → `members` | — | 8 KiB |

Who reads what: a member reads what is addressed to it, to `members` and to `team`; the lead reads what is addressed
to `lead` and to `team`; people read everything through the human API (slice 6).

How a message is shown to a model depends on its author:
- **Messages from agents** (the lead and the members) go inside the untrusted-content marker, with their seq, type and author
  (ADR-0014 point 10).
- **A person's messages** (`person_answer`, `person_message`) go in a block marked as from a signed-in person,
  with the person's display name. They are not inside the agent marker, so the lead doesn't discount them as an
  agent's text. Like any person's message in a task, they are user input, never system instructions.
- **The gateway's `notice`** is a fixed sentence from the gateway, with no free text from agents.

### Headers
Every `/runtime/teams/` route a tool calls takes two headers besides the runtime token (amended 2 Oct 2026, within
revision 12, which no release has carried yet):
- `X-TQA-Run-ID`, the calling run. It must parse as a run id (else `400 invalid_request`, naming `X-TQA-Run-ID`) and
  be the conversation's current run (else `409 not_current_run`). A call without it is refused the same way, naming
  the header. This lets any team tool's answer be recorded for its run like a read's, as the end note is (point 10).
- `Idempotency-Key`, on every route that changes something and on reads (the call's key, 1–128 visible ASCII).
- `GET /runtime/teams/role` takes neither: the runtime asks it while it builds the run's tools, before the run has
  an id, and it records nothing.

### Delivery
- Each reader has `last_read_seq`. `GET /runtime/teams/messages?wait=S&limit=N` answers the messages for this reader
  after its `last_read_seq` **and after every answer of its still outstanding** (recorded, result not yet stored), in
  seq order, at most `N` (1–200, default 50). The reader's row is locked for the read, so two reads take turns, and
  no two reads answer the same message (amended 2 Oct 2026 after review: reads in one step could otherwise both
  answer the same seqs).
- With `wait` (0–30 s, default 0) it holds until at least one message exists or the wait ends. It answers at once,
  with `hint: {"kind": "yield_slot"}`, while any run on the installation is queued for a slot. A lead's wait also
  answers at once with `hint: {"kind": "yield_files", "member"}` while a `my_files` member waits for the lease.
- The answer is recorded against the read's idempotency key (its tool call), for the run its `X-TQA-Run-ID` header
  names (see Headers). The key is the gateway's usual one (1–128 visible ASCII). The run header must
  parse as a run id and be the conversation's current run, so every recorded read belongs to a run whose end prunes
  it. A repeat with that key answers
  the same messages and the same `more`, even if newer ones have arrived meanwhile.
- **The reading tools are not parallel-safe** (amended 2 Oct 2026): a run makes its reads one after another, so their
  results reach the gateway in order and each advance is a step forward with nothing skipped.
- When the gateway stores that call's `tool.result` event from the runtime, it sets `last_read_seq` to the answer's
  last seq **in the same transaction**. Events are persisted in small batches (PERF-002, GW-U-116/117), so the advance
  rides in the transaction of the batch that holds that `tool.result`. So a message is delivered exactly once:
  - a crash before the commit writes neither, and the read is answered again;
  - a crash after it has both, so no later read returns those messages.

  The runtime journals the result before sending it, so a resumed run re-sends it, and it is stored then.
- **No cost when it isn't a team read.** The gateway keeps the outstanding reads in memory, loaded at start from the
  recorded ones, so storing any other event does no extra work, and none at all while agent teams are off (PERF-002
  unchanged).
  - **Ordering:** a read enters that index before its transaction commits, so before its answer leaves the gateway.
    Its `tool.result` can only follow the answer, so it always finds the read and advances the seq (GW-I-195). Were a
    result ever to miss the index, the read would be answered again: delivery at least once, never a loss.
  - **Reads left from an ended run** (one that ended while agent teams were off, say) go at the next start with
    agent teams on: the load first deletes every recorded read whose run isn't its reader's current run (GW-I-109).
- **An empty answer reports where it started**, never a seq behind an earlier answer's.
- **Recorded answers are pruned.** In the same transaction as the advance, the gateway deletes the reader's recorded
  answers whose last seq is at or below the new `last_read_seq`. When a run ends, it deletes that run's remaining
  ones: a read whose result was never stored has advanced nothing, so its messages come with the next read anyway.
  The table holds at most the reads of active runs.
- A send (`POST /runtime/teams/messages`) is one transaction: validated, stored with the next seq, and answered
  `201 {"seq": n}`; or refused, with nothing stored.

### Routes and the twelve tools
| Tool | Role | Route | Request | Answer (and the text the model sees) |
|---|---|---|---|---|
| `start_member` | lead | `POST /runtime/teams/members` | the brief above | `201 {"member": {"name", "conversation", "workspace", "workspace_id", "profile"?, "left_out"?: [tool or connector], "consistent"?: bool, "state": "starting"` or `"queued", "position"?}, "seq", "lead_tools": true}`. "Started helper researcher (own copy); its brief is message 3. It reports to you." On the first one the runtime adds the other nine lead tools from the next model call |
| `assign` | lead | `POST /runtime/teams/messages` | `{"type": "assign", "to": "<member name>", "text"}` | `201 {"seq"}`. "Sent to researcher as message 7." |
| `ask_all` | lead | same | `{"type": "question", "text"}` | `201 {"seq", "to": ["researcher", …]}`. "Asked 3 helpers as message 8." |
| `decide` | lead | same | `{"type": "decision", "text", "question_seq"?}` | `201 {"seq"}`. "Decision sent to 3 helpers as message 12." |
| `ask_person` | lead | same | `{"type": "person_question", "text"}` | `201 {"seq"}`, at once. "Asked; your task shows Waiting for you. The answer arrives as a message." |
| `read_reports` | lead | `GET /runtime/teams/messages` | `wait` (0–30 s), `limit` | `200 {"messages": […], "last_seq", "more", "hint"?}`. `hint` is `{"kind": "yield_slot"}` or `{"kind": "yield_files", "member"}` (above), and the tool then tells the model to end its turn. Each message is one block, marked by its author |
| `take_files` | lead | `POST /runtime/teams/files` | `{"report_seq", "files"?: [path]}`: paths as the report lists them, at most 32; all of the report's files when absent. `Idempotency-Key` required | `200 {"placed": [{"name": path in the report, "path": "uploads/<member>-<path>", "size"}]}`. "Placed 2 files from report 9 in uploads/: src/report.md as uploads/researcher-src-report.md (2,104 bytes), … Move them where they belong with your shell." (built in slice 5) |
| `stop_member` | lead | `POST /runtime/teams/members/{name}/stop` | `{"reason"}` (≤ 1 KiB) | `200 {"member": {"name", "state": "stopped"}}`; stopping a stopped member answers the same |
| `read_helper` | lead | `GET /runtime/teams/members/{name}/transcript` | `since` (a seq of that helper's events), `limit` (1–200) | `200 {"now", "events": […], "last_seq", "more"}`: `now` is where the helper is (its state and, mid-step, the step, the running tool, since when and for how many seconds, and about how many tokens of thinking and text the step has streamed; the tool shows it first, so a lead can tell slow from stuck: c16, GW-I-277, TOOL-U-058); then the helper's messages, tool calls and results in order, each one block marked untrusted; the gateway's wake is `{"type": "wake", "from", "after"}`, never a message (revision 13). An event's text, content or arguments past 8 KiB are cut and marked `truncated` with their full `bytes`, and a page holds at most 64 KiB (slice 4). Read-only; built in slice 4 (GW-I-197) |
| `done` | lead | `POST /runtime/teams/done` | `{"summary", "check"?}` (≤ 8 KiB); `check` is the acceptance check's report, sent when one is set (point 10), after `GET /runtime/teams/acceptance` (`200 {"check": command or null}`, the lead only) | `200 {"team": {"state": "stopping"}}`; a repeat answers the same. A refused check comes back as the tool's result: "acceptance_failed: the acceptance check failed (exit 1), so the team goes on. Fix what it found, then call done again. Its last output: …" |
| `send_to_lead` | member | `POST /runtime/teams/messages` | `{"type": "answer", "question_seq", "text"}`, `{"type": "report", "text", "files"?: [path]}` (at most 32 paths, each relative to the workspace, without `..`, at most 1,024 bytes, each a regular file within `files.upload_bytes`, together within `files.uploads_bytes`) or `{"type": "ask", "text"}` | `201 {"seq"}`. "Sent to your lead as message 9." |
| `wait_for_lead` | member | `GET /runtime/teams/messages` | `wait` (0–30 s, default 30), `limit` | as `read_reports`; `hint` is only ever `{"kind": "yield_slot"}`. With nothing new, the text tells the model to end its turn: "Nothing yet; end your turn, and you'll be woken by your lead's next message." |

The reading tools are in the read class but not parallel-safe (above); the others are not parallel-safe, in class
`communication`.
Every tool's schema is strict (TOOL-U-037). The gateway offers a member its two tools. A top-level task gets
`start_member` alone while agent teams are enabled. A conversation is a lead from its first successful `start_member`:
the other nine lead tools are added from the next model call of that run, and from run start afterwards (point 2).

### Refusals
| Code | Status | When | Retryable | Test |
|---|---|---|---|---|
| `invalid_request` | 400 | a field breaks its rule, with `details: [{field, reason}]`, a missing `Idempotency-Key` or `X-TQA-Run-ID` included, naming it; a report's `files` past 32 (`value_too_long`) or a path that is absolute or leaves the workspace (`parameter_invalid`) | no | GW-U-125 |
| `body_too_large` | 413 | a request body over 64 KiB | no | GW-U-125 |
| `teams_disabled` | 403 | agent teams are off for the installation (`[agent_teams] enabled = false`, the default) | no | TOOL-U-037, GW-I-106 |
| `not_a_member` | 403 | the caller's conversation is in no agent team (any route but `start_member`) | no | GW-U-069, SEC-038 |
| `not_lead` | 403 | a member calls a lead route (`start_member` included: depth 1) | no | GW-U-069, SEC-037 |
| `members_only` | 403 | the lead sends `answer`, `report` or `ask` | no | GW-U-069 |
| `unknown_member` | 404 | `to` or `{name}` names no member of this agent team | no | GW-I-109 |
| `unknown_reference` | 422 | `question_seq`, `decision_seq` or `report_seq` names no message of the right type in this agent team (an answer's `question_seq` a `question`; a decision's a `question` or an `ask`); `profile` names no profile the person may use | no | GW-U-125, GW-I-106, GW-I-109 |
| `profile_unusable` | 409 | the `profile` names something that can't be used now (an existing code, ADR-0012) | no | GW-I-106 |
| `member_gone` | 409 | the addressee is stopped, couldn't start or deleted; `stop_member` on a helper that couldn't start (a done one, or one whose run failed, is stopped) | no | GW-I-109, GW-I-111 |
| `team_stopped` | 409 | the agent team is stopping; at the relay, a member's call (the lead's passes, point 8) | no | GW-I-107, GW-I-108, GW-I-109 |
| `team_ended` | 409 | the agent team has ended, on every tool route, `start_member` included (a lead leads one team). For the lead, while it has unread messages, the end note among them, `details` is a read's answer (`messages`, `last_seq`, `more`), recorded under the call's key for its run exactly as a read is: a repeat answers the same, and `last_read_seq` advances when that call's `tool.result` is stored. The tool shows them as `read_reports` does, each a marked block. A member's, and the lead's once nothing is unread, carry no `details`. `GET /runtime/teams/role` answers `ended` while the lead's end note is unread (`read_reports` alone), then `none` | no | GW-I-108, TOOL-U-037 |
| `member_limit` | 409 | the lead already has as many helpers as the effective cap allows, a done one counting until it is stopped (the message says so, and that `stop_member` frees its seat): `max_members` (default 3, at most 8), the slots or the per-user cap, each minus one (`details: {limit, by}`, `by` being `max_members`, `slots` or `per_user`) | no | GW-I-106, GW-I-196, SEC-037 |
| `team_budget` | 409 | the agent team's budget or deadline is spent (the same code as at the relay) | no | GW-I-107, SEC-039 |
| `runtime_too_old` | 426 | the lead's environment can't run the team tools (`needs`, `has`) | no | TOOL-U-037 |
| `channel_rate` | 429 | over the channel's 5,000 messages a day | yes | GW-I-109 |
| `not_current_run` | 409 | a call whose `X-TQA-Run-ID` isn't the conversation's current run | no | GW-I-109 |
| `idempotency_key_reused` | 422 | the same key with another body | no | GW-I-109 |
| `acceptance_failed` | 409 | `done` while the administrator's acceptance check is set and didn't pass: it failed or was stopped at its 10 minutes (`details: {exit_code, timed_out, output}`, the output's last 4 KiB), the report is for another command, or there is none (a runtime older than the gateway) | no | GW-I-198, TOOL-U-056 |
| `path_invalid`, `not_found`, `upload_too_large`, `uploads_full`, `files_unavailable` | 400, 404, 413, 409, 409 | a report's files or `take_files`: a reported path with a link anywhere in it, or that is no regular file (an absolute or `..` path is `invalid_request` above); a reported path that is missing, or a `take_files` path not in that report or no longer stored (its member task deleted); a reported file over `files.upload_bytes`; a report's files together over `files.uploads_bytes`, or the lead's `uploads/` full; the member's or lead's workspace service unavailable. `details: {path}` names the file. Files placed before a failure stay | no | GW-I-110 |

A fork that fails in `start_member` keeps the fork route's own codes.

### Tests that come first
- **PROTO-U-023:** each type in the table round-trips exactly in `tqa-types`; each missing required field and each
  unknown field is refused naming it; a v1.0 reader gets a marked unknown block.
- **GW-U-125 (new):** the brief rules, row by row, at each boundary (empty, at the limit, one byte over, NUL, a
  newline in `role`); `profile` absent, malformed and well-formed; every message type's text limit and required
  references; names made unique.
- **GW-I-106, the profile cases:** a helper from the person's own profile and from a shared one gets that profile's
  instructions, model, environment and skills, and the brief after the instructions; a profile tool and a connector the
  lead may not use are left out and listed; the network list stays the lead's; an unknown profile and another
  person's personal profile are `unknown_reference`; a profile naming a disabled model is `profile_unusable`; with no
  profile, the lead's own settings.
- **GW-U-069:** the role × route × addressee table, every cell, including `not_lead` for a member's `start_member`
  and `members_only` for the lead's `report`.
- **GW-U-126 (new):** the cursor as a pure function: an answer is recorded per key; a repeat answers the same; the
  seq advances only with the stored result; a result for an unknown key changes nothing; recorded answers at or below
  the advanced seq are pruned with it, and a run's remaining ones when it ends.
- **GW-I-195 (new):** a read is delivered exactly once at the crash point, through the batched persistence path
  (the read's `tool.result` arrives in a batch with other events). With failure injection in the batch's commit:
  - the transaction that writes the read's `tool.result` and the seq's advance fails before commit: both are
    absent, and the next read answers the same messages;
  - it fails after commit: both are present, and a later read with a new key answers nothing old.
- **TOOL-U-037:** the twelve schemas and the result texts above, the refusal shown as `<code>: <message>`, and who
  is offered which tools. A top-level task's requests carry `start_member` alone. After the first successful
  `start_member`, the next model request carries the ten, and every later request has the same tools prefix, which
  is one prefix change. A member's requests never carry a lead tool. A `hint` makes the result say to end the turn.
- **GW-I-196 (new):** no slot deadlock. With 4 slots and a per-user cap of 4:
  - the effective member cap is 3, and a 4th `start_member` is refused `member_limit` (`by: slots`);
  - a lead and 3 helpers, plus another person's task queued for a slot, all finish: waits answer `yield_slot` at
    once, turns end, idle helpers are evicted for the queued run and woken later on new sandboxes, and no run waits
    on a held slot past its bound;
  - an `agent_teams.max_members` written above the slots is refused.
- **PROTO-U-023, rendering:** `person_answer` and `person_message` are shown in the signed-in-person block,
  agents' messages inside the untrusted marker, and a body can't close either.
- **GW-I-106 and GW-I-109:** the routes against PostgreSQL with each refusal row above, in the order the slices
  build them.

## Slice S6 contract (the people's side; written before code, DEV-PLAN §1; reviewed 2 Oct 2026)

What the web builds against in slice 6: how people see and steer an agent team (point 4, UI-GUIDELINES §13b), and the
administrator's limits as runtime settings (point 3, ADR-0028). The runtime's routes and shapes (the S1 contract) don't
change. Where a route or shape already exists it is reused; everything new is here first, and a change is a change to
this record first.

### Conventions
- **Routes** are under `/api/v1/`, for a signed-in person's session or a personal token, as other task routes. A team
  is its lead task's person's: every route below answers as that task does. Its person sees everything (point 1);
  anyone else gets the task's own answer (`403 forbidden`, "conversation ownership is required", for another person's
  task, an administrator's included, as the task routes answer it; corrected 3 Oct 2026 from the `404` first written
  here, at the gateway half's first build. An administrator sees other people's tasks as metadata only, SEC-005, so
  never a team's messages, helpers' names or briefs).
- **Writes** carry an `Idempotency-Key`; errors are the gateway's usual shape; request bodies are decoded strictly;
  sizes are UTF-8 bytes; NUL is refused (as in the S1 contract).
- **Off means no trace.** With `[agent_teams] enabled = false` every route below answers `403 teams_disabled`, no task
  carries a `team`, and the session says so (below), so the web shows nothing of teams.
- **Live, never stored twice.** Team state reaches an open page as one browser-only control frame (below), after which
  the page reads the routes. The frame is never stored and never reaches a runtime, so it needs no protocol revision
  (ADR-0026; the rule recorded for C3 of the Sandboxes contract).

### The switch, for the page
`GET /api/v1/auth/me` gains `features: {"agent_teams": bool}` (required): the install switch, read at start. The page
shows team parts only while it is true.

### A task's place in a team (`Conversation.team`)
`Conversation` (the task list's items and `GET /api/v1/conversations/{id}`) gains `team`, `null` for a task in no team:
```json
{"role": "lead", "id": "<team>", "state": "running", "helpers": 3,
 "attention": {"stuck": 1, "failed": 0}, "waiting_for_you": true}
{"role": "member", "id": "<team>", "state": "running", "lead": {"id": "<task>", "title": "Report on Q3"},
 "name": "researcher", "helper_state": "working", "can_stop": true}
```
| Field | Rule |
|---|---|
| `state` | the team's: `running`, `stopping` or `ended`. An ended team's tasks keep `team` (the list still groups them), with no tools (point 10) |
| `helpers` | the lead's helpers now, deleted ones not counted (§13b: the count drops) |
| `attention` | how many are `stuck` and `failed` while the team runs; the list says "1 stuck" or "1 failed" in the count's place. Once the team is `stopping` or `ended` it is 0 and 0: nothing in an ended team waits on anyone (GW-U-169; the c16 UX review) |
| `waiting_for_you` | a `person_question` is unanswered (point 4) while the team runs; the task's status word is "Waiting for you". A stopping or ended team waits for no one: an answer would be `team_stopped` (corrected in the review of 2 Oct 2026) |
| `lead.title` | the lead task's title as its person sees it (`null` when untitled) |
| `helper_state` | as in the team below |
| `can_stop` | as in the team below |

**Who sees what (SEC-005; the review of 2 Oct 2026, H1).** Only the task's own person gets `team` as above. For anyone
else, such as an administrator's `GET /api/v1/conversations?all=true`, `team` keeps only `role`, `id`, `state`, and for
a lead `helpers` and `attention` (counts), for a member `lead: {id}`: no `lead.title`, no `name`, no `helper_state`, no `can_stop` and
no `waiting_for_you`, as `title` is dropped for them today. A helper's name and the lead's title are the person's
content, like a brief.

**The list.** `GET /api/v1/conversations` gains `scope=top`: members are left out, their lead carries them, and the
sidebar opens them from the team (§13b: helpers are never top-level rows and follow their lead). Without `scope` the
list is unchanged. With `q`, `scope=top` is ignored: a search finds a member, with its
`team.lead`, so the page shows it under its lead. While agent teams are off, `scope=top` is accepted and filters nothing
(off means no trace, in both directions: no member vanishes from a list).

### The team
`GET /api/v1/conversations/{id}/team`, `id` being the lead's task or any member's. `404 no_team` for a task in no team.
```json
{"id": "<team>", "state": "running", "stop_reason": null, "lead": {"id": "<task>"},
 "started_at": "…", "deadline_at": "…", "budget": {"tokens": 2000000, "spent": 412000},
 "waiting_for_you": {"seq": 14, "text": "Which quarter counts as Q3 for you?", "at": "…"},
 "helpers": [{"conversation": "<task>", "name": "researcher", "role": "researcher", "workspace": "own_copy",
              "state": "queued", "can_stop": true, "position": 2, "tokens": 120400,
              "activity": {"step": 4, "tool": "shell", "since": "…"},
              "last_message": {"seq": 12, "type": "report", "text": "North: 4.1 M …", "at": "…"}}],
 "version": 9}
```
| Field | Rule |
|---|---|
| `helpers[].state` | `working`, `waiting_lead` (also one that stopped without reporting), `queued` (with `position`: its place in the slot queue counted from 1, so 1 is next; the page says how many are ahead), `stuck`, `done`, `stopped`, `failed`, or `unknown` when the gateway can't read it (§13b: "State unknown", never a blank) |
| `helpers[].can_stop` | whether **Stop helper** would stop it now, as `stop_member` decides: `true` while the team runs for any helper but a `stopped` one or one that couldn't start (`failed` without `failed_run`); `false` once the team stops. A helper whose run failed keeps its seat until stopped, so the page offers Stop for it from this, not from `state` (GW-U-168, GW-I-260) |
| `activity` | the helper's current step: its number; `tool`, the tool of a call started and not yet ended in that step, else `null` (the model is thinking or writing); `since`, when that call started, or when the step started while `tool` is `null`. `null` when no run is active. The page words it ("Running a command"); a tool name it doesn't know, such as a connector's `<server>__<tool>`, reads "Using a tool" |
| `last_message` | the helper's latest message to the lead, its text cut to 300 characters at a character boundary (the page links to the full one) |
| `tokens` | the prompt and completion tokens of the helper's model calls since the team started, as the relay counts them for the team; the lead's own share is `budget.spent` minus the helpers' |
| `budget.spent` | may pass `budget.tokens` by the calls in flight when the budget ran out (GW-I-107). The page caps its meter and percentage at 100 % and shows the real figures in the tooltip |
| `waiting_for_you` | the latest unanswered `person_question` while the team runs, or `null`. A lead may ask again before an answer: an older question can still be answered by its `question_seq`, the task says "Waiting for you" while any is unanswered, and `already_answered` is per question |
| `stop_reason` | once stopping: `done`, `stopped` (a person), `budget`, `deadline`, `idle`, `lead_deleted`, `person_disabled`; this list is the contract (the column is free text). The page shows a value it doesn't know as "Stopped" |
| `version` | rises with every change; the frame below names it |

### The team's messages
`GET /api/v1/conversations/{id}/team/messages?after=<seq>&limit=<n>` (any team task's id; `after` default 0; `limit`
1–200, default 100: deliberately above the runtime read's 50, since a page reads the log whole) answers `{"items": [...], "last_seq", "more"}` (`last_seq`: the last item's seq, or `after` when there is none), in seq order: every message of the team (people
read everything, the S1 contract's "who reads what"), in the S1 read shape. A person's own messages carry `by`, the user's id, so the page says "You" (decided in the review of 2 Oct 2026: the
stored message's own `by` and sender name are the fixed "your person", which is all the lead's runtime read shows, so
no email or other personal data reaches the model; the lead acts for one person). The end note is one more type, written by the gateway when the team stops (point 10):

| `type` | From → to | Body beyond `text` |
|---|---|---|
| `ended` | gateway → `lead` | `reason` (as `stop_reason`), `summary` (the lead's `done` text, when it was `done`), `tokens: {total, members: [{name, tokens}]}`, `reports: [{member, seq}]`, `files: [{member, seq, name, size}]` (what each report handed back: the report's seq, no path). `text` holds the stop path's own fixed sentences ("handed back", never "taken"); the page words the fields itself and shows `text` only to a client that doesn't know them |

Decided 2 Oct 2026 (the lead): the end note is this stored message, not a task event, so no protocol change; point 10's
"posts the gateway's end note into the lead's task" means it shows there. Members never read `ended` (it is the lead's,
and the team has stopped), and a lead's read after `stopping` is `team_stopped`, so in practice only people read it; a
runtime that meets it reads it as a marked unknown block (TOOL-U-036). The lead task's
page shows it as the team's summary, in order with its turns.

### Live: `team.changed`
On the stream of the lead's task and of every member's task (`GET /api/v1/conversations/{id}/events?stream=1`), the
gateway sends `{"type": "team.changed", "team": "<id>", "version": 9, "last_seq": 15}` whenever the team, a helper's
state or the message log changes: unsequenced, never stored, at most one per second per team (the latest wins). The
page reads `…/team` when `version` rose, and `…/team/messages?after=` when `last_seq` did. A page opened later, or
after a reconnect, reads both once: nothing is lost with a dropped frame. Being unsequenced on a sequenced stream, it
never moves the page's stream cursor, and a replay (`after_seq`) never includes it. The fake front sends it too, so its
drift check covers it.

### Writing to a helper, and answering the lead
- **To a helper:** the existing `POST /api/v1/conversations/{member}/messages`, as to any task. The gateway also
  stores a `person_message` for the lead (S1 contract); nothing new for the page.
- **The lead's question:** `POST /api/v1/conversations/{lead}/team/answers` with `{"question_seq", "text"}` (text
  1–16,000 bytes) answers `201 {"seq"}`: a `person_answer` to the lead, which wakes it (point 4). The page sends the
  message box there while `waiting_for_you` is set; the card then says "Answered". Refusals: `unknown_reference`
  (422, not a `person_question` of this team), `already_answered` (409), `team_stopped` (409), `channel_rate` (429,
  with `Retry-After`: the team's log is full for the day, ADR-0014).

### Stop
`POST /api/v1/conversations/{id}/team/stop` with `{}` and an `Idempotency-Key`: on the lead's task the whole team
(`202 {"team": {"state": "stopping"}}`), on a member's task that helper only (`202 {"member": {"name", "state":
"stopped"}}`, and the lead is told, point 10). A repeat answers the same; a team already ended answers `409
team_stopped`, and a helper that failed before the stop arrived `409 member_gone`; a done one is stopped (4 Oct 2026; added in the
review of 2 Oct 2026: the lead's `stop_member` refuses it the same way). The composer's Stop still stops only that task's own run. Both go through the one stop path the
liveness slice builds (`teams/stop.rs`, for every reason): this route records the person's stop and adds no stop logic
of its own.

### Delete
`DELETE /api/v1/conversations/{lead}` of a lead with helpers is `409 has_helpers` unless `?with_helpers=true`, with no details: the error's `details` are field problems only,
so the page reads the helpers from the team (corrected 2 Oct 2026; `details: {helpers: n}` first written here fits no
error shape). "With helpers" counts its member tasks not deleted, in any team state: an ended team's
helpers are ordinary tasks by then, but point 5 deletes them with their lead, so the confirmation names them. With the
flag it deletes as point 5 says: the team's messages, every helper's task and its own copy; the
lead's workspace stays; the team's stop is the same one stop path (reason `lead_deleted`). The page's confirmation
names the helpers, read from the team. Deleting a member's task is as
any task's, and the lead gets its `member_deleted` notice.

### The administrator's limits (ADR-0028)
- **The numbers** join `GET /api/v1/admin/limits` as `AdminLimit` keys, present only while agent teams are on, written
  through the existing `PUT /api/v1/admin/limits/{key}` (version-checked, audited, reset by `DELETE`):

| Key | Unit | Applies | Bounds (file default) |
|---|---|---|---|
| `agent_teams.max_members` | `helpers` | `next_member` | 1–8, and never above `admission.slots - 1` or `admission.per_user - 1` (3) |
| `agent_teams.token_budget` | `tokens` | `next_team` | 10,000–100,000,000 (2,000,000) |
| `agent_teams.deadline_minutes` | `minutes` | `next_team` | 5–1,440 (120) |
| `agent_teams.member_idle_minutes` | `minutes` | `next_check` | 1–1,440 (10) |
| `agent_teams.member_stuck_minutes` | `minutes` | `next_check` | 1–1,440 (15) |
| `agent_teams.idle_stop_minutes` | `minutes` | `next_check` | 1–1,440 (30) |

  `unit` gains `helpers`, `tokens` and `minutes`; `applies` gains `next_member`, `next_team` and `next_check`. A write
  that would put `max_members` above the slots or the per-user cap minus one is refused as the set's order is today
  (`400`, `details[].reason: limit_order`, naming the bound). A lower `admission.slots` lowers the effective cap from
  the next helper started (point 3); the page says so beside the value. The order is checked one way only, on purpose:
  `max_members` above the slots is refused, but lowering the slots below `max_members + 1` is allowed, and the smaller
  number applies.
- **The two that aren't numbers:** `GET /api/v1/admin/agent-teams` answers `{"member_route": string|null,
  "acceptance_check": string|null, "version"}`; `PUT` takes the same with the version read (the first is 0). `member_route` must name a model route (`422
  unknown_reference`, `details[].field: member_route`); a route exists or it doesn't, with no disabled state, as a
  profile's route is checked (corrected 2 Oct 2026 in the gateway half: the `409 route_unusable` first written here
  could never happen); `acceptance_check` is one line, at most 4,096 bytes (`400`, naming it). Administrators only;
  audited; `403 teams_disabled` while off.
- **The switch itself** stays in `gateway.toml` (it needs a restart); the page shows it read-only.

### Refusals (new in slice 6)
| Code | Status | When | Test |
|---|---|---|---|
| `teams_disabled` | 403 | any route above while agent teams are off | GW-I (S6, to claim) |
| `no_team` | 404 | `…/team`, `…/team/messages`, `…/team/stop`, `…/team/answers` on a task in no team | GW-I (S6) |
| `already_answered` | 409 | an answer to a `person_question` already answered | GW-I (S6) |
| `member_gone` | 409 | Stop on a helper that failed (a done one is stopped, 4 Oct 2026) | GW-I (S6) |
| `channel_rate` | 429 | an answer once the team's log is full for the day (ADR-0014), with `Retry-After` | GW-I (S6) |
| `has_helpers` | 409 | deleting a lead with helpers without `with_helpers=true` | GW-I-108 |
| `team_stopped` | 409 | stopping a member, or answering, once the team stopped (the S1 code) | GW-I-108 |
| `unknown_reference` | 422 | an answer's `question_seq` that isn't this team's `person_question`; a `member_route` that names no route | GW-I (S6) |
| `invalid_request` | 400 | a field breaks its rule (`details[].reason`, `limit_order` for a team cap above the slots) | GW-I (S6), GW-I-196 |

### Tests that come first
- **Gateway (slice 6's gateway half):** the human routes against PostgreSQL with every refusal row above; `scope=top`
  and search; `team.changed` sent on each change and never stored; the answer waking the lead; Stop on the lead and on
  a member (GW-I-108); the end note's `ended` message (GW-I-108); delete with helpers (GW-I-108); the limits and the
  two settings, with the order check (GW-I-196); the acceptance check's command from the setting (GW-I-198); an administrator never reads another person's team, and SEC-005's admin listing with a lead and a member shows
  `team` without names, titles or questions. Claimed by the gateway's owner when they start.
- **Web** (claimed 2 Oct 2026), each against the fake front, which answers exactly this contract (its drift check),
  with WEB-E2E-024 in the browser:

| ID | What |
|---|---|
| WEB-U-220 | Helpers grouped under their lead in the task list: the chevron and count, "1 stuck" / "1 failed", `scope=top`, a search result shown under its lead, loading and error |
| WEB-U-221 | The team strip and **All helpers**: each state's words and tone, `State unknown`, budget used and time left with exact figures in the tooltip, "+2", the 390 px line |
| WEB-U-222 | Team messages and the end note as quiet lines in the lead's transcript, in order with its turns; a long one ends in "…" and opens in full |
| WEB-U-223 | A helper's page: "Helper of {lead} · {name}" with the link, the **Brief from the lead** card (folding over 6 lines), the composer notice |
| WEB-U-224 | **Waiting for you**: the status word over "Working…", the question card, the message box sending the answer, "Answered", and each refusal in words |
| WEB-U-225 | **Stop team** and **Stop helper**: the menu items, their dialogs, each answer and refusal |
| WEB-U-226 | Deleting a lead task: the confirmation names its helpers and sends `with_helpers` |
| WEB-U-227 | The administrator's helper limits and the two settings: runtime rows with their units, the order refusal, the switch shown read-only |
| WEB-U-228 | Switched off (`features.agent_teams` false): no trace anywhere |
| WEB-U-229 | `team.changed`: the page reads only what moved; a page opened later or reconnected reads once |

## Links

- PRD FR-30, FR-31, FR-32, FR-36, §12 items 11, 18 and 20; ARCHITECTURE §4.5, §4.6, §16; DEV-PLAN Phases 10 and 12
- ADR-0012 (agent profiles), ADR-0014 (channels), ADR-0016 (admission), ADR-0019 (relay and metering), ADR-0026
  (protocol revision), ADR-0028 (runtime-mutable settings), ADR-0030 (workspace service), ADR-0035, ADR-0037
