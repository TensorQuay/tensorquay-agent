# Model providers

`tqa-llm` supplies the provider boundary described in [ARCHITECTURE](../../docs/ARCHITECTURE.md), sections 3 and 6.
The caller selects a route, resolves its credential separately, and receives typed streaming chunks. Each
adapter call performs one HTTP attempt; the caller controls retries and cancellation.

## Route contract

`Registry::from_toml` accepts exact TOML, rejects unknown fields, and validates every route before lookup.
Names are unique; `get` looks up a name, `names` iterates in sorted order, and `to_toml` exports a stable registry.
Input is limited to `MAX_REGISTRY_BYTES` (1 MiB). Errors expose a stable code and one-based line/column when a
source location is available.

`Registry::from_configs` builds the same registry from `RouteConfig` records, for example routes stored as JSON in
a database. It applies the one validator the file uses. When a route rule fails, `RegistryError::route` names the
route and `RegistryError::reason` gives a typed `RouteInvalid`, whose `code()` is stable for APIs:
`route_name_invalid`, `model_empty`, `base_url_invalid`, `key_ref_required`, `auth_header_invalid`,
`auth_query_invalid`, `reasoning_invalid`, `budget_invalid`, `max_output_required`, `limit_out_of_range`,
`extra_body_reserved`, `extra_header_invalid`, `extra_header_reserved`, `extra_header_credential`,
`quirk_unsupported` and `fallback_invalid`. The file loader's messages are unchanged: "route <name>: <reason>",
with the line.

A route may name its `display_name`, the name people see (LLM-U-051). The relay never reads it, and `Registry` keeps
it beside the routes (`Registry::display_name`); unset, it is never written, so a route's record is unchanged.

Two optional per-route bounds sit next to `context_window` and `max_output` (flat keys, stored in `Limits`):
`concurrency`, the most requests in flight on the route, streams included; and `requests_per_minute`, the most
starts per rolling minute. Absent means unlimited. `RouteLimiter` enforces them (LLM-U-012):
- requests are served in arrival order;
- a request waits at most `QueueBounds::wait`, and at most `QueueBounds::max_waiting` requests wait at once;
- a refusal (`LimiterRefusal`) carries a `Retry-After`, and cancellation frees the place at once;
- a start the rolling window cannot allow before the wait bound is refused immediately, with the time until it could.

The gateway relay holds one limiter per route, so the bound covers every conversation and user; the runtime doesn't
limit.

Declared limits must be positive, and `max_output` must fit `context_window`. That's the rule the route snapshot
enforces, so a route that loads is never refused later when a conversation starts.

```toml
[[routes]]
name = "local"
protocol = "openai_chat"
base_url = "https://models.example/v1"
model = "example-model"
auth = { kind = "none" }
context_window = 32768
max_output = 4096
native_tools = true
parallel_tools = true
vision = false
stream_usage = true  # the default; false sends Chat requests without stream_options, for a server that refuses them
cache_hint = "prefix"
reasoning = { field = "reasoning_effort", allowed = ["low", "high"], default = "low" }
extra_headers = { x-feature = "enabled" }
extra_body = { template_options = { thinking = true } }
quirks = []
```

`Route.limits` groups optional token limits in Rust; TOML keeps `context_window` and `max_output` flat.
`Limits::derived()` is true only when `context_window` is configured. `native_tools` defaults to true; the other
capability booleans default to false. `ContentPart` supports text only, including on routes declaring vision.
Cache hints are capability data; this adapter uses stable JSON ordering for prefix reuse and adds no explicit
cache-control blocks. No quirks are implemented, so nonempty `quirks` fails validation.

| `protocol` | Provider construction |
| --- | --- |
| `openai_chat` | Streaming chat completions |
| `openai_responses` | Streaming item-based responses, stateless (see below) |
| `anthropic_messages` | Streaming message-block completions, stateless (see below) |

`auth` selects exactly one placement: `none`, `bearer`, `header` with `name`, or `query` with `name`. Every
authenticated route requires `key_ref`, a secret-store reference rather than a credential. Pass its resolved
`SecretString` to `provider_for`; missing or empty credentials on authenticated routes fail. Base URLs must
use HTTP(S), without credentials, queries or fragments. Version paths belong in `base_url`; endpoint joining
accepts a trailing slash. Redirects, automatic HTTP-client retries and environment proxies are disabled.
A relay uses the same adapter with a different endpoint and credential.

Reasoning defaults must occur in the route's allowed values. Requests may select another allowed value;
without a reasoning mapping, their effort setting is omitted. `extra_body` cannot override messages, tools,
model, stream settings, tool choice, output limits, temperature, stop sequences, metadata, parallel tool
settings, `reasoning_effort` or the configured reasoning field. On a Responses route it cannot override `input`,
`instructions`, `store`, `previous_response_id`, `conversation`, `include`, tools, tool choice, parallel tool
settings, `max_output_tokens`, temperature or metadata, and it may add to `reasoning` (for example
`summary = "auto"`) but never set `reasoning.effort`. Where the request and `extra_body` both hold an object, the
two merge key by key: the adapter keeps the request's values, and the relay applies the route's. Because every field a
Chat request sets is harness-owned, this merge never changes a Chat request's bytes. `extra_headers` cannot contain credential headers. Provider
construction validates the route again because callers can construct `Route` directly.

### Chat (`openai_chat`): one system message
Strict single-system chat templates (for example Qwen and Llama on a local server) refuse a system message anywhere but
first, and every compatible server accepts one. So the adapter always sends the leading system messages (the system
prompt, instructions and tool slots) as **one** system message, their texts joined in order by a blank line, with empty
ones left out. The prefix stays byte-stable across steps, so prompt caching holds. A system notice after the conversation
starts, such as an environment change on a resumed conversation, goes as a **user** message that opens with
`[System notice] ` (LLM-U-033). The Responses adapter uses the same join for its `instructions` field (below).

### Responses (`openai_responses`)
The adapter posts to `{base_url}/responses` and builds `input` from our history on every request: messages (text as a
string, user images as `input_image` data URLs), `function_call` items for the assistant's calls and
`function_call_output` items for results. It sends `store: false` and never uses `previous_response_id`, because the
journal is the authority and a resumed run must send exactly what the history says.
- **Instructions:** the leading system messages go as the top-level `instructions`, joined as on Chat, and the field
  is left out when they're all empty. `input` never holds a `system` or `developer` message: some hosted backends
  refuse `system` input messages, and some chat templates behind compatible servers refuse `developer`. A server that
  accepts `instructions` puts it first itself. A later system notice goes as user text opening with
  `[System notice] ` (LLM-U-037).
- **Instructions role:** a route may set `instructions_role = "developer"` to send one leading `developer` input
  message instead; the relay applies it (LLM-U-039).
- **Reasoning:** the effort always goes in `reasoning.effort`, and the route's `reasoning.field` (a Chat field) does not
  apply. Reasoning items are not sent back, so a reasoning model re-reasons at each step of a tool loop; reasoning
  text still streams.
- **Unsupported inputs:** requests have no participant `name` and no stop sequences. A request with `stop` fails
  with `Protocol` before sending.
- **Streamed events:**
  - `response.output_text.delta` and `response.refusal.delta` become text.
  - `response.reasoning_text.delta` and `response.reasoning_summary_text.delta` become reasoning.
  - A function call is buffered until its `done` event and then emitted whole, after its arguments are checked
    against the complete arguments that event carries. A cut stream therefore never carries part of a call; Chat
    streams argument fragments instead.
  - `response.completed` gives usage and `Stop` or `ToolCalls`.
  - `response.incomplete` gives usage and `Length`, `ContentFilter` or `Other(reason)`.
  - Unknown event and item types are ignored.
  - The stream ends at the terminal event; a body without one is `StreamCut`.
- **Stream errors:** an `error` event or `response.failed` keeps only the error `code`, never its message.
  `rate_limit_exceeded` becomes `RateLimited`, and `server_error`, `internal_error`, `overloaded` and
  `server_is_overloaded` become `Unavailable`. Anything else, including account and billing codes, is a permanent
  `Protocol("model error: <code>")` that never triggers a route switch.
- **Reasoning items (ADR-0027):**
  - With a reasoning policy, the request asks for `include: ["reasoning.encrypted_content"]`.
  - A completed `reasoning` item with encrypted content becomes `StreamChunk::ReasoningState`: the item as received,
    without `status`. It is collected into `Completion::reasoning_state`.
  - `ContentPart::ReasoningState` items on an assistant message are replayed unchanged, as input items before that
    message and its calls. Chat leaves them out.
  - A state on any other role, or one that is not a JSON object, is refused with `Protocol`.
- **HTTP errors, retries and the size bound** match Chat. `relay_shaping` shapes relayed Responses requests (pinned
  model, forced streaming, `store: false`, the effort placement) and reads usage from the terminal events.

### Messages (`anthropic_messages`)
The adapter posts to `{base_url}/messages` with the protocol-required `anthropic-version: 2023-06-01`
(`MESSAGES_VERSION_HEADER`, `MESSAGES_VERSION`). The version is pinned, and an `extra_headers` entry with that name
fails validation. Beta features go through `extra_headers`. Requests are built from our history on every request:
- **Route rules:** the route must declare `max_output`, which is sent as the required `max_tokens` (a request's own
  `max_output` wins).
  - With a reasoning policy, `budget_tokens` maps every allowed value to a thinking budget of at least 1024 tokens,
    below `max_output`. For example:
    `reasoning = { field = "thinking", allowed = ["low", "high"], default = "low", budget_tokens = { low = 2048, high = 8192 } }`.
  - The effort becomes `thinking: {type: "enabled", budget_tokens}`, and `reasoning.field` does not apply.
  - A budget table on another protocol fails validation.
  - `extra_body` cannot override `model`, `messages`, `system`, tools, `tool_choice`, `stream`, `max_tokens`,
    `stop_sequences`, temperature, `metadata` or `thinking`.
- **History:**
  - System messages before the first turn become the top-level `system` text blocks. A later system message (a
    notice) is sent as user text.
  - Turns alternate and must start with a user turn. Consecutive same-role messages merge, and tool results become
    `tool_result` blocks of the next user turn, before any user text.
  - Content is always an array of blocks. User images become base64 `image` blocks on `vision` routes.
  - Calls become `tool_use` blocks whose `input` is the argument object. Arguments that are not a JSON object were
    already reported to the model as invalid and are replayed as `{}`, which only satisfies the protocol's shape.
- **Options:**
  - `stop` becomes `stop_sequences`.
  - `parallel_tools = false` sends `tool_choice: {type: "auto", disable_parallel_tool_use: true}`.
  - `metadata` is not sent: the protocol's field only takes a user id.
  - A temperature with thinking on is refused before sending.
  - With `cache_hint = "anthropic_cache_control"`, the last system block and the last tool carry
    `cache_control: {type: "ephemeral"}`.
- **Streamed events:**
  - `text_delta` becomes text and `thinking_delta` becomes reasoning.
  - A `tool_use` block's `input_json_delta` fragments are buffered and emitted whole at the block's stop. An input
    that never arrives is `{}`.
  - `message_delta` carries `stop_reason`: `end_turn` and `stop_sequence` map to `Stop`, `tool_use` to `ToolCalls`,
    `max_tokens` to `Length` and `refusal` to `ContentFilter`. Anything else is `Other(reason)`.
  - Usage and the finish reason are emitted at `message_stop`, which ends the stream; a body without it is
    `StreamCut`.
  - `ping` and unknown event, block and delta types are ignored.
- **Usage:** `prompt_tokens` is the whole prompt: `input_tokens`, `cache_creation_input_tokens` and
  `cache_read_input_tokens` together. `cached_tokens` is the cache read, and `reasoning_tokens` is `None`. `Usage`
  has no cache-write field yet; it comes with per-user metering.
- **Stream errors:** an `error` event keeps only its `type`. `rate_limit_error` becomes `RateLimited`, while
  `overloaded_error` and `api_error` become `Unavailable`. Anything else is a permanent `Protocol("model error:
  <type>")`.
- **Reasoning state (ADR-0027):**
  - A thinking block with a signature, or a redacted thinking block, becomes `ReasoningState`: the block as received.
  - States are replayed unchanged, first in their assistant turn.
  - A state that is not a thinking block is refused.
  - When the latest assistant turn has calls but no state (the state was lost), that one request is sent without
    `thinking` and without states, because the protocol refuses such a turn while thinking is on.
- **Cache breakpoints inside a sandbox:** the route snapshot carries `cache_hint` (protocol revision 7), and the
  runtime builds its route with it. So in-sandbox Messages requests on an `anthropic_cache_control` route carry the
  same markers as above, and the relay passes them through unchanged.
- **Relay:** `relay_shaping::shape_request` pins `model`, forces streaming and merges `extra_body` over the request.
  It only checks `thinking`: enabled thinking must carry a budget from the route's table, a route without one
  refuses it, and absent or disabled thinking passes (a request that lost its reasoning state stays without
  thinking). Thinking blocks, signatures and cache markers pass through unchanged. The relay sends the pinned
  version header itself (`MESSAGES_VERSION_HEADER`).
- **Relay usage:** usage is split across `message_start` (input side) and `message_delta` (output side), so
  `read_usage` returns `None` for this protocol. `relay_shaping::UsageReader` observes every event of one response
  and combines them. For Chat and Responses it keeps the latest report, exactly as `read_usage` reads it.

### Fallback routes
A route may declare `fallbacks = ["a", "b"]`: at most `MAX_FALLBACK_ROUTES` (3) other routes, in order (ADR-0025).
`Registry::from_toml` refuses an unknown name, the route itself, a duplicate, more than three, and any cycle through
the fallback graph; a fallback's own fallbacks are never followed by a run. Fallbacks are admin configuration; the
gateway passes the ones a user may use to the runtime.

## Encoding and decoding without a provider

`encode_request(route, request)` returns exactly the body the route's adapter sends, and `decode_response(protocol,
response, …)` reads a successful streaming response with that protocol's own wire, bounds and assembler checks
(LLM-C-028). They exist for callers that talk to a route without a provider, such as the gateway's route probe,
which must send and read what a runtime would. `request_body_len` is `encode_request(…).len()`.

## Requests and streams

`CompletionRequest` holds ordered messages, tool schemas, reasoning/output/temperature settings, stop strings
and a deterministic metadata map. System, user, assistant and tool messages retain their order. Text parts
concatenate; assistant tool calls retain argument JSON, and tool results require a call identifier. Request
output limits override the route's value. Unsupported native tools, disallowed effort, empty histories and
non-finite temperatures return `Protocol` before sending a request.

Before cloning schemas or constructing the HTTP body, requests enforce the shared inclusive limits of
256 tools, 64 KiB of serialized parameters per tool and 1 MiB for the complete encoded tools array, including
metadata and function wrappers. Oversize input returns non-retryable `LlmError::CatalogLimit` with stable code
`tool_catalog_limit`; its diagnostics name only the exceeded bound and no network request is made.

`provider_for(route, secret, http, clock, sleeper)` returns `Arc<dyn ModelProvider>`. Call
`complete(request, cancellation_token)` for one attempt. The stream yields text, reasoning, indexed tool
argument deltas, usage and a terminal reason. Optional usage counters remain `None` when absent.
`collect(stream, limits)` assembles calls in index order and validates their final argument JSON. Changed call
identifiers/names are protocol errors. A cut retains a boxed `PartialCompletion` snapshot of text, reasoning,
calls and usage. Missing `[DONE]` succeeds only after a terminal finish reason.
The HTTP transport (URLs, credentials, deadlines, status mapping) and the bounded SSE framing are shared by the
adapters. Each protocol supplies only its request encoder and its mapping from SSE data records to chunks.

Both adapter and collector require explicit `StreamLimits`. `StreamLimits::RECOMMENDED` supplies a 1 MiB
SSE-event cap, a 16 MiB combined text/reasoning/call-data cap, and at most 128 distinct calls. Indices do not
allocate sparse arrays. A non-retryable `StreamLimit` identifies the exceeded bound.

`HttpConfig::new(limits)` uses a 10-second connect deadline, a 30-second response-header deadline, a 60-second
idle-body deadline and a 1 MiB non-streaming body cap. `SystemClock` and `TokioSleeper` provide production time.
Connect/TLS deadlines use the HTTP client's runtime timer; header, body and retry waits use the injected
sleeper. An owned stream task and a single-slot output channel preserve backpressure and observe cancellation
even when the caller stops polling. Dropping the stream aborts its task; normal exhaustion joins it. Token
cancellation closes the response and yields `Cancelled`. Tests verify resource release within 200 ms.

`list_models(route, secret)` reads the route's discovery endpoint with the documented HTTP defaults. A 404
logs a discovery note and returns the configured model; other errors remain typed. Dropping its future cancels
the request. Discovery never changes registry limits.

### Images
`ContentPart::Image { media_type, data }` carries raw image bytes of a recognised format. The chat adapter keeps
text-only content as one string, so requests without images keep their exact bytes. Content with an image
becomes an ordered array of `text` and `image_url` parts, each image as a base64 data URL. An image on a route
without `vision` is refused before sending. `request_body_len(route, request)` returns the exact body size the
adapter would send, so the caller can fit images under the relay's request limit. Image bytes are never logged
or placed in errors.

## Errors and retries

Every `LlmError` exposes `code()` and `retryable()`. Display and Debug redact credential header values and
key-shaped tokens; HTTP excerpts also mask the resolved credential. Tracing excludes raw credentials, query
credentials, response headers and partial content. Redaction protects credentials; it does not sanitize
arbitrary prompt or completion content.

| Failure | Retryable |
| --- | --- |
| HTTP 408, 429, 500, 502, 503, 504 and 529 (an overload status some servers send); `RateLimited`, `Unavailable` | Yes |
| Connect/header/idle `Timeout`; `StreamCut` | Yes |
| `Connection`: refused, reset or closed before response headers (stable code `io`) | Yes |
| Other HTTP statuses; `Protocol`; `ToolArgsInvalid` | No |
| `BodyTooLarge`; `StreamLimit`; `UnsupportedProtocol`; `Registry`; `Cancelled`; `Io` | No |

Connection classification uses typed transport causes only at the response-header boundary. DNS failures retain
their phase, including when their underlying OS error says connection refused; TLS verification, malformed headers
and generic I/O stay permanent. An incomplete streaming body uses `StreamCut` with its bounded partial completion.
Failed attempts do not execute tools. The core journals distinct attempt identities and projects only the successful
attempt, so replaying a request cannot append a second copy of a discarded partial answer.

`RetryPolicy::new(max_attempts, base, cap, jitter)` rejects zero attempts, zero base and a cap below the base.
There is no implicit policy; attempt count includes the first operation. Call
`retry(&policy, clock, sleeper, &mut jitter_source, &cancel, operation)` with a `Jitter` implementation.
`ThreadRngJitter` provides production fractions. Disabling jitter uses the exponential delay directly; equal
jitter draws between half and all of that delay using a fraction in `[0, 1)`. Invalid fractions fail. Backoff doubles with saturation and is
capped. A 429 `Retry-After` in seconds or any of the three HTTP-date formats overrides jitter and is also capped. Past
dates produce zero delay. Date conversion uses wall time at receipt; elapsed attempts use the injected clock.
The core uses three total attempts, a two-second base and a thirty-second cap: transport failures wait 1–2 seconds
then 2–4 seconds, preserving at least three seconds of retry opportunity even at minimum jitter. Provider-supplied
`Retry-After` still overrides that calculation within the cap. Per-attempt connect/header/idle deadlines and the run's
wall-clock budget remain in force.

Cancellation interrupts both attempts and backoff. Tracing records attempt numbers, error codes, elapsed
time and retry delays. The helper does not consume or replay a returned stream; callers decide how to handle
partial content before starting another attempt.

A 503 with `Retry-After` is `LlmError::Unavailable { retry_after }` (code `unavailable`), retryable and delayed like
`RateLimited`; a 503 without the header stays `Http { status: 503 }`.

## Adding an adapter

Implement `ModelProvider` in a separate protocol module, consume route capabilities, map wire data to shared
request/chunk/error types, and register its constructor in `provider_for`. Add offline contract, cancellation,
limit and malformed-stream tests using `tqa-testkit`. The loop continues using the provider trait; adapter
selection must not depend on model names. Preserve explicit limits, owned tasks, credential separation and
stable serialization.

## Verification

```sh
cargo nextest run -p tqa-llm -p tqa-testkit
cargo llvm-cov nextest -p tqa-llm --fail-under-lines 90
cargo doc -p tqa-llm --no-deps
```

Tests use synthetic loopback HTTP only, except the opt-in live LLM-I-014 check (`tests/live_model.rs`), which is
ignored by default and runs against a real OpenAI-compatible server named by `TQA_LLM_LIVE_BASE_URL`, with the key read
from the file named by `TQA_LLM_LIVE_KEY_FILE`. Seeded properties cover chunk splits and stable system/tool prefixes
across 20 requests. Contract cases use the `LLM-*` identifiers in [TEST-PLAN](../../docs/TEST-PLAN.md).

`StreamAssembler` exposes the same bounded delta assembly to the agent loop. Its finish checks the
finish reason and call identity while preserving raw arguments for corrective tool results. `collect`
adds strict argument JSON validation. `RetryPolicy::next_delay` computes a bounded delay from a
one-based local attempt and injected jitter, allowing callers to journal between attempts.
