# Responsibility

A host-independent native-tool agent loop. The runtime injects the provider, registry, decision
engine, event sink, clock, sleeper, workspace boundary and explicit limits.

# Contract

`Runner::new(RunnerDeps)` validates programmatic settings. `Runner::run(history, message, cancel,
shutdown)` derives recovery from the journal and returns `Idle`, `Completed`, `Cancelled`,
`Interrupted` or a typed `Failed` outcome. An invalid journal, busy conversation, invalid input or
failed sink returns `CoreError` instead. The supervisor serializes calls for a conversation.

`fail_before_run` records a classified initialization failure before any model step. It validates the
pending turn, journals `message.user`, `run.started` and `run.failed` through the same sink, and uses
the same message-sequence run identifier. It resumes either interrupted start append without
duplicating a turn or run. An idle journal without a message stays unchanged. An already started model
step is rejected as busy; this entry point cannot bypass tool recovery. Reasons are fixed,
bounded status text. No provider, tools or host services are needed to record the failure.

The sink assigns sequences, persists and delivers events. The loop retains local projection frames
with the same envelope and `LOCAL_PROJECTION_TIMESTAMP`; they never leave this crate. A new
`message.user` is followed immediately by `run.started` for `r-<message sequence>`, including when
user cancellation was already requested. Shutdown can interrupt this boundary; recovery starts the
unanswered message's deterministic run. Sink writes are never raced against either stop token.
A sink failure stops all further writes. The 200 ms cancellation-reflection guarantee assumes a
healthy sink with bounded append latency.

User cancellation closes every unanswered current call, pairs executed calls with `tool.end`, and
appends `run.cancelled`. Shutdown stops further writes and returns `Interrupted`, preserving the
journal for recovery. A write already in progress still completes. Both paths cancel and join owned
tool and spill workers before returning; cleanup can finish after the terminal event. The caller
must signal a stop token and await the loop instead of dropping its future.

Recovery preserves finished calls, reports started unfinished calls as interrupted, dispatches calls
that never started, and retries partial model attempts with a higher attempt
number. The highest attempt is authoritative. A completed step without calls completes its run
without another model request. Ended runs' dangling calls are answered before another user message.

# Model and tool behavior

Every model attempt has its own `step.started`. The loop uses `StreamAssembler` with the provider
crate's recommended stream bounds. Visible and reasoning deltas are emitted as received; completed
calls are emitted in stream-index order, followed by reported usage and `model.completed`.
Only object arguments are dispatchable. Invalid JSON and other JSON values are preserved as raw
string arguments and receive corrective tool results.

Transient errors allow three attempts per step per invocation, with a 2 second backoff base,
30 second cap and injected equal jitter. Recovery continues the durable attempt number while using
its own three-attempt retry budget. `provider.retry` contains only the error code and optional HTTP
status. An empty or reasoning-only response gets one retry within that same budget. Its usage is
retained, but it is not marked complete or projected as an answer. Non-retryable provider errors fail
immediately.

Requests use the stable system, instruction and sorted tool slots first, then `tqa_context::project`
in call order. Missing route capacities use `DEFAULT_CONTEXT_WINDOW` (32768) and
`DEFAULT_OUTPUT_RESERVE` (4096); a guessed window has the context crate's conservative reduction.
Context overflow fails without dropping history. Provider messages preserve visible text and tool
calls; reasoning remains separate in the journal and is not added to chat history.

`ModelDecides` passes every model call through. Valid decision hooks may narrow the registered tool
set and select original calls by identifier. Unsupported decisions and invalid plans fall back to
model decisions. Selected calls retain model order. Dropped calls still receive error results.

The dispatch order is object parsing, registry lookup and schema validation, the call's frozen identity,
execution. The loop doesn't access the filesystem; tools enforce workspace and
protected-path confinement when using paths. Calls that never execute get only a result.
Consecutive parallel-safe calls form a concurrent batch; other calls run alone, preserving every
read/write dependency between batches. Within a batch, results are emitted in completion order;
projection restores call order for the next request. Output uses protocol chunks, and authoritative
results are capped and spilled through an owned blocking worker. The cap is the smaller of `tool_result_bytes` and
half of the input capacity still free when the result is recorded, never below `MIN_RESULT_BYTES` (1 KiB). The
step's still-unanswered calls count as empty results in that measurement, so a result larger than the window, or
several parallel ones, is cut with its head, tail and spill path, and the run continues (CTX-E-004). Tool failures use their bounded
`model_message` diagnostics. The third matching error result ends a run; cancelled and interrupted
results do not count. For identical in-flight calls, dispatch reserves room for their possible
error results so a fourth execution cannot race ahead of the third matching failure.

# Budgets and limits

Completed steps persist `duration_ms`. Active elapsed time is their sum plus time spent in the current
step during this invocation, including retries and backoff. Recovery preserves completed time and
does not count down time; at most the pre-crash part of one step is lost. Warnings at
80 percent are recorded at step boundaries and deduplicated by run history. Hard step limits are
checked after step completion and again before recovery starts another step. A remaining-time
deadline interrupts waits and closes open calls before `run.failed` with code `budget`.

Output tokens are charged across all attempts, including failed attempts, from journalled usage.
Requests reserve at most the remaining run budget. Until provider usage arrives, streaming enforcement
uses the context byte estimator; missing usage is recorded with `estimated: true`. Recovery estimates
an unfinished attempt from its journalled deltas when usage is absent. The output warning occurs once
at 80 percent; exhaustion uses `budget.output_tokens`, and active-time exhaustion uses
`budget.wall_clock`. A final answer exactly at the output ceiling succeeds. A length finish stops the
run, and exhausted output never permits another step.

Progress means new capped tool-result content compared with every earlier completed step in the run.
Six consecutive steps without it emit one `no_progress` warning; twelve fail with `no_progress`.
The first new result makes progress. New content resets the counter, and recovery preserves it.
Steps consisting only of identical errors are left to the repeated-failure rule. The projection
turns the warning into a fixed nudge. Shell side effects with byte-identical output are not detected.

On a route with `vision`, each request of a run adds the images attached to that run's own user message, after
its text and in attachment order. Earlier runs' images stay paths. At most `MAX_IMAGES_PER_MESSAGE` (4) are sent.
Each is read confined and bounded at every request, and the images must fit the route's `request_budget()`
exactly, measured with the adapter's encoder. Each image counts `IMAGE_TOKENS` (1600) in the window check and in
the compaction trigger. An image that is too large, unreadable, over the limit or over the request budget is not
sent. One fixed line follows the user text instead:
`[Images not sent, data only: "/workspace/uploads/a.png": too large; ...]`. A file that changes between requests
never fails the run. No event records images: the journal keeps only the paths.

Before a step's first attempt the loop compacts when the request would need more than
`COMPACT_AT_PERCENT` (85) of the input capacity, and the history has turns to replace. One summary call uses
the fixed prompt, no tools and `COMPACTION_OUTPUT_TOKENS` (1024). Its deltas are not streamed, and its usage is
recorded and counts toward the run's output budget. A usable reply is one JSON object within the summary bounds,
optionally inside one Markdown code fence. A retryable transport failure (the step classification, with the server's
Retry-After) is retried up to `SUMMARY_RETRY_ATTEMPTS` (2) times within the run's wall clock. A relay throttle is
waited out without spending an attempt, as for a step, with the same coalesced `provider.retry` events, so the wait
is visible; it switches no route. Any other failure, or a reply that does not parse or breaks the bounds, uses the
deterministic fallback at once. The `compaction` event records the source, and the runtime log gets one warning with
a bounded reason code (the provider error's code, such as `rate_limited`, `stream_cut` or `http 413`, or `parse` or
`bounds`), never any of the reply's text or an error body. Cancellation, shutdown or the wall clock during the call
write no event, so recovery compacts again. If the compacted request still does not fit, the run fails with
`budget`.

On a route with `native_tools` false the loop uses the text protocol: the request carries no native tool
definitions, the system slot gains the fallback instructions, and history is sent back as text (calls as
`tool_call` blocks after the assistant's text, results as `[Tool result for call <id>, data only: "..."]` user
messages). Only prose is streamed: text from the first line that could open a fence, or a reply that may be a bare
call object, waits for the finished reply. Parsed calls become ordinary `tool.call` events with identifiers
`fb-<step>-<attempt>-<n>`, so the schema, prepared-action hashes and budgets apply exactly as
for native calls; the output budget counts the hidden call text. An unreadable reply records `provider.retry` with
reason `fallback_parse: <code>`; the projection turns that into one fixed retry prompt, and a second unreadable
reply in the same step fails the run with `fallback_parse`.

Fallback routes (ADR-0025): `RunnerDeps::fallback_routes` lists up to three `RouteProvider`s. A run switches to
the next one when the active route spends its retry budget on availability errors (408, 429, 5xx, timeouts,
connections, cut streams), or at once when a 429 or 503 asks to wait more than 60 s; other errors never switch. The
switch is the event `route.fallback { from, to, reason }`; the step continues with a new attempt, and the rest of the
run stays on that route, while the next run starts on its own route again. Every request uses the active route's
window, output reserve, reasoning, request budget, tool-call mode (the text protocol without native tools) and image
input (the fixed `no image input on this route` notice without vision), and the window is re-checked, with
compaction if needed, before the first request on the new route. Budgets are per run, so a switch never resets them.
A resumed run restores its route from the journal; a recorded fallback that is no longer configured returns it to its
own route with a `route.fallback` notice (reason `route_unavailable`).

Opaque reasoning state (ADR-0027): a completed response's `Completion::reasoning_state` items are journaled as
`reasoning.state` events, in stream order and before the response's `tool.call` events.
- **Bounds:** 16 items and 512 KiB per step, 128 KiB per item, and an item whose escaped form must fit one frame.
- **Over a bound:** the event keeps `bytes` and drops `item`.
- **Replay:** the request builder puts an assistant message's kept items first in its content, as
  `ContentPart::ReasoningState`, only when their route, protocol and model equal the active route's. A fallback
  never receives another route's state, and a run back on its own route reuses it.

No host discovery, network construction or process
creation occurs in this crate; tools and the provider supply those boundaries.

# Calls run (ADR-0035)

Every call the model makes runs: there is no broker, rule, approval or grant. The runner freezes each validated
call with its conversation, run, step, attempt and call identifiers, and its canonical digest. A call that can't be frozen (too large, too deep) is answered with an error
result and the run continues. A started interrupted push is never repeated by recovery; its result says the outcome
is unknown and asks the user to check the remote before trying again.

A history whose last frame is the retired `run.parked` marker or an unanswered retired `permission.request` (a run
waiting for an approval when ADR-0035 came) is failed with `approvals_removed` and its waiting call closed as
cancelled, never run (CORE-U-038).

# How to test

```sh
cargo nextest run -p tqa-core
cargo llvm-cov nextest -p tqa-core --fail-under-lines 90
```

The tests use finite provider scripts, an append-only sink and injected clocks. They cover exact
event goldens, all recovery states, every scripted cancellation and shutdown boundary, deterministic
requests, batching, retry limits, actionable errors and joined cleanup. One Unix shell test checks
the real tool boundary under shutdown.

Model connections refused, reset or closed before response headers share the bounded retry path with stream cuts.
Three total attempts use equal jitter with a two-second base: waits of 1–2 and 2–4 seconds keep the retry window open
through a brief relay restart. The thirty-second per-delay cap and cancellation remain enforced. Each attempt has its
own journal identity; a later successful attempt supersedes partial text in the projection without executing tools
from the discarded attempt.
