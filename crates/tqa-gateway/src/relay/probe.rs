//! The route probe, "Test route" (GW-I-037): does a configured route work the way a runtime will use it?
//! Two tiny requests go through the relay's own path: the runtime's encoding, the relay's shaping, the route's
//! credential and headers, the route's limiter (without queueing) and the protocol's own decoder. On a route that
//! sends strict tools, up to two more check that a strict tool's string argument keeps its line breaks (GW-U-064). No
//! user data, at most four upstream requests, and the whole probe within 60 s.

use super::{
    RelayConfig,
    request::{self, Operation},
};
use bytes::Bytes;
use futures::StreamExt as _;
use secrecy::{ExposeSecret, SecretString};
use serde::Serialize;
use serde_json::{Value, json};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use tokio_util::sync::CancellationToken;
use tqa_llm::{
    Completion, CompletionRequest, ContentPart, FinishReason, HttpConfig, LlmError, Message,
    Protocol, Role, Route, RouteLimiter, StreamLimits, ToolCall, ToolSpec,
};

/// The whole probe's bound.
const DEADLINE: Duration = Duration::from_mins(1);
/// A local route's whole check: bounded by progress, not by its total (the c16d first hour, F2). Each answer's first
/// byte waits up to `[relay] local_check_timeout`, then a gap of `[relay] idle_timeout` without a token ends it; a
/// model that keeps answering may finish within this outer cap, however slowly it thinks.
const LOCAL_OUTER_CAP: Duration = Duration::from_mins(10);
/// Below this output speed a local route's check passes with [`SLOW_ANSWERS`]: a task's step takes minutes there.
const SLOW_TOKENS_PER_SECOND: u64 = 15;
/// The warning for a local model that works but answers slowly, with its speed (`output_tokens_per_second`).
const SLOW_ANSWERS: &str = "slow_answers";
/// Whether the route reaches a model on this computer (loopback, localhost, the container host) or on the owner's
/// network (a compose service such as `ollama`, `*.local`, `*.internal`, a private, link-local, Tailscale or
/// unique-local address: `route_place`), or one without a key: most likely a local model, which may answer slowly. The
/// one rule for every local-model allowance (GW-U-092); the web repeats it in `web/src/settings/route-local.ts`. No
/// DNS lookup: the address as written decides.
pub(crate) fn local_route(route: &Route) -> bool {
    matches!(route.auth, tqa_llm::Auth::None)
        || crate::route_place::place(&route.base_url) != crate::route_place::Place::Hosted
}

/// The longest any route's check may take: a local route's outer cap, or its first-byte wait if that is longer.
pub(super) fn longest_deadline(config: &RelayConfig) -> Duration {
    DEADLINE.max(local_deadline(config))
}

/// A local route's whole check: [`LOCAL_OUTER_CAP`], or `[relay] local_check_timeout` when that is longer.
fn local_deadline(config: &RelayConfig) -> Duration {
    LOCAL_OUTER_CAP.max(Duration::from_secs(config.local_check_timeout))
}

/// How long the whole check may take on this route: a local model may keep answering up to [`local_deadline`], as long
/// as it makes progress (GW-U-092); a hosted one gets the minute.
pub(super) fn deadline(route: &Route, config: &RelayConfig) -> Duration {
    if local_route(route) {
        local_deadline(config)
    } else {
        DEADLINE
    }
}

/// How long one request of the check waits for its answer's first byte: the relay's own wait, or on a local route as
/// long as the whole check (a local model may think for a while before its first byte).
pub(super) fn first_byte(route: &Route, config: &RelayConfig) -> Duration {
    Duration::from_secs(if local_route(route) {
        config.local_check_timeout
    } else {
        config.header_timeout
    })
}
/// Output tokens per request, beyond any thinking budget.
const OUTPUT_TOKENS: u32 = 64;
/// Output tokens per request on a route that declares no reasoning: room for a model that reasons unasked before its
/// call (seen live: a call cut at 64 tokens), still bounded.
const PLAIN_ALLOWANCE: u32 = 256;
/// Output tokens per request on a route that declares reasoning without a budget table: room to think and answer.
/// Small local models think long (qwen3:4b: 279 to 577 tokens before a call). Also the room a step gets when a model
/// that thinks unasked ran out of the plain allowance (GW-U-095).
const REASONING_ALLOWANCE: u32 = 4096;
/// The warning when a model needed room to think on a route that declares no reasoning (GW-U-095).
const THINKS_UNASKED: &str = "thinks_unasked";
/// The server refused `stream_options`: the check went on without it to see the rest, and fails with this code and the
/// suggestion to turn usage off, since the route's tasks would be refused the same way.
const USAGE_REFUSED: &str = "usage_refused";
/// The model server gives the model less context than tasks need (F3): the check fails, suggesting the route's Context
/// window be set to what the server has, so tasks compact in time until the server's setting is raised.
const CONTEXT_TOO_SMALL: &str = "context_too_small";
/// The server's context is enough but the route's Context window says otherwise (or nothing): a warning, with the same
/// one click.
const CONTEXT_WINDOW_DIFFERS: &str = "context_window_differs";
/// The strict-string check's output tokens: enough for a sentence before the call and the two-line value (GW-U-064).
const LINES_TOKENS: u32 = 256;
/// The most bytes of an upstream error body kept, redacted, in the observation.
const EXCERPT_BYTES: usize = 512;
const TOOL: &str = "probe_echo";
/// The strict-string check's tool (GW-U-064).
const LINES_TOOL: &str = "probe_lines";

/// What a probe needs: the relay's HTTP client and bounds, the route's limiter and credential, and the nonce the
/// model must echo (random in production, fixed in tests).
pub(super) struct Probe<'a> {
    pub client: &'a reqwest::Client,
    pub config: &'a RelayConfig,
    pub limiter: Arc<RouteLimiter>,
    pub secret: Option<SecretString>,
    pub nonce: String,
}

/// One check's outcome; `NotDeclared` when the route does not claim the capability.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Check {
    Passed,
    Failed,
    NotDeclared,
    NotRun,
}

#[derive(Debug, Default, Serialize)]
pub(super) struct UsageSeen {
    pub reported: bool,
    pub cached: bool,
    pub reasoning: bool,
}

#[derive(Debug, Default, Serialize)]
pub(super) struct Tokens {
    pub prompt: u64,
    pub completion: u64,
}

/// A named failure: a stable code, the upstream status when there was one, and a redacted excerpt.
#[derive(Debug, Serialize)]
pub(super) struct Failure {
    pub code: &'static str,
    pub status: Option<u16>,
    pub excerpt: Option<String>,
    /// The route setting a 400's wording points at (a hint; the probe never changes the route).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suggestion: Option<Box<super::probe_hints::Suggestion>>,
    /// What went wrong in the answer itself, redacted and bounded: the decoder's words for `protocol_error` and for a `bad_tool_call` whose arguments couldn't be assembled, the
    /// content type and the start of the body for `not_streaming` (GW-U-061).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    /// On `unreachable` at a loopback address only: whether the container host's name (`host.docker.internal`)
    /// resolves from the gateway's container, so the page offers it only where it would reach this computer.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub container_host: Option<bool>,
}

/// What the probe observed; stored as the route's latest probe (099b-2).
#[derive(Debug, Serialize)]
pub(super) struct Observation {
    pub ok: bool,
    pub streaming: Option<bool>,
    pub tool_call: Check,
    pub round_trip: Check,
    /// Whether a strict tool's string argument keeps its line breaks (GW-U-064); `NotRun` on a route that sends no
    /// strict tool.
    pub strict_line_breaks: Check,
    pub usage: UsageSeen,
    pub reasoning_accepted: Option<bool>,
    pub first_byte_ms: Option<u64>,
    pub total_ms: u64,
    pub tokens: Tokens,
    /// The model's output speed over the check's answers, from their usage and how long they streamed; absent without
    /// usage.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_tokens_per_second: Option<u64>,
    /// How long the answers streamed, from each one's first byte to its end.
    #[serde(skip)]
    pub streamed_ms: u64,
    pub failure: Option<Failure>,
    pub warnings: Vec<&'static str>,
    /// The shape of any opaque extra the endpoint attached to its tool call: each leaf's key path and JSON size, never
    /// a value (ADR-0027, Chat tool-call extras).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tool_call_extras: Vec<String>,
    /// What the model server says the route's model runs with (F3): Ollama's loaded context, on a local route whose
    /// check worked; absent when unknown.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server_context: Option<super::probe_context::ServerContext>,
    /// The server's refusal of `stream_options` earlier in this check: its later requests go without it, and the check
    /// ends failed with it.
    #[serde(skip)]
    pub usage_refusal: Option<Failure>,
}

/// The name a container uses for the computer it runs on (Docker Desktop's; Podman's is `host.containers.internal`).
const CONTAINER_HOST: &str = "host.docker.internal";
/// How long the container host's name may take to resolve.
const CONTAINER_HOST_LOOKUP: Duration = Duration::from_secs(2);

/// `unreachable`, and at a loopback address whether the container host's name resolves from here (see
/// [`Failure::container_host`]).
async fn unreachable(route: &Route) -> Failure {
    let mut failure = fail("unreachable");
    if crate::route_place::loopback(&route.base_url) {
        failure.container_host = Some(resolves(CONTAINER_HOST).await);
    }
    failure
}

/// Whether `name` resolves to an address from the gateway, within [`CONTAINER_HOST_LOOKUP`].
pub(super) async fn resolves(name: &str) -> bool {
    match tokio::time::timeout(CONTAINER_HOST_LOOKUP, tokio::net::lookup_host((name, 80))).await {
        Ok(Ok(mut addresses)) => addresses.next().is_some(),
        _ => false,
    }
}

pub(super) fn fail(code: &'static str) -> Failure {
    Failure {
        code,
        status: None,
        excerpt: None,
        suggestion: None,
        detail: None,
        container_host: None,
    }
}

/// `text` redacted, the route's credential removed, and cut to [`EXCERPT_BYTES`] on a character boundary.
pub(super) fn bounded(text: &str, secret: Option<&SecretString>) -> String {
    let mut text = tqa_llm::redact(text);
    if let Some(secret) = secret
        .map(ExposeSecret::expose_secret)
        .filter(|value| !value.is_empty())
    {
        text = text.replace(secret, "[redacted]");
    }
    let mut end = text.len().min(EXCERPT_BYTES);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text.truncate(end);
    text
}

/// The answer as an event stream: labelled one, or text/plain that starts like one (with a warning, GW-U-062);
/// anything else fails `not_streaming` with its type and the start of its body.
async fn as_stream(
    mut response: reqwest::Response,
    probe: &Probe<'_>,
    observed: &mut Observation,
) -> Result<reqwest::Response, Failure> {
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    // Only a text/plain answer is sniffed, from its first bytes, which then go back in front of the rest.
    let (kind, first) = if super::stream_kind::sniffed(content_type.as_deref()) {
        sniff(&mut response).await
    } else {
        (
            super::stream_kind::classify(content_type.as_deref(), b""),
            Bytes::new(),
        )
    };
    observed.streaming = Some(kind.is_stream());
    if !kind.is_stream() {
        let status = response.status().as_u16();
        let body = excerpt(&mut response, probe.secret.as_ref()).await;
        let detail = bounded(
            &format!(
                "content-type: {}; body: {}{body}",
                content_type.as_deref().unwrap_or("none"),
                String::from_utf8_lossy(&first)
            ),
            probe.secret.as_ref(),
        );
        return Err(Failure {
            status: Some(status),
            detail: Some(detail),
            ..fail("not_streaming")
        });
    }
    if kind == super::stream_kind::StreamKind::Mislabelled {
        if !observed.warnings.contains(&"stream_mislabelled") {
            observed.warnings.push("stream_mislabelled");
        }
        response = prepend(response, first);
    }
    Ok(response)
}

/// A text/plain body's kind, from as many leading chunks as it takes (GW-U-062), and those chunks' bytes.
async fn sniff(response: &mut reqwest::Response) -> (super::stream_kind::StreamKind, Bytes) {
    let mut sniffer = super::stream_kind::Sniffer::default();
    let mut read = Vec::new();
    loop {
        match tokio::time::timeout(Duration::from_secs(5), response.chunk()).await {
            Ok(Ok(Some(chunk))) => {
                read.extend_from_slice(&chunk);
                if let Some(kind) = sniffer.push(&chunk) {
                    return (kind, Bytes::from(read));
                }
            }
            _ => return (sniffer.finish(), Bytes::from(read)),
        }
    }
}

/// `response` with `first` put back in front of the rest of its body.
fn prepend(response: reqwest::Response, first: Bytes) -> reqwest::Response {
    let (status, headers) = (response.status(), response.headers().clone());
    let mut rebuilt = axum::http::Response::new(reqwest::Body::wrap_stream(
        futures::stream::once(async move { Ok::<_, reqwest::Error>(first) })
            .chain(response.bytes_stream()),
    ));
    *rebuilt.status_mut() = status;
    *rebuilt.headers_mut() = headers;
    reqwest::Response::from(rebuilt)
}

/// The first [`EXCERPT_BYTES`] of a response body, read with a short deadline per chunk, redacted.
async fn excerpt(response: &mut reqwest::Response, secret: Option<&SecretString>) -> String {
    let mut kept = Vec::new();
    while kept.len() < EXCERPT_BYTES {
        match tokio::time::timeout(Duration::from_secs(5), response.chunk()).await {
            Ok(Ok(Some(chunk))) => kept.extend_from_slice(&chunk),
            _ => break,
        }
    }
    kept.truncate(EXCERPT_BYTES);
    bounded(&String::from_utf8_lossy(&kept), secret)
}

fn millis(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

/// Runs one probe; it never fails as a call: a failure is part of the observation.
pub(super) async fn run(
    route: &Route,
    probe: Probe<'_>,
    cancel: &CancellationToken,
) -> Observation {
    let started = Instant::now();
    let declared = |yes: bool| {
        if yes {
            Check::NotRun
        } else {
            Check::NotDeclared
        }
    };
    let mut observed = Observation {
        ok: false,
        streaming: None,
        tool_call: declared(route.native_tools),
        round_trip: declared(route.native_tools),
        strict_line_breaks: declared(route.native_tools),
        usage: UsageSeen::default(),
        reasoning_accepted: None,
        first_byte_ms: None,
        total_ms: 0,
        tokens: Tokens::default(),
        output_tokens_per_second: None,
        streamed_ms: 0,
        failure: None,
        warnings: Vec::new(),
        tool_call_extras: Vec::new(),
        server_context: None,
        usage_refusal: None,
    };
    let mut outcome = tokio::select! {biased;
        () = cancel.cancelled() => Err(fail("cancelled")),
        result = tokio::time::timeout(deadline(route, probe.config), steps(route, &probe, &mut observed)) => {
            result.unwrap_or_else(|_| Err(fail("timeout")))
        }
    };
    let out_of_time =
        super::probe_slow::out_of_time_after_round_trip(route, &mut outcome, &mut observed);
    // Only after the model answered: Ollama lists a model once it's loaded.
    if outcome.is_ok() && local_route(route) {
        observed.server_context = tokio::select! {biased;
            () = cancel.cancelled() => None,
            context = super::probe_context::ollama(probe.client, route) => context,
        };
    }
    observed.total_ms = millis(started.elapsed());
    observed.output_tokens_per_second = speed(&observed);
    match outcome {
        // Tasks would lose their request without a word once a conversation outgrows the server's context: failed,
        // before any other refusal (which tasks would at least meet loudly).
        Ok(()) if let Some(failure) = context_too_small(&observed) => {
            observed.failure = Some(failure);
        }
        // The rest worked without usage, but the route's tasks would be refused as the check first was: failed, with
        // the switch to turn off.
        Ok(()) if observed.usage_refusal.is_some() => {
            observed.failure = observed.usage_refusal.take();
        }
        Ok(()) => {
            observed.ok = true;
            if !observed.usage.reported {
                observed.warnings.push("usage_missing");
            }
            if local_route(route)
                && (out_of_time
                    || observed
                        .output_tokens_per_second
                        .is_some_and(|speed| speed < SLOW_TOKENS_PER_SECOND))
            {
                observed.warnings.push(SLOW_ANSWERS);
            }
            if observed
                .server_context
                .as_ref()
                .is_some_and(|server| route.limits.context_window != Some(server.context_length))
            {
                observed.warnings.push(CONTEXT_WINDOW_DIFFERS);
            }
        }
        Err(failure) => observed.failure = Some(failure),
    }
    observed
}

/// Output tokens per second over the answers' streaming time, when usage counted them; a stream too short to time
/// counts as instant.
fn speed(observed: &Observation) -> Option<u64> {
    (observed.usage.reported && observed.tokens.completion > 0)
        .then(|| observed.tokens.completion * 1000 / observed.streamed_ms.max(1))
}

/// The failure when the server's context is below what tasks need, suggesting the route's Context window be set to it.
fn context_too_small(observed: &Observation) -> Option<Failure> {
    let server = observed
        .server_context
        .as_ref()
        .filter(|server| server.context_length < super::probe_context::MINIMUM)?;
    Some(Failure {
        suggestion: Some(Box::new(super::probe_hints::Suggestion {
            field: "context_window",
            value: std::borrow::Cow::Owned(server.context_length.to_string()),
            reason: "The model server gives the model less context than tasks need; until it's raised, the route's \
                     Context window set to what the server has lets tasks compact in time.",
        })),
        ..fail(CONTEXT_TOO_SMALL)
    })
}

async fn steps(
    route: &Route,
    probe: &Probe<'_>,
    observed: &mut Observation,
) -> Result<(), Failure> {
    // A probe never queues behind real traffic: a busy route is reported as busy.
    let _permit = probe
        .limiter
        .try_acquire()
        .map_err(|_| fail("rate_limited"))?;
    let mut request = first_request(route, &probe.nonce);
    let first = match exchange_with_room(route, probe, &mut request, observed).await {
        Ok(first) => first,
        // The first answer's tool call couldn't be assembled: the call itself failed, as check_call would say.
        Err(failure) if failure.code == "bad_tool_call" => {
            observed.tool_call = Check::Failed;
            return Err(failure);
        }
        Err(failure) => return Err(failure),
    };
    observed.reasoning_accepted = route.reasoning.as_ref().map(|_| true);
    if !route.native_tools {
        // A server that held part of the answer back took a call the model wrote natively (GW-U-079): tasks recover
        // with one retry per step, so this is a warning that suggests native tool calls, not a failure.
        if first.withheld_chunks > 0 && first.tool_calls.is_empty() {
            observed.warnings.push("swallowed_calls");
            return Ok(());
        }
        return if first.text.trim().is_empty() {
            Err(fail("protocol_error"))
        } else {
            Ok(())
        };
    }
    let call = check_call(&first, &probe.nonce, observed)?;
    observed.tool_call_extras = extra_shapes(&first.reasoning_state);
    let mut assistant = message(Role::Assistant, &first.text);
    assistant.tool_calls = vec![call.clone()];
    // The answer's opaque states go back as a runtime sends them (ADR-0027).
    assistant.content.extend(
        first
            .reasoning_state
            .iter()
            .map(|item| ContentPart::ReasoningState(item.clone())),
    );
    let mut result = message(Role::Tool, &format!("ok {}", probe.nonce));
    result.tool_call_id = Some(call.id);
    request.messages.extend([assistant, result]);
    let second = match exchange_with_room(route, probe, &mut request, observed).await {
        Ok(second) => second,
        Err(failure) => {
            observed.round_trip = Check::Failed;
            return Err(Failure {
                code: "round_trip_failed",
                ..failure
            });
        }
    };
    if second.finish_reason != Some(FinishReason::Stop) || second.text.trim().is_empty() {
        observed.round_trip = Check::Failed;
        return Err(fail_after("round_trip_failed", &second));
    }
    observed.round_trip = Check::Passed;
    strict_strings(route, probe, observed).await
}

/// The strict-string check (GW-U-064): one call of a strict tool whose string argument must hold a line break. When
/// the call holds the value without its line break, the same request without strict tells whether strict mode lost
/// it: then the probe fails and suggests `strict = "off"`. Any other answer, or a retry that loses it too, says
/// nothing about strict mode: only a warning, and no retry after an answer that isn't the asked call.
async fn strict_strings(
    route: &Route,
    probe: &Probe<'_>,
    observed: &mut Observation,
) -> Result<(), Failure> {
    if !sends_strict(route) {
        return Ok(());
    }
    let mut request = lines_request(route, &probe.nonce);
    // A model that already needed room to think gets it here from the start (GW-U-095).
    if observed.warnings.contains(&THINKS_UNASKED) {
        request.max_output = request.max_output.max(Some(thinking_room(route)));
    }
    // How the strict answer lost the line break, for the failure's detail (GW-U-067).
    let (strict, how) = match exchange_with_room(route, probe, &mut request, observed).await {
        Ok(answer) if answer.finish_reason == Some(FinishReason::Length) => {
            (Lines::Lost, finish(&answer))
        }
        Ok(answer) => (
            line_break(&answer, &probe.nonce),
            format!("the value without its line break, {}", finish(&answer)),
        ),
        // The decoder refused the answer: a call cut off or not JSON (bad_tool_call), as a strict endpoint that ends the
        // string at the line break was seen to send, or another refusal. What strict mode lost, not a broken route.
        Err(failure) if matches!(failure.code, "protocol_error" | "bad_tool_call") => (
            Lines::Lost,
            failure
                .detail
                .unwrap_or_else(|| "refused by the decoder".to_owned()),
        ),
        Err(failure) => {
            observed.strict_line_breaks = Check::Failed;
            return Err(failure);
        }
    };
    match strict {
        Lines::Kept => {
            observed.strict_line_breaks = Check::Passed;
            return Ok(());
        }
        Lines::Unclear => {
            observed.warnings.push("line_breaks_unverified");
            return Ok(());
        }
        Lines::Lost => {}
    }
    let mut plain = route.clone();
    plain.strict = Some("off".into());
    // Whatever the retry does, the route already passed its call and round trip: only a kept line break counts.
    let kept = matches!(
        exchange(&plain, probe, &request, observed).await,
        Ok(answer) if line_break(&answer, &probe.nonce) == Lines::Kept
    );
    if !kept {
        observed.warnings.push("line_breaks_unverified");
        return Ok(());
    }
    observed.strict_line_breaks = Check::Failed;
    Err(Failure {
        suggestion: Some(Box::new(super::probe_hints::STRICT_LOSES_LINE_BREAKS)),
        detail: Some(format!("strict answer: {how}")),
        ..fail("strict_line_breaks_lost")
    })
}

/// Whether the route sends the check's tool with `strict: true`, decided as for any tool: native tools, on a
/// protocol whose tools carry the field, and the route's strict and schema-keyword settings.
fn sends_strict(route: &Route) -> bool {
    route.native_tools
        && route.protocol != Protocol::AnthropicMessages
        && tqa_llm::tool_schemas::shape_specs(
            route.strict_tools(),
            route.schema_keywords(),
            &[lines_tool()],
        )
        .first()
        .is_some_and(|tool| tool.strict)
}

/// A strict-valid tool with one string argument.
fn lines_tool() -> ToolSpec {
    ToolSpec {
        name: LINES_TOOL.into(),
        description: "Record a two-line value for the connectivity test.".into(),
        parameters: json!({"type":"object","properties":{"value":{"type":"string"}},"required":["value"],
                           "additionalProperties":false}),
        strict: true,
    }
}

/// The fixed strict-string conversation; the value is two lines of the nonce.
fn lines_request(route: &Route, nonce: &str) -> CompletionRequest {
    CompletionRequest {
        messages: vec![
            message(
                Role::System,
                "You are a connectivity test. Call the tool exactly once.",
            ),
            message(
                Role::User,
                &format!(
                    "Call {LINES_TOOL} once. Set value to these two lines, with the line break between them:\n\
                     {nonce}\n{nonce}"
                ),
            ),
        ],
        tools: vec![lines_tool()],
        max_output: Some(lines_tokens(route)),
        ..CompletionRequest::default()
    }
}

/// The check's output cap: its own [`LINES_TOKENS`] (or the probe's larger cap for a reasoning route), always within
/// the route's declared limit.
fn lines_tokens(route: &Route) -> u32 {
    let wanted = output_tokens(route).max(LINES_TOKENS);
    route
        .limits
        .max_output
        .map_or(wanted, |limit| wanted.min(limit))
}

/// What the strict-string check's answer shows.
#[derive(Debug, Eq, PartialEq)]
enum Lines {
    /// The check's tool, with a value that holds a line break.
    Kept,
    /// The check's tool, with a value that holds the nonce but no line break.
    Lost,
    /// Anything else: no call, another tool, arguments that aren't an object with a string value, or a value without
    /// the nonce.
    Unclear,
}

/// What the answer's first call shows about the two-line value.
fn line_break(answer: &Completion, nonce: &str) -> Lines {
    let value = answer
        .tool_calls
        .first()
        .filter(|call| call.name == LINES_TOOL)
        .and_then(|call| serde_json::from_str::<Value>(&call.arguments).ok())
        .and_then(|arguments| arguments["value"].as_str().map(str::to_owned));
    match value {
        Some(value) if value.contains('\n') => Lines::Kept,
        Some(value) if value.contains(nonce) => Lines::Lost,
        _ => Lines::Unclear,
    }
}

/// The most extra leaves an observation lists.
const MAX_EXTRA_SHAPES: usize = 16;

/// Each leaf of the answer's Chat tool-call extras as `path: N B` (its JSON size); values are never kept.
fn extra_shapes(states: &[String]) -> Vec<String> {
    fn leaves(path: &str, value: &Value, out: &mut Vec<String>) {
        match value {
            Value::Object(members) if !members.is_empty() => {
                for (key, member) in members {
                    leaves(&format!("{path}.{key}"), member, out);
                }
            }
            leaf => out.push(format!("{path}: {} B", leaf.to_string().len())),
        }
    }
    let mut shapes = Vec::new();
    for state in states {
        let Ok(item) = serde_json::from_str::<Value>(state) else {
            continue;
        };
        if item["type"] == tqa_llm::CHAT_TOOL_CALL_EXTRA {
            leaves("extra_content", &item["extra_content"], &mut shapes);
        }
    }
    // A key path is the endpoint's text: an overlong one is left out rather than cut.
    shapes.retain(|shape| shape.len() <= EXCERPT_BYTES);
    shapes.truncate(MAX_EXTRA_SHAPES);
    shapes
}

/// The first answer's tool call: it must name the probe tool with a JSON-object argument. A wrong echoed value is a
/// warning, since the call parsed; the model's accuracy is not what the probe measures.
fn check_call(
    first: &Completion,
    nonce: &str,
    observed: &mut Observation,
) -> Result<ToolCall, Failure> {
    let Some(call) = first.tool_calls.first() else {
        observed.tool_call = Check::Failed;
        return Err(fail_after("no_tool_call", first));
    };
    let arguments: Option<Value> = serde_json::from_str(&call.arguments).ok();
    let Some(arguments) = arguments.filter(|value| call.name == TOOL && value.is_object()) else {
        observed.tool_call = Check::Failed;
        return Err(fail_after("bad_tool_call", first));
    };
    if arguments["value"] != nonce {
        observed.warnings.push("argument_mismatch");
    }
    observed.tool_call = Check::Passed;
    Ok(call.clone())
}

/// Why an answer stopped, as a failure's detail (GW-U-067): `finish: stop`, `length`, `tool_calls`, `content_filter`,
/// the provider's own word, or `none` when the stream named none.
fn finish(answer: &Completion) -> String {
    let reason = match &answer.finish_reason {
        Some(FinishReason::Stop) => "stop".to_owned(),
        Some(FinishReason::ToolCalls) => "tool_calls".to_owned(),
        Some(FinishReason::Length) => "length".to_owned(),
        Some(FinishReason::ContentFilter) => "content_filter".to_owned(),
        Some(FinishReason::Other(other)) => other.chars().take(64).collect(),
        // A reason a newer decoder names.
        Some(_) => "other".to_owned(),
        None => "none".to_owned(),
    };
    format!("finish: {reason}")
}

/// A failure with its code and why the answer stopped.
fn fail_after(code: &'static str, answer: &Completion) -> Failure {
    Failure {
        detail: Some(finish(answer)),
        ..fail(code)
    }
}

fn message(role: Role, text: &str) -> Message {
    Message {
        role,
        content: vec![ContentPart::Text(text.into())],
        tool_calls: vec![],
        tool_call_id: None,
        name: None,
    }
}

/// The fixed probe conversation; nothing in it comes from a user or a workspace. The stacks' fake model recognises it
/// by these strings and the tool names (tqa-testkit `standalone/route_check.rs`, TK-U-037): change both together.
fn first_request(route: &Route, nonce: &str) -> CompletionRequest {
    let tools = if route.native_tools {
        vec![ToolSpec {
            name: TOOL.into(),
            description: "Echo a value back to the connectivity test.".into(),
            parameters: json!({"type":"object","properties":{"value":{"type":"string"}},"required":["value"]}),
            strict: false,
        }]
    } else {
        vec![]
    };
    CompletionRequest {
        messages: vec![
            message(
                Role::System,
                "You are a connectivity test. Call the tool exactly once.",
            ),
            message(Role::User, &format!("Call {TOOL} with value {nonce}.")),
        ],
        tools,
        max_output: Some(output_tokens(route)),
        ..CompletionRequest::default()
    }
}

/// The probe's output cap, always bounded: [`PLAIN_ALLOWANCE`] on a route without reasoning; a route with a thinking
/// budget table (Messages) gets its smallest budget plus a few tokens; any other route that declares reasoning gets
/// [`REASONING_ALLOWANCE`] to think, since thinking counts against the cap. Always within the route's declared limit.
fn output_tokens(route: &Route) -> u32 {
    let wanted = match route.reasoning.as_ref() {
        None => PLAIN_ALLOWANCE,
        Some(reasoning) => reasoning
            .budget_tokens
            .as_ref()
            .and_then(|budgets| budgets.values().min().copied())
            .map_or(REASONING_ALLOWANCE, |thinking| {
                thinking.saturating_add(OUTPUT_TOKENS)
            }),
    };
    route
        .limits
        .max_output
        .map_or(wanted, |limit| wanted.min(limit))
}

/// The route as a runtime sees it: no vendor settings or credential; the relay adds those, as for real traffic.
pub(super) fn runtime_view(route: &Route) -> Route {
    let mut view = route.clone();
    view.extra_body.clear();
    view.extra_headers.clear();
    view.auth = tqa_llm::Auth::None;
    view.key_ref = None;
    // A runtime always asks for usage (the relay then follows the route's setting), so the check and titles do too.
    view.stream_usage = Some(true);
    if let Some(reasoning) = &mut view.reasoning {
        reasoning.field = "reasoning_effort".into();
    }
    view
}

/// One request through the relay's path. A Chat server that refuses `stream_options` (a 400 or 422 naming it) is asked
/// once more without it, and so is every later request of the check; the check then says the route must turn usage
/// off, since its tasks would be refused the same way.
async fn exchange(
    route: &Route,
    probe: &Probe<'_>,
    request: &CompletionRequest,
    observed: &mut Observation,
) -> Result<Completion, Failure> {
    if observed.usage_refusal.is_some() {
        return exchange_as(&without_usage(route), probe, request, observed).await;
    }
    match exchange_as(route, probe, request, observed).await {
        Err(failure) if refuses_usage(route, &failure) => {
            observed.usage_refusal = Some(Failure {
                code: USAGE_REFUSED,
                suggestion: Some(Box::new(super::probe_hints::Suggestion {
                    field: "stream_usage",
                    value: std::borrow::Cow::Borrowed("false"),
                    reason: "The server refuses usage reporting in the stream.",
                })),
                ..failure
            });
            exchange_as(&without_usage(route), probe, request, observed).await
        }
        answered => answered,
    }
}

/// Whether a failure is a Chat server refusing the `stream_options` the request carried.
fn refuses_usage(route: &Route, failure: &Failure) -> bool {
    route.protocol == Protocol::OpenAiChat
        && route.stream_usage()
        && matches!(failure.status, Some(400 | 422))
        && failure
            .excerpt
            .as_deref()
            .is_some_and(|excerpt| excerpt.to_ascii_lowercase().contains("stream_options"))
}

/// The route with usage in the stream turned off: the relay then leaves `stream_options` out.
fn without_usage(route: &Route) -> Route {
    Route {
        stream_usage: Some(false),
        ..route.clone()
    }
}

/// One request through the relay's path; records streaming, timing and usage.
async fn exchange_as(
    route: &Route,
    probe: &Probe<'_>,
    request: &CompletionRequest,
    observed: &mut Observation,
) -> Result<Completion, Failure> {
    let encoded = tqa_llm::encode_request(&runtime_view(route), request)
        .map_err(|_| fail("protocol_error"))?;
    let mut body: Value = serde_json::from_slice(&encoded).map_err(|_| fail("protocol_error"))?;
    tqa_llm::relay_shaping::shape_request(route, &mut body).map_err(|_| fail("protocol_error"))?;
    let operation = match route.protocol {
        Protocol::OpenAiResponses => Operation::Responses,
        Protocol::AnthropicMessages => Operation::Messages,
        _ => Operation::Chat,
    };
    let bytes = serde_json::to_vec(&body).map_err(|_| fail("protocol_error"))?;
    let builder = request::build(probe.client, route, operation, probe.secret.as_ref())
        .map_err(|_| fail("configuration"))?
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .body(bytes);
    let sent = Instant::now();
    let headers = first_byte(route, probe.config);
    let response = match tokio::time::timeout(headers, builder.send()).await {
        Err(_) => return Err(fail("timeout")),
        Ok(Err(error)) if error.is_timeout() => return Err(fail("timeout")),
        Ok(Err(_)) => return Err(unreachable(route).await),
        Ok(Ok(response)) => response,
    };
    observed.first_byte_ms.get_or_insert(millis(sent.elapsed()));
    let response = refuse_status(response, probe.secret.as_ref())
        .await
        .map_err(super::probe_hints::protocol_endpoint)?;
    let response = as_stream(response, probe, observed).await?;
    let streaming = Instant::now();
    let mut http = HttpConfig::new(StreamLimits::RECOMMENDED);
    http.idle_timeout = Duration::from_secs(probe.config.idle_timeout);
    let stream = tqa_llm::decode_response(
        route.protocol,
        response,
        http,
        Arc::new(tqa_llm::TokioSleeper),
        CancellationToken::new(),
    );
    let completion = tqa_llm::collect(stream, StreamLimits::RECOMMENDED)
        .await
        .map_err(|error| match error {
            LlmError::Timeout(_) => fail("timeout"),
            // Arguments the stream can't assemble into JSON (broken, or cut off at the output limit) are the model's
            // tool call, not the server's protocol: a local model writing broken JSON reads as such.
            LlmError::ToolArgsInvalid { .. } | LlmError::ToolArgsCut { .. } => Failure {
                detail: Some(bounded(&error.to_string(), probe.secret.as_ref())),
                ..fail("bad_tool_call")
            },
            other => Failure {
                detail: Some(bounded(&other.to_string(), probe.secret.as_ref())),
                ..fail("protocol_error")
            },
        })?;
    observed.streamed_ms += millis(streaming.elapsed());
    if let Some(usage) = &completion.usage {
        observed.usage.reported = true;
        observed.usage.cached |= usage.cached_tokens.is_some();
        observed.usage.reasoning |= usage.reasoning_tokens.is_some();
        observed.tokens.prompt += usage.prompt_tokens;
        observed.tokens.completion += usage.completion_tokens;
    }
    Ok(completion)
}

/// One step, asked once more with room to think when its answer ran out at the cap after reasoning (GW-U-095): a model
/// that thinks although the route declares no reasoning spends the plain allowance before its answer. The room is
/// [`REASONING_ALLOWANCE`] within the route's limit, and it stays on the request for the steps after it. A cut without
/// reasoning, or with no room beyond the cap, is the answer as it came.
async fn exchange_with_room(
    route: &Route,
    probe: &Probe<'_>,
    request: &mut CompletionRequest,
    observed: &mut Observation,
) -> Result<Completion, Failure> {
    let answer = exchange(route, probe, request, observed).await?;
    let room = thinking_room(route);
    let thought = !answer.reasoning.trim().is_empty();
    if answer.finish_reason != Some(FinishReason::Length)
        || !thought
        || request.max_output.is_some_and(|cap| cap >= room)
    {
        return Ok(answer);
    }
    request.max_output = Some(room);
    let retried = exchange(route, probe, request, observed).await?;
    // Only a route without reasoning can do better by turning it on; the note is for an answer that then fit.
    if route.reasoning.is_none()
        && retried.finish_reason != Some(FinishReason::Length)
        && !observed.warnings.contains(&THINKS_UNASKED)
    {
        observed.warnings.push(THINKS_UNASKED);
    }
    Ok(retried)
}

/// The room a thinking model gets: [`REASONING_ALLOWANCE`], within the route's declared limit.
fn thinking_room(route: &Route) -> u32 {
    route
        .limits
        .max_output
        .map_or(REASONING_ALLOWANCE, |limit| REASONING_ALLOWANCE.min(limit))
}

/// A non-success answer as a named failure, with a short redacted excerpt that never carries the credential.
pub(super) async fn refuse_status(
    mut response: reqwest::Response,
    secret: Option<&SecretString>,
) -> Result<reqwest::Response, Failure> {
    let status = response.status().as_u16();
    if response.status().is_success() {
        return Ok(response);
    }
    let code = match status {
        401 | 403 => "auth_rejected",
        404 => "not_found",
        429 => "rate_limited",
        _ => "upstream_error",
    };
    let excerpt = excerpt(&mut response, secret).await;
    let suggestion = super::probe_hints::suggestion(status, &excerpt).map(Box::new);
    Err(Failure {
        code,
        status: Some(status),
        excerpt: Some(excerpt),
        suggestion,
        detail: None,
        container_host: None,
    })
}
