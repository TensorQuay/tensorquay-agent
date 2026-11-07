# Deterministic provider tests

`tqa-testkit` provides manual time, predictable jitter and an in-process model endpoint for tests and
benchmarks. It is development support and is never shipped. Its server binds an ephemeral loopback port;
tests need no external service, credentials or live response fixtures.

## Time and jitter

`FakeClock::new()` creates a stationary monotonic clock. Clones share time, and `advance(duration)` wakes
sleepers when their deadlines are reached. Overflow returns `InvalidInput` and preserves the clock value.
`FakeSleeper::new(clock)` implements the shared `Sleeper` trait. Clones share recorded durations; `durations()`
returns them in first-poll order. Unpolled sleeps record nothing. Zero sleeps complete on their first poll.
An unrepresentable deadline remains pending until dropped. Notifications avoid polling and real-time sleeps.

`FixedJitter::new(fraction)` repeats one fraction. `FixedJitter::sequence(values)` cycles through a nonempty
sequence. All fractions must be finite and in `[0, 1)`. Both implement the provider's `Jitter` trait without a
random-library dependency, making retry delays directly assertable.

## In-process provider

`FakeProvider::new(route, replies, sleeper)` implements `ModelProvider` directly without a server or background
task. Each `ProviderReply` delays the initial response and contains either a typed failure or a finite list of
`ProviderChunk` values. Each chunk has its own injected delay and result. Scripts do not add a finish marker;
ending without one models a cut. An error terminates its stream. An exhausted reply script returns a protocol error.

`requests()` records every received completion request in arrival order for exact mapping and replay assertions.
`active_streams()` counts owned streams; normal exhaustion and dropping release ownership. Cancellation interrupts
both initial-response and per-chunk latency. Use `FakeSleeper` with a manually advanced `FakeClock` for deterministic
latency tests. As with the HTTP fake, fixtures must contain synthetic data only.

## Scripted server

Start `FakeModelServer::start(scripts, sleeper).await` with FIFO `Script::new(events)` responses and use
`server.base_url()` as a route's API prefix. Each completion request consumes one script; an exhausted queue
returns 503. Scripts automatically finish with a terminal reason and `[DONE]` unless a control changes this.

The same scripts serve both protocols. `POST /v1/chat/completions` renders Chat chunks. `POST /v1/responses` renders a
Responses stream: `response.created`, one item per text, reasoning or call run, argument deltas with their `done`
events, and a terminal `response.completed`. `Length`, `ContentFilter` and `Other` finishes become
`response.incomplete` with that reason, and `Usage` goes into the terminal event. `RawJson` records keep their own
`type` as the `event:` line, which is how tests inject `error` events or unknown event types.

`POST /v1/messages` renders the same script as a Messages stream:
- `message_start` carries the input side of `Usage`: uncached input and `cache_read_input_tokens`, which sum to
  `prompt_tokens`.
- Each text or reasoning run is one `text` or `thinking` block. A thinking block closes with the synthetic
  signature `sig-<block index>`, so replay tests have a state to send back.
- Each `ToolCall` is one `tool_use` block with its `input_json_delta` fragments. The protocol never interleaves
  blocks, so explicitly indexed calls stream whole, in index order, at the end.
- `message_delta` carries `stop_reason` (`end_turn`, `tool_use`, `max_tokens`, `refusal` or the `Other` reason)
  and the output tokens, and `message_stop` ends the stream.
- The standalone `tqa-fake-model` serves the same route. It counts earlier responses as the assistant turns in
  `messages`, and every Messages response reports usage.

`tqa-fake-model --profile <name>` answers as a hosted provider's fake (TK-U-018): a request that breaks one of the
profile's rules, or uses another protocol's path, gets that provider's refusal before the scenario is consulted.
Names: `chat-strict`, `chat-compat`, `responses`, `responses-backend`, `messages`.

| `Scripted` entry | Behavior |
| --- | --- |
| `Text(text)` | Emit visible text |
| `Reasoning(text)` | Emit reasoning text |
| `ToolCall { name, arguments_json, split_into }` | Emit exactly that many UTF-8-safe argument fragments with an assigned call id |
| `ToolCallDelta { index, id, name, arguments_delta }` | Emit explicit indexed fragments, allowing interleaved parallel calls |
| `Usage(usage)` | Emit usage after the terminal finish record |
| `Status { code, body, retry_after }` | Replace streaming with an HTTP status/body and optional delay header |
| `CutAfter(count)` | End after `count` logical output events, omitting automatic finish, usage and done records |
| `MalformedSse(variant)` | Apply a framing variation to all output |
| `Delay(duration)` | Delay the next ordinary output record using the injected sleeper |
| `Finish(reason)` | Override the automatic terminal reason |
| `RawJson(text)` | Emit literal data-field contents for malformed-JSON tests |

`MalformedSse` supports `SplitUtf8` (one byte per fragment), `SplitData` (split the `data:` prefix), `CrLf`,
`Comments` (comment records and blank keep-alives), and `MissingDone`. These include valid SSE variations.
HTTP may coalesce writes, so provider decoder tests also exercise every byte split independently.

Set `script.header_delay` to delay response headers. Repeated `Delay` values accumulate until output; a
trailing delay applies to the automatic finish. Usage entries are deferred until after that finish and do not
consume pending delay. Tool output selects a tool-call finish reason unless subsequently overridden. The
last cut, framing or finish control wins; the first `Status` replaces streamed output. Invalid status values
become 500; invalid delay header values produce an explicit 500 diagnostic.

```rust
use std::sync::Arc;
use tqa_testkit::{FakeClock, FakeModelServer, FakeSleeper, Script, Scripted};

async fn example() -> std::io::Result<()> {
    let sleeper = Arc::new(FakeSleeper::new(FakeClock::new()));
    let server = FakeModelServer::start(
        vec![Script::new(vec![
            Scripted::Text("Checking the file.".into()),
            Scripted::ToolCall {
                name: "read_file".into(),
                arguments_json: r#"{"path":"README.md"}"#.into(),
                split_into: 5,
            },
        ])],
        sleeper,
    ).await?;
    // Configure a provider with server.base_url() and consume its completion stream.
    server.shutdown().await
}
```

`requests()` returns ordered snapshots: method, query-free path, sanitized headers/query, parsed body and
exact body bytes. Query values and unrecognized header values are masked; only ordinary HTTP content, host
and user-agent values survive after redaction. Synthetic request bodies remain intact for golden assertions;
never insert real credentials or user data. `wait_for_requests(count).await` uses notifications with a
five-second control deadline.

Discovery initially advertises `fake-model`. `set_models(Some(ids))` changes the list; `set_models(None)`
returns 404. Discovery does not consume completion scripts.

The fake bounds its queue to 128 scripts, script/event expansion to 16,384 records or fragments, request
bodies to 2 MiB, and captures to 1,024 requests. Invalid tool fragment counts fail during construction.
Request reads and explicit shutdown have five-second control deadlines. Scripted delays use the injected
sleeper and observe server cancellation. `shutdown().await` cancels delays and joins the server task;
dropping the server cancels and aborts it.

The fake gateway's relay serves `/relay/r/{route}/chat/completions`, `/relay/r/{route}/responses` and `/relay/r/{route}/messages`, and forwards each to the same operation on the scripted model server. `FakeGateway::send_text` sends one frame's text verbatim, for frames that do not parse, such as a route with an unknown protocol. It passes back `Content-Type` and `Retry-After`, as the real transparent relay does, so rate-limit handling is observable through the runtime.
Each recorded request also carries the `TQA-Protocol-Revision` header, when present, and `closes()` counts the
WebSocket close frames the runtime sent.

## Provider profiles

`FakeModelServer::start_as(profile, scripts, sleeper)` serves the same scripts but first checks each request against a
hosted provider's rules (`FakeProfile`). A request that breaks one gets that provider's status and error body, as JSON,
and consumes no script; a request on another protocol's path gets 404. Each `Rule` names its `Source`: the provider's
public API documentation, or a request captured in the field. The rules are deliberately independent of our own
request checks, so a contract test can't pass because both sides share a mistake. When the manual live provider suite
disagrees with a rule, the rule changes and names the live evidence. `FakeProfile::check(path, body)` runs the rules
without a server. The provider matrix (LLM-C-029..031) sends one request set through each adapter and the relay's
shaping to each profile. The rules' own tests are TK-U-016.

## Strict tools that lose line breaks

`FakeModelServer::start_with(FakeModelOptions { strict_line_breaks: Some(mode), .. }, scripts, sleeper)` models an
endpoint whose constrained decoding never emits a line break inside a strict tool's string argument
(docs/evals/gate-5.md, "Cause found"). Only the scripted calls of a tool the request sent with `strict: true` change
(Chat `function.strict`, Responses `strict`; Messages has no strict field and never changes). `Strip` removes the
line breaks from every string argument, nested ones too, which is what most strict calls looked like on the measured
server. Off by default; the tests are TK-U-019.

## Verification

```sh
cargo nextest run -p tqa-llm -p tqa-testkit
cargo llvm-cov nextest -p tqa-testkit
cargo doc -p tqa-testkit --no-deps
```

Provider integration tests exercise goldens, auth, tool round trips, fragmented framing, cuts, HTTP failures,
deadlines and cancellation. Fake-time unit tests verify shared wakeups and deterministic jitter independently.

On macOS, first execution of a new binary triggers system security scanning. Parallel enumeration of fresh
test binaries can queue that work ahead of the standalone model server, before its application code starts.
The nextest setup for the `standalone` target primes the exact Cargo artifact once: it reads the active
artifact search path, starts the server, waits for its ready line and bound port, then terminates and reaps it.
Setup has a bounded two-minute budget and fails the run on any error. Timed tests retain their five-second
ready-line deadline, with no polling sleep or retry. Native, coverage and custom target directories use the
same setup; tests reject a marker for a different executable. Linux CI does not have this macOS scan and
does not run the setup. Direct `cargo test` does not run nextest setup; use nextest for this suite on macOS.

## Tool support

`FakeTool` consumes a finite list of `FakeToolReply` values, records call arguments, waits through the supplied
context sleeper, and returns scripted text or a typed failure. Cancellation interrupts latency; an exhausted script
returns `Unsupported`. Prepared calls also record their frozen hash and context for call-binding assertions.
`workspace_fixture()` owns a disposable workspace and separate outside directory, with text,
binary, nested and git-protected files plus an outside symlink on Unix. Keep the fixture alive while its paths are in
use. Neither helper starts a shell or contacts the network.

## Public API client and E2E harness
`client::ApiClient` drives a real stack through the public HTTP API only (FR-26). It reads the API token from a file,
never from arguments or the environment, sends it as a bearer token, and never shows it in `Debug`, errors or results.
Every write carries a fresh `Idempotency-Key`. Downloads are bounded by `MAX_DOWNLOAD_BYTES`.

The `e2e` modules are the building blocks of the TEST-PLAN §16 task-set driver:
- `manifest`: a task directory's `task.toml`, validated against its bounds and the task's `task.md`, `seed/` and
  `check/run.sh` (TK-U-002).
- `integrity`: the replayed event log is contiguous from 1, and the stream matches it (TK-U-003).
- `classify`: one first-match rule table from the event log and the check verdict to `pass`, `harness.*` or `model.*`;
  anything unrecognised is a harness failure (TK-U-004).
- `result`: `run.json` with the schema version and the §15.1 environment, refused when incomplete, plus one JSON line
  per attempt; a run directory is never overwritten (TK-U-005). `environment` collects it from git, Docker and
  `/proc`; whether the route uses native tools is declared, because the public API does not expose it.
- `attempt` and `sse`: one attempt creates the conversation on a prepared workspace, sends the task, follows the event
  stream (reopening after the last sequence), cancels on a task budget or the
  driver deadline, then replays the log for the integrity check (TK-U-006).
- `workspace` and `docker`: the seed is copied into a new workspace by a container, because the storage belongs to
  the runtime user; the check runs on a copy downloaded through the files API, in a container without network, after
  protected paths are compared with the seed and restored (TK-U-007). The files API does not carry execute bits, so a
  check calls `sh` or `python3` explicitly, and a check that prints more than 64 KiB is reported as broken.

The standalone fake model normally picks a reply by the number of earlier model responses in the request. A compaction
shortens that history, so a scenario for a run that crosses one sets `"select": "arrival"` (replies in arrival order;
one conversation per server) and may set a `"summary"` reply, served to every request without tools, as the
compaction summary call is (TK-U-010). Two cautions: arrival order counts retries, since a retried step arrives again
and gets the next reply, so an arrival scenario should script no faults; and on a route without native tool calls
(`native_tools = false`) every request is tool-less, so use `summary` only with native-tools routes.

A reply may set `"wait_ms"` (0 to 600000) to hold its stream before the first record, sending an SSE comment every
15 seconds so no client idle timer fires (TK-U-035). The run stays active meanwhile, holding its admission slot and
workspace lease, so a stack test can keep a run going and end it with Stop, or make a slow first step. `delay_ms`
(at most 1000) is the pause between records.

With `"select": "prompt"` every reply names a `when` phrase, and a request gets the replies whose phrase appears in its
conversation's first user message, in order, by its turn (the last repeats). So several conversations on one stack
each follow their own script whatever the arrival order; a conversation no reply names gets `422` (TK-U-036).

The gateway's route check (Settings › Models › Check) is answered by the fake itself, before any selection: its fixed
conversation (the one tool `probe_echo` or `probe_lines`, or none on a route without native tools, and its fixed
message) gets the asked call with the check's own nonce, then a text answer. So a stack's check says Verified whatever
the scenario, and a check never uses up an arrival-order reply or gets `422` under prompt selection (TK-U-037).

A reply may end as a model server cuts one at its output limit: `"finish": "length"` sends the length stop on every
wire (Chat `length`, Messages `max_tokens`, Responses `incomplete` with `max_output_tokens`). This works on a text
reply, a reasoning-only reply, or one with a tool call whose `"cut": true` sends only the first half of its arguments
(TK-U-038). Under prompt selection, a reply with `"once": true` is served once per conversation (by its first user
message). After that, the next reply naming the same phrase answers the same turn, so a new message after a cut reply
gets an answer: the cut attempt leaves the history's turn count as it was (TK-U-039).

`"by_model"` maps a model name (other than the scenario's) to one fixed answer, given before the route check and any
scripted reply, so a route pointing at that name always gets it (TK-U-040). The answers:
- `{"status": 401, "body": "<the provider's error JSON>", "retry_after": "5"}`: any status from 400 to 599, with an
  optional Retry-After.
- `{"text": "..."}`: a text-only answer, even to a route check.
- `{"tool_call": {"arguments": "[1, 2]"}}`: a tool call whose arguments are sent as exactly these bytes, named after
  the request's first tool unless `name` is given.
- A text or tool call may add `"finish": "length"`.
`/v1/models` lists these names too. For an unreachable server, point a route at an address nothing listens on, such as
`http://127.0.0.1:9/v1`.

Test with `cargo nextest run -p tqa-testkit -E 'test(tk_u_)'`.
