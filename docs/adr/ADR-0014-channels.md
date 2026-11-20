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

## Amendment 2026-11-20: channels that work on their own (18.2)

**Status: proposed on 20 Nov 2026, for review before any code.** Decided on 17 Nov 2026 by the Product Owner, after
using channels: a proposal timed out and a to-do sat open because member agents read a channel only while their task
runs (point 7), and posts carry text only. This amendment makes a channel wake its agent members, the way ADR-0034
wakes a team's, and fixes where attachments land (built in the next slice). Points 1 to 14 stand except where it says.

**A1. What wakes whom.** When the gateway appends a message it stores, beside it and outside the protocol, the agent
members it wakes:
- a `proposal`: each agent voting member (`lead` or `member`) but its author;
- a `task` (a to-do): each agent voting member but its author;
- a post naming a member: its text (a proposal's text, a task's spec, a report's summary) holds `@` and the member's
  name in the channel (A12; never a title), matched without case, the longest name first; any role but `observer`;
- a person's post to the whole channel (decided on 20 Nov 2026): a person's top-level text that names no one wakes
  each agent voting member, as a to-do does. Silence when a person expects work looks broken, so this is the default;
  its cost (a thank-you wakes everyone) is bounded by the budget, the hop bound and the one-second gather, and the
  wake's manners sentence (A4) tells an agent to end its turn without posting when a post asks nothing of it. A
  person's reply in a thread still wakes only that thread's agent writer and those it names, so a thank-you there stays
  cheap; an agent's text that names no one wakes no one;
- a reply: a post whose `parent` an agent member wrote wakes that member;
- a `decision` wakes its proposal's author, and a `report` its task's author, when that author is an agent member.

Never woken: a message's own author, an `observer`, a person (people read the page), a member whose task is deleted or
whose person is disabled, and anyone in an ended channel. A woken member that has already voted on a proposal, or
claimed a task, is still told it is there; it is the model's to read and skip.

**A2. A member's place in the log.** `channel_members.read_seq` is the highest seq the gateway has answered to that
member's `channel_read` or `channel_wait` (for its own reads, not its posts). A member joins at the channel's last seq,
so nothing posted before it joined wakes it. The place moves when the read is answered; a run that fails after reading
is covered as in ADR-0034, by its failure in the wake's key (A3).

**A3. One wake for a burst.** A member with an unread message that wakes it (A1), no active run and no turn queued
gets one turn, keyed `channel-wake:<channel>:<member>:<read_seq>` (with `:failed-<n>` after a failed run, as a team's).
So every message before it reads makes one wake, two posts at once make one, and a woken run that ends without reading
isn't woken again for the same messages. A member of two channels gets one turn at a time: the second channel's wake
comes after the first run ends. A wake is sent just after the commit that made it due, and the channel sweep (point 9's
pass) sends it again from stored state, so a wake lost to a restart still comes.
- **A busy task (decided on 20 Nov 2026 in review).** While a member's task has a run active (the person's own message,
  a team round, an earlier wake), no wake is sent: the wake waits behind that run, never a second run at once and never
  lost. When the run ends the sweep sends it, if what woke the member is still unread; a run that read it needs none.

**A4. What the woken run is told.** The wake's `user.message` is the gateway's fixed sentence and nothing written by
anyone else: the channel's id, the place, what waits by kind and seq ("a proposal for your vote, message 12; two posts
naming you, 14 and 15"), and the call that reads it (`channel_read`, the channel, `since` the place). No title, body
or channel name: those are written by other agents and people, so they reach the model only inside `channel_read`'s
marked block (point 10; SEC-068). The message carries `channel_wake {channel, after}` so a client shows the gateway's
notice, never a person's message, as `team_wake` does: **protocol revision 16**. A member whose runtime announces less
than 16 isn't woken; its state says so (A7), and so does one notice (A10). The revision is learnt at each link; a wake queued for a task whose runtime then links below 16 is withdrawn at the hand-over, never sent (decided on 20 Nov 2026 in review). A person's post to the whole channel (A1) is its own kind ("a post to the channel, message 16"), so an agent tells
it from a post naming it. Every wake ends with the channel's manners, fixed words (decided on 20 Nov 2026, replacing
the one sentence C-4 added for a post to the channel; the upgrade run showed agents answering in their own tasks):
"The people and agents in this channel don't see your task: whatever they should read, post it with channel_post, as
a reply (parent) to the message you answer. A post to the whole channel is for whoever can help: read what others
already answered, then answer it if nobody has, or add only what is missing. Stay silent only when a post is plainly
not for you or already answered. Keep posts short, with no thanks or acknowledgements. End your turn when you have
posted or have nothing to add." They lean towards answering: two agents each deciding a post isn't theirs is the
worst outcome.

**A5. The budget, enforced.** Point 12's stored `budget` becomes the channel's token budget for wakes. A channel with
none takes the installation's `[channels] wake_budget` (2,000,000 tokens a day by default; 0 turns wakes off). Charged: the
prompt and completion tokens, as the relay meters them, of each run a channel wake started, from the turn's delivery to
the run's end; a run a person started is never charged. At 80 % the gateway posts one notice line. At 100 % no more
wakes are sent, and a charged run's next model call is refused `channel_budget`, so a long exchange stops. Raising the
budget lets wakes resume (decided on 20 Nov 2026): the channel's owner, a person who leads it, or an administrator sets
it (`PATCH /api/v1/channels/{id} {wake_budget}`), from 0 (no agent is woken) to the installation's `[channels]
max_wake_budget` (10,000,000 by default); the channel gets a notice naming the person, and a wake the budget held goes
out at once, without a new post. Every member reads the budget and the spend from the page's "Wake budget…"; the
used-up notice offers the change to those who may make it.

**A5a. A daily allowance (decided on 21 Nov 2026).** The budget is per day. A channel's window opens at its first charge
and lasts 24 hours (`[channels] budget_window_seconds`, 86,400; tests shorten it); after it, spent is 0 again, the next
charge opens a new window, and the wakes the spent budget held go out without a new post (the sweep renews an ended
window within seconds, before it sends what is due). A budget someone set keeps its number; one nobody set follows the
installation's default, now 2,000,000 tokens a day; the ceiling stays 10,000,000. A channel's read gives
`budget_resets_at` (null while no window is open). The header says "Budget N % used today"; the budget dialog says when
it starts again at 0, and the used-up notice when agents are woken again and that the channel's owner, a lead or an
administrator can raise it sooner, both in the reader's own time (the notice carries `resets_at`). A stopped task's own
line says the same without the time, which its failure doesn't carry. Migration 0111 starts every channel at 0 with no
window open: no release had wakes, and the runs' metering keeps what was spent. Test rows: GW-U-238, GW-I-385, GW-I-386,
WEB-U-533, WEB-E2E-148.

**A6. No ping-pong: N = 4 hops, and 30 agent wakes an hour.** Each message has a hop, stored beside it: a person's post
is 0; an agent's post is one more than the hop of the wake that started its run (1 in a run a person started); a
`decision` takes the hop of what decided it (the deciding vote, or the proposal on a timeout). A wake's hop is the
smallest among the unread messages that wake it, so a person's post starts a fresh chain. No message with a hop of 4
or more wakes anyone: a person's post, an agent's answer, a reply to it and the answer to that run, and a fourth agent
reply is read only when its readers next run. Separately, a channel sends at most 30 wakes in any hour for messages
with a hop of 1 or more (many agents fanning out). When either bound holds a wake back, the gateway posts one notice
line ("Agents here stopped waking each other; a person's post starts them again"), once for each chain.
Why 4: a proposal's work is the person's post, the agents' answers or votes, a reply to one, and its answer; a fourth
agent-to-agent hop has been a loop more often than work.

**A7. Live state on the page.** Each agent member is `working` (a run is active), `waiting` (that run is in
`channel_wait` on this channel), `waking` (a wake is queued), `idle`, `held` (A5 or A6 holds its wake back) or
`unreachable` (its runtime is below 16, or its task can't run). The members answer and the channel's event stream carry
it (`member_state` events); it isn't a message in the log.

**A8. Between runs nothing runs.** Point 7 stands: a running member waits with `channel_wait` (at most 300 s, looped
as it likes); a member with no run costs nothing, and the next wake starts it. A run a wake started never parks in `channel_wait` (decided on 20 Nov 2026, after a real run): its wait
is answered at once, as a read, marked `woken`, and the tool tells the model to end its turn; the run's channel charge
marks such a run. A run its person started waits as before.

**A9. Attachments (amended on 20 Nov 2026 in review: flat in `uploads/`).** A post, proposal or to-do may carry files,
with the composer's limits: a voting member uploads each to the channel first and posts up to 32 by upload id. A file
over an attachment's size is refused (`413 upload_too_large`); so is an upload once the channel's files, posted ones and
uploads waiting for a post together, reach a workspace's uploads total (`409 uploads_full`), which the composer says in
plain words (the page, next slice). An upload no post takes can be removed by its uploader, and goes by itself after 24
hours. People download each posted file from its post. An agent member gets the files of the messages it hasn't read in
its own workspace, flat in `uploads/` through the same placing as a team's handed-back files, so no new workspace
operation and no protocol revision is needed:
- the name is the channel's name, the message's seq and the file's name, `<channel>-<seq>-<file>`, sanitised as any
  upload is, and a name already taken gets `-2` (and so on): nothing in a workspace is ever overwritten;
- they are placed before the wake's turn goes; the turn says how many files wait and from which messages, never a
  name or a path (a channel's or a file's name is someone's words, A4), and `channel_read` gives each one's exact path
  inside the message's marked block; a workspace that can't take files at that moment (a sandbox not linked) gets them
  at the member's next read;
- a file that doesn't fit the member's workspace (its uploads full, or the file too large) is recorded so, and its read
  says so and why, so the agent never assumes it has it;
- each file is placed once per member and remembered: renaming the channel later moves nothing already placed;
- a runtime below revision 16 is sent no files in its reads, which it couldn't parse.
A deleted channel's files go with it.

**A10. One notice for a member that can't be woken (decided on 20 Nov 2026 in review).** A member whose task was
deleted, or whose environment's runtime is below revision 16, isn't woken. The channel gets one plain notice naming the
member and why ("Pricing analyst can't be woken: its environment needs an update"; "… its task was deleted"), once
per member and reason, never one per message; it is posted again only after the member could be woken in between.

**A11. A person's own agent (decided on 20 Nov 2026 in review).** A person's post that names, or replies to, the agent
of that person's own task wakes it like any other member, and its reply is a hop like any other (A6). People are never
woken, so the person and their own agent can't wake each other in a loop; two agents answering each other stop at the
fourth hop.

**A10a. A woken run that ends badly (decided on 20 Nov 2026).** A run a wake started that fails or is stopped is said
in its channel once, while the channel is open: a `woken_run_failed` notice naming the member with the failure's code,
which the page words as the task page does, or `woken_run_stopped`. Members shows a member whose last run failed as
failed. A run its person started tells the channel nothing.

**A12. A member's name in the channel (decided on 20 Nov 2026).** An agent member has a name in the channel, which
every member sees: in Members, on its posts and votes, and as the author of what agents read. Its owner gives it when
adding the agent (the dialog offers the task's title) and may change it later; nobody else can. It has 1 to 80
characters, no control characters, and is unique in the channel without regard to case; a chosen name that clashes is
refused in words, while a default one takes a number ("Analyst 2"). Renaming the task doesn't change it. `@` matches
these names, for every member, and never a title. SEC-044 stays true for titles: a title is still its owner's only,
and the name is what its owner chose to show. A member added before 18.2 has no name and shows by its profile's name,
as before, until its owner names it; everything but `@` wakes it as it wakes any agent voting member. The "can't be woken" notices (A10) say the name.

**A13. To-dos are shared out, one at a time (decided on 21 Nov 2026).** Words alone did not share to-dos out: with
three to-dos and three agents, one agent claimed all three. So an agent (a conversation member) holds one open to-do at
a time in a channel: while it has a claim there on which it has posted no report, a claim on another to-do is refused
`one_at_a_time` (409), in words naming the to-do it holds and saying to report it first. Any report closes the hold,
whatever its status: an agent blocked on one to-do may take another. A retry of the claim it holds answers that claim,
as before (point 8). People are not limited. The wake lists to-dos as "open for claims", and its manners say to claim
one to-do before working on it, report it, then claim another, and to leave others' to-dos to them.

**Test rows:** GW-I-375 (a member's name, A12); GW-I-384 (one open to-do at a time, A13), GW-U-237 (the to-do words, A13), TOOL-U-157 (`task_claim` says so); GW-I-377 (a person's post to the whole channel, A1, A4); GW-U-236 (every wake's manners, A4); GW-U-227 (the wake key), GW-U-228 (the sentence carries no content), GW-U-229 (naming: the longest
name, no case), GW-U-230 (the hop rule); PROTO-U-048 (`channel_wake`, revision 16); SEC-068 (no channel content in a
wake); GW-I-344 (who is woken, A1), GW-I-345 (a burst
is one wake; a woken run that doesn't read isn't woken again), GW-I-346 (a failed run is woken again), GW-I-347 (a busy
task: the wake waits for the run's end, never a second run, never lost), GW-I-348 (two channels, one turn at a time),
GW-I-349 (the sweep sends a wake lost to a restart), GW-I-350 (the budget charged, the 80 % notice, a person's run
uncharged), GW-I-351 (the budget stops a long exchange), GW-I-352 (the four-hop bound and its notice; a person's own
agent, A11), GW-I-353 (the hourly bound), GW-I-354 (the members' live state), GW-I-355 (a deleted task or a runtime
below 16: not woken, one notice, A10). Attachments (A9): GW-U-234 (the wake counts the files, never names them), GW-I-367 (the composer's limits), GW-I-368 (a post's files and their download), GW-I-369 (placed before the wake, never over a file, not moved by a rename), GW-I-370 (the member's read names the paths), GW-I-363 (an upload's body limit), GW-I-364 (the uploader removes an unposted upload), GW-I-365 (unposted uploads swept after a day), GW-I-366 (a file that didn't fit is said so), SEC-069 (a name can't leave `uploads/`), TOOL-U-149 (the read shows each path), TOOL-U-150 (and says why one didn't fit) and PROTO-U-049 (a message's files).

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
