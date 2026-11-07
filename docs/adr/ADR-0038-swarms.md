# ADR-0038: Swarms: one goal across a person's agent teams

## Status

Accepted (design; built after agent teams, DEV-PLAN Phase 12). The owner agreed this shape on 2026-10-02, and the
two open points were decided that day (under "Decisions (2 Oct 2026)").

## Date

2026-10-02

## Context

An agent team (ADR-0034) is one lead and its helpers working on one task. Some goals are bigger than one team's
work: a quarterly report needs the finance numbers, the market picture and the write-up, each a team's job, and someone
to put the parts together. The owner's word for that is a **swarm**: agent teams working on one goal, coordinated the
way a lead coordinates its helpers, one level up.

ADR-0034 point 13 left room for this. Messages are addressed to a conversation and delivered by the gateway, so a lead
writing to a coordinator is the same operation as a helper writing to its lead. This record decides what a swarm is,
who talks to whom, what a person sees and does, and what stays out.

## Decision

1. **Scope: one person's own teams.** A swarm is made of one person's tasks, acting as that person, with that person's
   files, keys (ADR-0037) and network rules (ADR-0035).
   - **Not in scope:** publishing a swarm, sharing one between people, and outside agents. An open agent-to-agent
     standard (A2A) is out of scope. If that ever changes, "Waiting for you" (point 5) maps onto A2A's
     `input-required` task state.

2. **A swarm is a goal the person sets up.**
   - The person names the **goal** and its **parts** ("Finance: the numbers", "Market: the competitors"), and picks an
     agent profile for each part: their own, or one an administrator shared (ADR-0012).
   - **Each part runs as a new lead task** from its profile, owned by the person. It leads its own helpers as in
     ADR-0034, with every rule there unchanged.
   - **Or the coordinator proposes the split:** the person states the goal; the coordinator proposes parts with a
     profile for each (`propose_parts`); the goal page shows the proposal as a "Waiting for you" card, and nothing
     starts until the person confirms it, edits it, or declines it.
   - Unlike an agent team, which a person starts by asking in words, a goal has a setup page (point 6), because the
     person chooses parts and profiles.

3. **One coordinator, and the lead-and-helpers rules one level up.**
   - **The coordinator** is one task: one the person picks, or else the goal's first task, made for the purpose. It
     talks to the leads, puts the parts together, and decides for the goal. It doesn't lead helpers of its own: its
     team is the leads.
   - **Leads talk only to the coordinator**, only inside their goal, and only while the goal runs. **Helpers never
     leave their team:** they have no goal tools, and nothing reaches them from outside their team except through
     their lead.
   - **Enforced by the gateway, as in ADR-0034:** roles (`coordinator`, and `lead` with its goal) are stored when the
     goal starts and no agent can change them; tools differ by role; the per-run token says who calls; the gateway
     delivers each message only to its addressee.
   - **Tools:**
     - coordinator: `propose_parts`, `tell_lead`, `ask_leads`, `read_from_leads`, `decide_goal`, `take_files`,
       `ask_person` and `goal_done`;
     - a lead in a goal: `send_to_coordinator` (an answer, a report with files, or a question), next to its own team
       tools. Coordinator messages arrive in the lead's inbox (`read_reports`), marked as from the coordinator.
   - **Same storage and delivery:** one stored, addressed log per goal, on the channel storage (kind `goal`), with
     per-reader last-read seqs, exactly-once reads (ADR-0034's S1 contract), waking instead of waiting, and bounded
     waits that give way to queued runs.

4. **What crosses teams, and the limits.**
   - **Only messages and their attached files cross teams.** No workspace is shared between teams. A lead attaches
     result files to a report; the coordinator places them in its own workspace with `take_files`. The coordinator
     can attach files to a message for a lead, which the lead takes the same way.
   - **One budget and one deadline for the goal,** covering the coordinator, every lead and every helper, enforced at
     the relay as in ADR-0034 point 8. Each team draws on the goal's budget and has none of its own while in a goal.
   - **At most two levels** (coordinator → lead → helper) **and no cycles:** a task is in at most one goal, as its
     coordinator or as one lead; a coordinator is never a lead or a helper; a helper is never a lead.
   - **Admin limits,** runtime-mutable (ADR-0028): `swarms.enabled`, `swarms.max_parts`, `swarms.token_budget` and
     `swarms.deadline_minutes`. Parts fit the admission slots as ADR-0034 point 3 requires of helpers: every waiting
     task gives way to queued runs, and the caps keep a goal's tasks able to run alongside each other.
   - **Everything is logged:** every message, file hand-over, decision and stop is in the goal's log and the audit.

5. **People: each page's message box talks to that page's agent.**
   - **The goal page** talks to the coordinator ("Reply to the coordinator…").
   - **A lead's page** talks to that lead; a line under the box says "The coordinator sees what you send here." The
     coordinator gets a copy, as a lead does of a person's message to a helper.
   - **A helper's page** talks to that helper; "The lead task sees what you send here." (ADR-0034).
   - **Questions:** an agent that needs the person asks (`ask_person`), and its own conversation shows a "Waiting for
     you" card, answered in that page's box. The goal page and the goal's row in the task list carry the amber mark
     while any of its agents waits.
   - **Stop** a helper, a team (from its lead's ⋯) or the whole goal (from the goal page's ⋯). Stopping the goal stops
     every team in it; stopping one team tells the coordinator.

6. **What a person sees** (the goal-page mock-up agreed with the owner).
   - **The goal page:**
     - the header: the goal's title, its status, and ⋯;
     - a summary line: the number of teams, "Budget 34 % used" and the time left;
     - a row per team: state dot, name, part, state in words, and Open;
     - "Conversation", with a filter (All, or one team): each message headed "Coordinator → Finance lead · 09:14",
       its full text (folded with Show all when long), its files as chips, and each reply under its request
       ("↳ Finance lead → Coordinator · 09:31");
     - a "Waiting for you" card when the coordinator asks;
     - the message box, "Reply to the coordinator…".
   - **Setting up a goal** is a page (the UI standard's Page control): the goal, its parts with a profile each, the
     coordinator, and the budget and time within the admin's limits. When the coordinator proposes the parts instead,
     the same page opens filled in, to confirm or edit.
   - **The task list:** the goal is one item, its teams are under it, and each team's helpers are under that. Rows are
     one line, with chevrons, and nothing opens on its own (UI-GUIDELINES §13b).
   - **A lead's page** shows the goal messages it sent or received as quiet lines, as it shows its team messages.

7. **Lifecycle and deletion.**
   - **A goal runs** from its setup until the coordinator's `goal_done`, a person's Stop, its budget or deadline, or an
     idle time with no message and no active run. Stopping ends every team in it (ADR-0034 point 10) and posts an end
     note on the goal page: the reason, what each team used, and the reports and files taken.
   - **After the stop,** every task is an ordinary task again, as after an agent team.
   - **Pausing** (decided): Pause goal on the goal page's ⋯ lets each team finish its current step and then wait; no
     new run starts in the goal until Resume. The deadline keeps running, and the goal page says it is paused.
   - **Deleting the goal** removes its log and any files not yet taken (PRD §12 item 18), and deletes its teams as
     deleting a lead's task does: the coordinator's and the leads' tasks, the helpers' tasks and their copies. The
     lead tasks' own workspaces stay.

8. **Orchestration and compatibility** follow ADR-0034 points 11 and 14:
   - data (`goals`, `goal_parts`) plus the same reconciler loop;
   - HTTP routes under `/runtime/goals/`, with no session frames, so an older runtime simply gets no goal tools.

## Security

- **No widening:** every task in a goal acts as the person; each lead takes its profile narrowed to the person's
  reach, and its helpers stay within that lead's (ADR-0034 Security, ADR-0035).
- **Isolation:** teams share no workspace; only addressed messages and files taken by the receiver cross, all logged.
- **Amplification:** two levels, the parts cap, per-user admission and one goal budget at the relay.
- **Injection:** messages between agents are marked untrusted; a person's messages are marked as a person's
  (ADR-0034's S1 contract); the coordinator decides, and people can read everything and stop anything.
- **No sharing:** a goal, its log and its tasks are one person's; administrators see metadata as for any task.

## Decisions (2 Oct 2026)

1. **Pause goal:** yes, in the swarm phase (point 7).
2. **"Coordinator"** stays the word in the product.

## Alternatives considered

- **Leads talking to each other directly.** It is less predictable (ADR-0034's group-chat finding), and harder to
  follow; one coordinator keeps one place where the goal is decided.
- **One big agent team with more helpers.** That puts every helper's work through one lead's context, which can't
  hold a large goal; parts with their own leads split the context the way the work splits.
- **A protocol between installations (A2A).** It is out of scope: no outside agents and no sharing (point 1).

## Test rows reserved (Phase 12)

| ID | Pri | What |
|---|---|---|
| E2E-SWARM-009 | P2 | A goal end to end on the fake model: a person sets up two parts from profiles, the coordinator asks both leads, each team works with a helper, the leads report with files, the coordinator takes them and calls `goal_done`; the goal page shows the conversation in order and the end note |
| E2E-SWARM-017 | P2 | The coordinator proposes the parts and the person confirms them; a lead's question shows "Waiting for you" and the amber mark on the goal; a person stops one team (the coordinator is told) and then the goal at its budget |

The gateway and tool rows are numbered when Phase 12's contract is written, before any code (DEV-PLAN §1).

## Links

- PRD §6 (later), §12 items 18, 20 and 21; ARCHITECTURE §16.4; DEV-PLAN Phase 12; UI-GUIDELINES §13c
- ADR-0012 (agent profiles), ADR-0028 (runtime-mutable settings), ADR-0034 (agent teams), ADR-0035, ADR-0037
