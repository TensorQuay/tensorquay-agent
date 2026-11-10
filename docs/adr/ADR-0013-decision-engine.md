# ADR-0013: Decision engine seam: two consultation points, the `decision/v1` protocol, narrowing only

## Status

Accepted (2026-09-28), after a comparison with the loop as built; see "Decisions 2026-09-28".

## Date

2026-09-28

## Context

FR-27 asks for a decision engine that the loop consults before each step and after each model response. The default
engine lets the model decide. An external engine may narrow the tools, act without a model call, choose the route,
request compaction or stop. It can never widen the permission policy, it falls back visibly when it's unavailable, and
its effect is measured, never assumed (E2E-DECISION-008).

The loop already has the seam's first cut in `tqa-core`: the `DecisionEngine` trait with `before_step` and
`after_response`, the identity `ModelDecides` wired by the runtime, and the vocabulary `CallModel | Act | Compact | Stop`.
Only a `CallModel` on the conversation's own route with a valid tool subset is honoured, and an `after_response` plan
that selects a subset of the model's calls. Anything else falls back with a log line only. There is no event for a
decision or a fallback, and `Act`, `Compact` and `Stop` are reserved.

## Decision

1. **Two consultation points.** `before_step(state)` returns a decision once per step, before the step's first model
   attempt. The step's retries and a failover to a fallback route reuse that decision and never ask again.
   `after_response(state, response)` selects a subset of the model's calls by id. Dispatch keeps the model's order, and
   v1 doesn't reorder. Both hooks return a `Result`: a failure (`unreachable`, `timeout`, `invalid`) reaches the loop,
   which records it as a `decision.fallback` with that code. With `ModelDecides`, the loop's requests and events are
   byte-identical to the loop without an engine, and it emits no events (CORE-U-032).
2. **The four decisions, each within the step's limits.**
   - `CallModel { route, tool_subset }` calls `route` with `tool_subset` in registry order. `route` is the
     conversation's own route or one of its administrator-declared fallback routes (ADR-0025), never another. There is
     no separate route list for engines. `tool_subset` is a subset of the step's tools. The step's request is built for
     the chosen route: its context window and output limits, compaction measured against it, and opaque items from
     another protocol dropped, as on failover (ADR-0027).
   - `Act { tool, arguments }` makes a call the loop originates and runs through the same path as a model call:
     schema validation, the permission broker, approvals, the prepared-action hash and budgets. Only a tool in the
     step's set qualifies. An `Act` step counts as a step against `max_steps`. After three `Act` steps in a row, the
     next `Act` is refused with the fallback code `act_loop`, and that step calls the model. The next model request shows
     the action as a user-role note (the tool, its arguments and its result, stated as done by the system), never as a
     synthetic model call: providers refuse a tool result without a call of theirs, and a made-up call carries no
     signature (ADR-0027).
   - `Compact` runs compaction now (ADR-0009), even below the threshold, then asks again. A second `Compact` in the
     same step is refused with `repeated_compact`. A `Compact` with nothing old enough to fold is refused with
     `compact_nothing`.
   - `Stop { reason }` ends the run with `run.failed(code = decision_stop, reason)`, with the reason bounded to 200
     characters and no control characters. An engine can't declare a run completed in v1: success could hide
     unfinished work, so completion stays the model's own answer.
   - `after_response` calls the engine leaves out get an error result saying they were omitted, so the model sees
     what happened.
3. **Narrowing only.** Every decision is checked against the step's limits (the route set, the tool set, the argument
   schema) before it takes effect, and then passes the permission broker like a model call. An engine can only be more
   restrictive than the policy. It never sees credentials and never answers a permission request. An invalid decision
   is never partly applied: the whole step falls back to `ModelDecides`.
4. **Two events, which are also the replay record.**
   - `decision { point, engine, choice, latency_ms }` records the full applied decision, bounded like a tool call, and
     is journaled before it takes effect.
   - `decision.fallback { point, engine, reason }` has a closed reason code: `unreachable`, `timeout`, `invalid`,
     `route_not_allowed`, `tool_not_allowed`, `schema`, `repeated_compact`, `compact_nothing`, `act_loop` or
     `circuit_open`.
   - On resume, a step whose decision or fallback is journaled uses that record and never asks the engine again, so a
     resumed run takes exactly the decision it took (as ADR-0027 does for opaque state).
   - Both events go from the runtime to the gateway. The gateway's event and error vocabularies are closed, so an
     older gateway can't parse them or the new `decision_stop` code. They are part of the one protocol revision in
     point 7.
   - The circuit breaker's state is derived from the journaled `decision.fallback` events, in `tqa-core`, so a resumed
     run keeps it. The runtime adapter only reports `unreachable` or `timeout`.
5. **The `decision/v1` protocol,** HTTP with JSON, versioned and documented like the wire protocol.
   - **Transport:** the runtime POSTs to the gateway relay (`/relay/d/<engine>/v1/before_step` and
     `/v1/after_response`), and the gateway adds the engine's credential from the secret store and forwards, as for a
     model route. The runtime never holds the engine's credential or address.
   - **Request:** bounded, without conversation contents by default: the run id, step and attempt; the allowed routes
     and their capabilities; the step's tools (name, description, parameter schema); budget use against each limit;
     the previous step's calls with names and result status; and, for `after_response`, the model's calls. History
     contents are an exposure setting per engine (`none`, `calls` or `full`), `none` by default. With `none`, the
     model's calls go by id and name only: their arguments are transcript content, so they're left out (SEC-036).
     With `calls` and `full`, the arguments go too. Only an administrator may set another value, and the change is
     audited.
   - **Response:** exactly one decision object, strictly parsed; an unknown field or value is `invalid`. At most
     64 KiB.
   - **Bounds:** a per-engine timeout (2 s by default, at most 10 s) that counts inside the run's wall clock, and a
     circuit breaker: after three consecutive fallbacks in a run, the loop stops asking for the rest of that run and
     records one `circuit_open` fallback.
   - **Types** live in `tqa_types::decision::v1`, with an exported JSON Schema, and are shared by the runtime adapter,
     the test fake and the conformance suite.
6. **Where the pieces live.**
   - `tqa-core`: applying and validating decisions, the events and replay. It stays free of network code.
   - `tqa-runtime`: the external adapter over the relay, with the timeout and the circuit breaker.
   - `tqa-gateway`: an engine is a record of its own, never a model protocol, so no model path can reach it (amended
     2026-09-28, see below). `[[decision_engines]]` in `gateway.toml` seeds it and the admin API edits it, in the
     `decision_engines` table under the ADR-0028 rule; its `key_ref` names a secret the engine must be allowed (vault
     D8), read as the `engine` consumer and audited. The relay path is `/relay/d/<engine>/v1/<point>`. A conversation
     uses one through its agent profile's `decision_engine` field; the install default is none.
   - `tqa-testkit`: a fake engine, both in process and as a `decision/v1` server with scripts, delays and malformed
     replies.
   - `tools/ext-check decision`: the conformance suite an engine author runs (valid decisions, invalid decisions,
     unavailability, permission narrowing), as ARCHITECTURE §15 describes.
7. **The protocol revision.** One revision covers:
   - the session configuration's `decision_engine` field (the relay path and bounds), gateway → runtime;
   - the `decision_stop` error code and the `decision` and `decision.fallback` events, runtime → gateway.

   The revision (9) is raised in the same commit that makes the gateway send the field (ADR-0026). The floor stays 8
   (amended 2026-09-28, ADR-0026's note): the gateway sends the field only to a runtime announcing 9, and refuses an
   older runtime for a conversation whose profile names an engine with `runtime_too_old`. The runtime consults an
   engine only when `session.config` names one; otherwise the model decides. The snapshot may name the `points` the
   engine answers; absent means both, as for every `decision_v1` engine. At a point it doesn't answer, the loop never
   asks it and journals nothing, so an engine that only routes (`before_step`) costs no request and no event after each
   response.
8. **Text-protocol routes.** On a route without native tool calls, `after_response` sees the calls parsed from the
   text, with their `fb-` ids (ADR-0007).
9. **The measurement (E2E-DECISION-008).**
   - **Engine:** a small reference engine (`examples/decision-reference`, under an Apache-2.0-compatible licence) that
     routes routine steps to a cheaper declared route and the others to the default route. This measures cost at equal
     quality, which is why the seam exists. A tool-narrowing policy may follow as a second arm.
   - **Design:** the E2E task set, each task three times with `ModelDecides` and three times with the engine, the same
     model family, route set, images and harness, interleaved per task.
   - **Measures:** completion (paired, exact McNemar test, Wilson intervals); tokens, steps and wall time per attempt
     (paired differences with bootstrap 95 % intervals); the engine's latency and share of wall time; the fallback
     rate; and how often each decision kind was used.
   - **Rule:** no saving is claimed unless its interval excludes zero, and a completion drop beyond its interval is a
     regression whatever the saving.

## Decisions 2026-09-28

Decisions from comparing this ADR with the loop as built, applied in the points above:
1. The engine is consulted once per step, never per attempt; the step's retries and failover reuse its decision.
2. `after_response` selects and doesn't reorder in v1.
3. One protocol revision covers the session-configuration field, `decision_stop` and the two events (the floor was
   later left at 8; see point 7).
4. With exposure `none`, the model's calls go by name and id only, their arguments left out.
5. An `Act` reaches the model as a user-role note, never as a synthetic call, with the three-in-a-row cap.
6. Both hooks return a `Result`, so a failure's reason reaches the `decision.fallback` event.

## Amendment 2026-09-28: the engine record and its relay

Decisions for the gateway half (point 6), replacing "an engine is a route kind":
1. **Its own record.** An engine has a name, a `url`, `auth` and `key_ref` like a connector's, a `timeout_ms` (2000 by
   default, at most 10000), an `exposure` (`none` by default) and `enabled`; `insecure_http` and `ca_file` are
   file-only. The file seeds at most 16. Engines are never in `models.toml` and never a route, so no model request
   can reach one.
2. **The relay checks what the runtime could bypass.** A tool in the sandbox can read the runtime token, so the
   gateway itself refuses an engine the conversation's profile doesn't name (`403`), a request carrying what the
   engine's exposure excludes (`400`), including a call name that isn't a tool name, and a body over 64 KiB (`413`).
   The runtime leaves out a call whose name the model made up. The tools' descriptions and parameters still reach
   the engine under every exposure, a channel bounded by the 64 KiB request; the engine is the administrator's own,
   so this is accepted. It bounds the answer to 64 KiB, forwards within
   `timeout_ms`, answers a failed or oversized reply with `502` and a late one with `504`, and passes back only the
   content type.
3. **Metered, not budgeted.** Each consultation counts against the owner's relay rate and is recorded as a usage row
   under `decision:<name>`; v1.0 enforces no budget on it. (The adapter amendment below adds an optional budget.)
4. **Refused, never skipped.** A message to a conversation whose profile names an engine the installation no longer
   has, or has disabled, is refused with `409 decision_engine_unavailable`. If the engine goes after a message was
   accepted, the link refuses the sandbox's runtime and retires the sandbox without a replacement, revoking its token
   so the runtime dials at most once more; the owner gets a retryable `decision_engine_unavailable` notice and the
   sandbox shows as failed. The run stays unfinished, admission leaves the task alone while the engine is missing,
   and a message after the engine is restored resumes it. Work still queued for a slot when the engine goes fails
   once with `decision_engine_unavailable`, so no task shows as queued for ever. The profile's own record keeps the name;
   the runtime's snapshot carries no copy of the engine.
5. **Slices.** The record, seed, relay and profile field came first; the session-configuration field and protocol
   revision next (point 7); then the admin API (`/api/v1/admin/decision/engines`), its settings page and the audited
   exposure, before v1.0.
6. **The admin API.** Writes follow the ADR-0028 pattern (versioned, idempotent, audited, file-only fields refused, at
   most 16 engines); `key_ref` must name a shared secret that allows the engine. Every change of an engine's exposure
   is its own `exposure` audit row with the old and new exposure, since the exposure decides how much of each
   conversation reaches a third party and a replace row can't show it afterwards. An engine that a profile or an
   active conversation names can't be deleted (`409 decision_engine_in_use`); disabling it is the soft path. A live
   session keeps the exposure it started with: after a narrowing, its runtime still sends what is now excluded, the
   relay refuses it, and consultations fall back to the model until the conversation's next session; a widening
   applies from the next session.

## Amendment draft 2026-09-29: the external typed-question adapter and the engine budget (Phase 9 item 1)

**Status: accepted on 29 Sep 2026; built.**
This covers two things: an adapter that lets an installation use an outside service which answers typed questions
(multiple choice with a confidence) as its decision engine, and an optional budget per engine.

1. **An engine kind of its own.** `protocol = "typed_questions_v1"` is a second engine protocol, next to
   `decision_v1`. It is configured like any engine (`[[decision_engines]]`, the admin API and the settings page),
   plus:
   - `model`: the service's model identifier, **required**: the service's API refuses a request without one, and a
     default the gateway picked could change under an installation. Pinned, so a measurement never moves mid-run;
   - `min_confidence`: 0 to 1, 0.6 by default;
   - optional one-line `route_hints` per route, which the question offers as each option's description (the route's
     name otherwise).
   An engine of this kind answers `before_step` only: its snapshot names `points = ["before_step"]`, so the runtime
   never asks it after a response. The runtime is unchanged and there is **no protocol revision**.

   On the settings page (UI-GUIDELINES §0, "it asks for little"), adding an engine starts with its kind as a first
   step, in plain names: "Your own engine" or "A multiple-choice service" ("typed-question" stays an internal term).
   Beside the display name, this kind then shows four fields: its address, its key, its **Model** (a plain field
   under the key, since there is no default to fall back on) and what it sees. The budget (point 5) is visible, not
   under More options, because it is what an administrator looks for on a paid service: one line, "No budget · Set a
   budget", that opens a row of calls, tokens and period. `min_confidence` and `route_hints` sit under More options
   with their defaults (29 Sep).
2. **Translated in the gateway.** The relay (`/relay/d/<engine>/v1/before_step`) runs every existing check first:
   the binding, the exposure refusal, the 64 KiB bound, the owner's relay rate and the key's audited read. For this
   kind it then:
   - builds the question's `state` from the `decision/v1` request: the run, step and attempt, budget use against
     each limit, the previous step's calls by name and status, and the step's tool names. Arguments are added only
     under exposure `calls`, the response text and transcript only under `full`. The exposure refusal already stopped
     anything more, so the state can't carry it. The state is at most 16 KiB of JSON, refused before sending;
   - asks one question, `route`, a single choice among the conversation's own route and its declared fallbacks, each
     option described by its hint: "the cheaper route for a routine step (reading, looking something up, a small
     change), the default route otherwise";
   - answers the runtime with a `decision/v1` response it builds itself: `call_model` on the chosen route with every
     step tool. A choice below `min_confidence` answers `call_model` on the conversation's own route: the conservative
     choice, recorded as a normal `decision`. A choice outside the options is `invalid`; it can't happen by
     construction, and is still checked.
   `act`, `compact` and `stop` are never produced by this adapter.
3. **The key.** It stays in the secret store as the engine's `key_ref`. The installation's administrator enters it
   (never a user's own secret), the secret must allow the engine, and every read is audited with the engine as the
   consumer. Neither the runtime nor any person reads it back. Each installation brings its own key and pays its own
   bill.
4. **Usage rows.** Each consultation's usage row under `decision:<name>` also records the tokens the service reports
   for the call. The Usage page then shows calls and tokens per engine.
5. **The budget, an administrator's setting, off by default.**
   - **What it is:** per engine, an optional `budget = { calls, tokens }`, either or both, and a period: `total`, or
     `month` (a calendar month in UTC). `tokens` counts what the service reports for each call, input and output
     together: one number for the administrator, and the one the bill counts. It is audited when changed, like the
     exposure: each change is its own audit action, `budget`, with the old and new budget (migration 0058). `total`
     counts from when the budget was set or last changed, so saving a new budget starts a new count and saving the
     engine with the same budget keeps it; `month` counts the whole calendar month whatever changed in it, since the
     month is the bill's period and raising the cap only raises the ceiling (decided on 29 Sep 2026). It applies to
     every engine kind: a `decision_v1` engine counts calls only.
   - **Where it is counted:** from the engine's usage rows since the period began (calls = rows; tokens = their sum),
     in the gateway relay, which holds the key, so nothing can call around it. The counters survive restarts because
     they are the usage rows.
   - **Concurrency:** the relay checks the count before a call and the row is written when the call is made, so
     consultations running in parallel can pass the same last unit. The overshoot is at most the calls already in
     flight when the budget runs out; with tokens, at most those calls' tokens. That is the rule, stated in the
     admin guide, rather than an atomic reservation (which would hold a row open across a call of up to
     `timeout_ms`).
   - **When it is spent:** a consultation that would pass it is refused in the relay before any call leaves the
     gateway (409 `decision_engine_budget_spent`). The runtime records it as its own fallback reason, `budget_spent`
     (revision 9 isn't released, so the reason joins it), once: the agent decides alone and the run stops asking the
     engine, with no `circuit_open`, so a spent budget costs no further requests. A new run asks again, so a raised
     budget takes effect then. That event is the durable record in the transcript. The task's owner also gets one notice
     per task per period, `decision_engine_budget_spent` (retryable: false; "this task's decision engine has used its
     budget; the agent decides on its own"). Administrators see the budget's use on the engine's settings row.
   - **The notice's code** is gateway-to-client only, like `decision_engine_unavailable`, so no raised floor (the
     runtime reads the same code from the relay's 409, which is not a frame). Revision 9 isn't released yet, so
     `decision_engine_budget_spent` and the `budget_spent` fallback reason join revision 9: no new row and no new
     revision (decided on 29 Sep 2026).
6. **Failure and timeout.** The engine's `timeout_ms` (2 s by default, at most 10 s) bounds the whole service call.
   There are no retries inside a step: a retry would spend the step's latency and double the cost, and the circuit
   breaker handles persistent failure.

   | Service outcome | Relay answer | `decision.fallback` |
   |---|---|---|
   | 200 with a well-formed answer | 200, a `decision/v1` response | none (a `decision` event) |
   | 200 but malformed, or 422 (our request was wrong) | 502 `decision_engine_invalid`, with a WARN naming the class | `invalid` |
   | 401 or 403 (the key) | 502 `upstream_unavailable`, with a WARN naming the key's source, never its value | `unreachable` |
   | 429, 529, 5xx or a connection error | 502 `upstream_unavailable` | `unreachable` |
   | no answer within `timeout_ms` | 504 `timeout` | `timeout` |
   | the state over 16 KiB | 502 `decision_engine_invalid`, before any call, and not counted against the budget | `invalid` |
   | the budget spent | 409 `decision_engine_budget_spent`, before any call | `budget_spent`, then not asked again that run |

   The runtime maps a 502 with `error.code = decision_engine_invalid` to `Invalid`, and a 409 with
   `decision_engine_budget_spent` to `BudgetSpent`; neither is a wire field. Every other non-2xx stays `Unreachable`.
7. **Tests, all against a fake: no live call, and the key is never read.**
   - A fake typed-question server in tqa-testkit (TK-U): scripted answers, confidences, statuses, delays and
     malformed bodies.
   - The translation (GW-I): the request shape (one `choice` question over exactly the allowed routes, no arguments
     or text under exposure `none`); the answer mapping, including `min_confidence`; each row of the table above; the
     usage row's tokens.
   - The budget (GW-I): unlimited when unset; refused before sending at each limit; the notice once per task and
     period; the change audited; the count unchanged across a restart; a `decision_v1` engine counting calls; the
     month rolling over at 00:00 UTC on the 1st; and N consultations in parallel at the last unit leaving an
     overshoot no larger than N calls (or their tokens).
   - The runtime (RT-U): a 502 `decision_engine_invalid` is `Invalid`; any other 502 stays `Unreachable`.
   - The engine kind's config validation (GW-U), and SEC-036's case for this kind: the service request under
     exposure `none` carries no argument or transcript text.
   Test IDs are claimed in the team's claim list when coding starts.
8. **The with-versus-without measurement (E2E-DECISION-008), as point 9 describes, made concrete.**
   - **Tasks:** the E2E task set, 20 tasks.
   - **Arms:** `ModelDecides`, and the typed-question engine choosing between two routes. Both are local, on the
     shared GPU's model, so the model side spends no paid quota and only the engine calls cost money:
     - the conversation's route, `local-32k` (the default route for full steps);
     - the declared fallback `local-text`, the cheaper route (thinking off, text protocol) for routine steps.
     Same model, route set, images and harness; each task three times per arm, interleaved per task: 120 attempts.
   - **Metrics:** completion (paired, exact McNemar test, Wilson intervals); tokens, steps and wall time per attempt
     (paired differences, bootstrap 95 % intervals); the engine's latency and its share of wall time; the fallback
     rate; how often each route was chosen. No saving is claimed unless its interval excludes zero, and a completion
     drop beyond its interval is a regression whatever the saving.
   - **GPU time:** about 50 to 60 minutes of one window at yesterday's rate (30 attempts in about 12 minutes).
   - **Cost:** the engine arm makes about 600 to 720 calls of about 1,000 input tokens: about 0.7 million tokens,
     about USD 0.03 at the service's list price at the time of writing (about USD 0.04 per million input tokens). The
     `ModelDecides` arm makes no calls.
   - **Caps, far below USD 50:**
     - the engine's budget for the measurement install: 3,000 calls and 30 million tokens in total, about USD 1.30;
     - the harness's `--max-engine-calls 1500`, about twice the expected calls, which stops new attempts;
     - the 16 KiB state bound per call.
     The worst case through every cap is the budget, about USD 1.30.
   - **Before the measurement:** one live smoke of at most 20 calls measures the service's latency and real token
     counts (about USD 0.001). It runs only after the technical lead's go and the product owner's OK. The key is
     imported by the lead into the secret store with a no-print script; the developer never reads it.

## Consequences

- Anyone can build an engine in any language against `decision/v1` and prove it with `ext-check decision`.
- With the default engine the loop is unchanged, byte for byte.
- One protocol revision, in the commit that sends the engine to the runtime. It covers the new events and error
  code too. Only conversations with an engine need a revision-9 runtime.
- An `Act` is shown to the model as a note, so the model sees what the system did without a call it never made.
- An engine's latency counts against the run's wall clock; the timeout and the circuit breaker bound it.
- Completion stays the model's decision in v1; an engine can only stop a run as failed.
- Implementation starts with Phase 9; until then the first cut in `tqa-core` stays as it is.

## Links

PRD FR-27; ARCHITECTURE §5.1a, §15 and §18; ADR-0009 (compaction), ADR-0025 (fallback routes), ADR-0026 (runtime
protocol revision), ADR-0027 (replay of opaque state); TEST-PLAN CORE-U-032, CORE-U-033, CORE-S-034, CORE-E-035,
CORE-U-058, CORE-U-059, CORE-U-060, CORE-E-039, PROTO-U-021, PROTO-U-022, RT-I-031, GW-I-105, TK-U-021, TK-I-004,
SEC-036, E2E-DECISION-008.
