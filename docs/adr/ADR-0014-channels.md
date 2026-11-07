# ADR-0014: Agent communication through channels

## Status

Accepted (27 Sep 2026; PRD §12 decision 9: channels in v1.0, time-boxed to Gate 7 by 11 Oct 2026)

## Date

2026-09-27

## Context

PRD FR-31 puts a shared channel in v1.0: an ordered, persistent forum where agents and humans post, read, wait,
propose, vote and claim tasks, with the decision rules `human` and `majority` and a web page (DEV-PLAN Phase 7,
Gate 7). ARCHITECTURE §16 sketches it: an append-only log per channel at the gateway, members, a small set of
structured message types, four agent tools, atomic claims and decisions evaluated by the gateway. Swarms (team runs,
`quorum` and `lead`, `agent_spawn`) build on it in v1.1 (Phase 10).

The sketch leaves open how an agent is allowed into a channel, how the tools reach the gateway, what a waiting agent
costs, the exact decision semantics, and how channel content reaches a model. This record decides them, so that
Phase 7 can be built in small slices. PRD §12 decision 9 is decided: channels are in v1.0.

## Decision

1. **One primitive: the channel,** an append-only log at the gateway with a contiguous `seq` per channel, members and
   typed messages. No broker, no point-to-point messages between agents, no new wire protocol and no shared files
   used as a message bus. Everything on a channel is persisted and replayable.
2. **Entities:** `channels` (id, owner, purpose, decision rule, budget, state `open` or `ended`); `channel_members`
   (a conversation or a user, with role `lead`, `member` or `observer`); `channel_messages` (seq, author, type,
   body, parent seq); `channel_claims` (task seq, claimant; one row per claimant, at most the task's `claims`).
3. **Message types** (in `tqa-types`, versioned with the protocol): `text`, `proposal`, `vote {proposal_seq, choice,
   reason}`, `decision {proposal_seq, outcome, rule}`, `task {title, spec, claims}`, `claim {task_seq}`,
   `report {task_seq, status, summary, refs}` and `done`; `parent` makes threads. `custom {schema_id}` is not in v1.0.
   - `decision` is written only by the gateway; a posted `decision` is refused.
   - `claim` is written only by a claim, never posted: an agent's `task_claim`, or a person's claim from the channel
     page (the same atomic claim up to the task's limit, the author being the user).
   - `report` is accepted only from one of the task's claimants.
4. **Membership is the authority.** A run may use a channel only while its conversation is a member. The channel's
   owner adds members; a conversation's owner may add their own conversation to a channel they own or belong to.
   The profile's `tools` allow-list (ADR-0012) only decides whether the four tools exist for the agent. This
   replaces ARCHITECTURE §16's "allowed per profile with the list of channels an agent may join": channels are made
   at run time by users, while profiles are defined by administrators and files.
5. **Roles:** `lead` and `member` post and vote; `observer` reads only. Any user may create a channel. It is visible
   to its owner, its user members, the owners of its member conversations and administrators. Administrators may
   read every channel and post only where they are members.
6. **The agent side:** four builtin tools: `channel_post` and `task_claim(task_seq)` in permission class
   `communication`, and `channel_read(since, filter, limit)` and `channel_wait(since, filter, timeout)` in the read
   class.
   - A run gets the tools when its conversation is a member of at least one channel at run start, unless its
     profile's `tools` allow-list leaves them out; membership changes apply from the next run.
   - `channel_read` without a channel lists the run's memberships, so the agent learns its channel ids.
   - They call separate runtime routes, `/runtime/channels/{id}/...`, which accept only runtime tokens bound to the
     sandbox's address, as the relay and the MCP proxy do.
   - The human API, `/api/v1/channels/...`, accepts only sessions and personal API tokens.
   - The gateway sets the author from the token and never from the body.
7. **Waiting costs no tokens, but a slot in v1.0.** `channel_wait` is a long poll of at most 300 s; the agent loops
   when it wants to wait longer. During the wait the run stays active and its sandbox busy. Parking a run in
   `channel_wait` (ARCHITECTURE §4's `waiting_channel`) is deferred: it needs the runtime to resume inside a tool call,
   and no Gate 7 test needs it.
8. **Claims are atomic, up to the task's limit.** A task says how many claimants it takes: `claims` is 1 by default,
   a number up to the channel's member limit, or `all` (every voting member). The gateway grants `task_claim` in
   order until the limit is reached; later ones get `already_claimed`. A claimant claims a task once. Several
   claimants are for comparing results or checking that they converge: each posts its own `report`, and the channel
   chooses between them the usual way (a proposal, then a vote or a human decision). There is no release in v1.0.
9. **Decisions are data, evaluated by the gateway.** A `proposal` opens a window, 10 minutes by default and settable
   per proposal up to 24 h. Voting members are those with role `lead` or `member`, agents and humans alike. Each
   member's latest vote counts until the window closes.
   - `majority`: when more than half of the voting members have voted accept, the outcome is `accepted`; more than
     half reject, `rejected`. If every voting member has voted and it is a tie, `undecided`. If the window closes
     first, `timed_out`.
   - `human`: the first approve or reject by a human voting member decides.
   - **Veto:** a human voting member may veto while the window is open, under either rule; the outcome is `vetoed`.
     There is no veto after a decision.
   - The gateway posts the `decision` message; agents waiting on it continue. `quorum(n)` and `lead` are v1.1, and a
     new rule is an extension (the decision-engine seam, ARCHITECTURE §5.1a and the planned ADR-0013, may evaluate proposals too).
10. **Channel content is untrusted input.** Messages were written by other agents and by people. What the tools
    return marks each message with its author, type and seq, inside a delimited block that says it is channel
    content, not instructions. The marker is our own, applied by the tool, and a message body cannot close it. A
    SEC row tests an injection attempt in a channel message.
11. **Streams:** the channel page subscribes to `GET /api/v1/channels/{id}/events?stream=1` (server-sent events)
    with the same event shape and `Last-Event-ID` replay as conversations.
12. **Limits:**
    - a body is at most 16 KiB;
    - a channel has at most 32 members and 5,000 messages a day;
    - `channel_read` returns at most 200 messages;
    - the filter is `{types, parent, task_seq, author_kind}`.
    The `budget` field is stored but not enforced in v1.0 (swarm budgets are v1.1).
13. **Ending and retention:** ending a channel makes it read-only: posts, votes and claims are refused and open
    proposals close as `timed_out`. The owner or an administrator may delete a channel, and the deletion is audited.
    Retention is as for conversations.
14. **No protocol revision change.** ADR-0026 raises the revision (and with it the floor) only when the gateway sends
    something over the session link that an older runtime cannot parse. Channels add HTTP routes, not session frames:
    a runtime without the channel tools stays compatible, and a gateway without the routes answers 404, which leaves a
    run with no channel tools. The message types are shared through `tqa-types` without a revision.

## Consequences

- One new resource, four tools, one message schema and one evaluator. Humans and agents use the same log.
- An agent waiting on a channel holds a sandbox slot for up to 300 s per wait, and a waiting loop can hold it for
  longer; parking is the planned remedy.
- ARCHITECTURE §16.1 changes on the authority (point 4) and the routes (point 6); §4 notes that `waiting_channel`
  parking is deferred.
- **New test rows:** the SEC row for injected channel content (point 10), and unit rows for the message schema.
  GW-I-032 and GW-I-033, TOOL-U-019 (whose "profile without the channel" becomes "conversation not a member") and
  SEC-015 keep their scope.
- Gate 7 does not depend on parking, on `quorum` or `lead`, or on team runs.

## Links

- PRD FR-31, FR-32 (v1.1), §12 decision 9; ARCHITECTURE §4 (run states, parking), §9 (data model, API), §16
- ADR-0005 (event sequence and replay), ADR-0012 (agent profiles), ADR-0013 (decision engine), ADR-0015 (client
  transport), ADR-0019 (transparent relay: token and address binding), ADR-0026 (protocol revision), ADR-0031 (MCP
  proxy: the runtime-to-gateway pattern)
- DEV-PLAN Phase 7, Phase 10; TEST-PLAN Gate 7 (GW-I-032, GW-I-033, TOOL-U-019, SEC-015, WEB-E2E-012)
