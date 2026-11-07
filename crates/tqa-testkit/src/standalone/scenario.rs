use crate::{
    Script, Scripted,
    script_wire::{Frame, render_messages_turn, render_responses_turn, render_turn},
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs::File,
    io::{self, Read},
    path::Path,
    sync::Arc,
    time::Duration,
};
use tqa_llm::{FinishReason, Usage};

const MAX_SCENARIO_BYTES: u64 = 1024 * 1024;
const MAX_WAIT_MS: u64 = 600_000;

/// Validated, immutable response scripts for a standalone test endpoint.
pub struct Scenario {
    pub(super) model: String,
    pub(super) responses: Vec<Response>,
    pub(super) select: Select,
    /// Served to every request without tools, such as a compaction summary call, when present.
    pub(super) summary: Option<Response>,
    /// Model names that fail on request, before the route check and any scripted reply (TK-U-040).
    pub(super) by_model: BTreeMap<String, ByModel>,
    /// How the server presents itself beyond the script: as Ollama, and refusing `stream_options` (c16).
    pub(super) server: ServerTraits,
}

/// What a scenario asks of the server itself (TK-U-046, TK-U-047).
pub(super) struct ServerTraits {
    /// Ollama's loaded models and the context each runs with: when any, `GET /api/version` and `GET /api/ps` answer
    /// as Ollama does.
    pub(super) ollama: BTreeMap<String, u32>,
    /// Models whose Chat Completions requests carrying `stream_options` are refused with a 400 naming it.
    pub(super) refuses_stream_options: std::collections::BTreeSet<String>,
    /// That refusal, as such servers send it.
    usage_refusal: ByModel,
    /// Named `decision/v1` engines served at `/engines/<name>` (TK-U-048).
    pub(super) engines: BTreeMap<String, Engine>,
}

/// How a scenario's decision engine answers (TK-U-048).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Engine {
    /// Stops the run, with this reason.
    Stop(String),
    /// Lets the run go on as without an engine.
    Pass,
}

/// A decision engine as the scenario writes it: exactly one of `stop` and `pass: true`.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EngineInput {
    #[serde(default)]
    stop: Option<String>,
    #[serde(default)]
    pass: Option<bool>,
}

/// How a request for one model name is answered instead of the script (TK-U-040).
pub(super) enum ByModel {
    /// An error status with the provider's body, and a Retry-After header when given.
    Status {
        code: u16,
        body: String,
        retry_after: Option<String>,
    },
    /// A text answer, even to a request that offers tools.
    Text { text: String, finish: bool },
    /// One tool call with these raw argument bytes, named after the request's first tool unless named.
    ToolCall {
        name: Option<String>,
        arguments: String,
        finish: bool,
    },
}

impl Scenario {
    /// How a request for one of the `by_model` names is answered, if it names one; or the refusal of a request carrying
    /// `stream_options` for a model that refuses it (only Chat Completions requests carry it, TK-U-047).
    pub(super) fn failing(&self, body: &Value) -> Option<&ByModel> {
        let model = body["model"].as_str()?;
        if body.get("stream_options").is_some()
            && self.server.refuses_stream_options.contains(model)
        {
            return Some(&self.server.usage_refusal);
        }
        self.by_model.get(model)
    }
}

impl ByModel {
    /// The reply for a request whose first offered tool is `tool`, compiled for its turn.
    pub(super) fn reply(&self, tool: &str, turn: usize) -> io::Result<Option<Response>> {
        let (event, finish) = match self {
            Self::Status { .. } => return Ok(None),
            Self::Text { text, finish } => (Scripted::Text(text.clone()), *finish),
            Self::ToolCall {
                name,
                arguments,
                finish,
            } => (
                Scripted::ToolCall {
                    name: name.clone().unwrap_or_else(|| tool.to_owned()),
                    arguments_json: arguments.clone(),
                    split_into: 1,
                },
                *finish,
            ),
        };
        let mut events = vec![event];
        if finish {
            events.push(Scripted::Finish(FinishReason::Length));
        }
        compile(events, &FIXED_USAGE, Timing::default(), turn).map(Some)
    }
}

/// How a scripted reply is chosen for a request.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub(super) enum Select {
    /// By the number of earlier model responses in the request's history (the default).
    #[default]
    Turn,
    /// In arrival order across requests. A compaction shortens the history, so only this order survives one;
    /// it assumes one conversation per server, as in a dry run.
    Arrival,
    /// By a phrase of the conversation's first user message, then by turn among the replies naming it: parallel
    /// conversations on one server each follow their own script (TK-U-036).
    Prompt,
}

pub(super) struct Response {
    pub plain: Arc<[Frame]>,
    pub usage: Arc<[Frame]>,
    /// The same response as a Responses stream, which always reports usage.
    pub responses: Arc<[Frame]>,
    /// The same response as a Messages stream, which always reports usage.
    pub messages: Arc<[Frame]>,
    pub delay: Duration,
    /// How long the stream holds before its first record, sending keep-alive comments meanwhile.
    pub wait: Duration,
    /// The phrase of the first user message this reply belongs to, under prompt selection.
    pub when: Option<String>,
    /// Served once per conversation, then the next reply naming the same phrase takes its place (TK-U-039).
    pub once: bool,
    /// The reply as written, when a tool argument is a `seq_of` placeholder: it is compiled for each request, with the
    /// seq read from that request's history (TK-U-042).
    pub template: Option<Arc<Template>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Document {
    model: String,
    responses: Vec<Reply>,
    #[serde(default)]
    select: Select,
    #[serde(default)]
    summary: Option<Reply>,
    #[serde(default)]
    by_model: BTreeMap<String, ModelInput>,
    #[serde(default)]
    ollama: BTreeMap<String, u32>,
    #[serde(default)]
    refuses_stream_options: Vec<String>,
    #[serde(default)]
    decision_engines: BTreeMap<String, EngineInput>,
}

/// A model name's failure as the scenario writes it: exactly one of `status`, `text` and `tool_call`.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ModelInput {
    #[serde(default)]
    status: Option<u16>,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    retry_after: Option<String>,
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    tool_call: Option<RawCall>,
    #[serde(default)]
    finish: Option<Finish>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawCall {
    #[serde(default)]
    name: Option<String>,
    /// Sent as these exact bytes: not checked to be JSON, so a test can send what a broken model does.
    arguments: String,
}

/// How a reply ends when it doesn't end normally: cut at the model's output limit.
#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Finish {
    Length,
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct Reply {
    events: Vec<Output>,
    usage: TokenCounts,
    #[serde(default = "default_delay")]
    delay_ms: u64,
    /// A hold before the first record, 0–600000 ms: the run stays active, so a stack test can keep a slot or a lease
    /// and end it with Stop (TK-U-035).
    #[serde(default)]
    wait_ms: u64,
    /// Under prompt selection, the phrase of the conversation's first user message this reply belongs to.
    #[serde(default)]
    when: Option<String>,
    /// Cut at the output limit, as a model server cuts a reply (TK-U-038).
    #[serde(default)]
    finish: Option<Finish>,
    /// Under prompt selection, served once per conversation (TK-U-039).
    #[serde(default)]
    once: bool,
}

#[derive(Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
enum Output {
    Text {
        text: String,
    },
    Reasoning {
        text: String,
    },
    ToolCall {
        name: String,
        arguments: Value,
        split_into: usize,
        /// Only the first half of the arguments is sent, as a cut reply leaves them; needs `"finish": "length"`.
        #[serde(default)]
        cut: bool,
    },
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct TokenCounts {
    #[serde(rename = "prompt_tokens")]
    prompt: u64,
    #[serde(rename = "completion_tokens")]
    completion: u64,
    #[serde(rename = "cached_tokens")]
    cached: Option<u64>,
    #[serde(rename = "reasoning_tokens")]
    reasoning: Option<u64>,
}

/// A reply made for one request rather than read from the scenario (the route check's, TK-U-037).
pub(super) enum Answer {
    /// One call of `name` with `{"value": value}`.
    Call { name: &'static str, value: String },
    /// A text answer.
    Text(&'static str),
}

impl Answer {
    /// The answer as a compiled response for the request's turn, streamed at once.
    pub(super) fn compile(self, turn: usize) -> io::Result<Response> {
        let event = match self {
            Self::Call { name, value } => Output::ToolCall {
                name: name.into(),
                arguments: serde_json::json!({ "value": value }),
                split_into: 1,
                cut: false,
            },
            Self::Text(text) => Output::Text { text: text.into() },
        };
        Reply {
            events: vec![event],
            usage: TokenCounts {
                prompt: 20,
                completion: 5,
                cached: None,
                reasoning: None,
            },
            delay_ms: 0,
            wait_ms: 0,
            when: None,
            finish: None,
            once: false,
        }
        .compile(turn)
    }
}

fn default_delay() -> u64 {
    20
}
fn invalid(reason: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, reason.into())
}

impl Scenario {
    /// Loads at most 1 MiB and validates the complete scenario before binding a listener.
    /// Responses have 1–128 events, at most 512 wire records and a 0–1000 ms chunk delay.
    ///
    /// # Errors
    /// Rejects malformed JSON, unknown keys, invalid bounds and non-object tool arguments.
    pub fn load(path: &Path) -> io::Result<Self> {
        if !std::fs::metadata(path)?.is_file() {
            return Err(invalid("scenario must be a regular file"));
        }
        let mut bytes = Vec::new();
        File::open(path)?
            .take(MAX_SCENARIO_BYTES + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() as u64 > MAX_SCENARIO_BYTES {
            return Err(invalid("scenario exceeds 1 MiB"));
        }
        let mut document: Document = serde_json::from_slice(&bytes)
            .map_err(|error| invalid(format!("invalid scenario: {error}")))?;
        if document.model.trim().is_empty()
            || document.model.len() > 128
            || document.model.chars().any(char::is_control)
        {
            return Err(invalid(
                "scenario model must have 1–128 bytes without control characters",
            ));
        }
        if document.responses.is_empty() || document.responses.len() > 128 {
            return Err(invalid("scenario requires 1–128 responses"));
        }
        let prompt = document.select == Select::Prompt;
        if document.responses.iter().any(|reply| {
            reply.when.is_some() != prompt
                || reply
                    .when
                    .as_deref()
                    .is_some_and(|when| when.trim().is_empty())
        }) {
            return Err(invalid(
                "every reply names a nonempty `when` under prompt selection, and none does otherwise",
            ));
        }
        if !prompt && document.responses.iter().any(|reply| reply.once) {
            return Err(invalid("`once` goes with prompt selection only"));
        }
        if document.by_model.len() > 32
            || document
                .by_model
                .keys()
                .any(|name| name.is_empty() || name.len() > 128 || *name == document.model)
        {
            return Err(invalid(
                "by_model names up to 32 other models, each 1–128 bytes",
            ));
        }
        let server = document.server()?;
        let by_model = document
            .by_model
            .into_iter()
            .map(|(name, input)| input.compile().map(|behaviour| (name, behaviour)))
            .collect::<io::Result<_>>()?;
        let count = document.responses.len();
        let responses = document
            .responses
            .into_iter()
            .enumerate()
            .map(|(turn, reply)| reply.compile(turn))
            .collect::<io::Result<_>>()?;
        // The summary gets a turn number after every scripted reply, so its identifiers never collide.
        if document
            .summary
            .as_ref()
            .is_some_and(|reply| reply.when.is_some())
        {
            return Err(invalid("the summary reply takes no `when`"));
        }
        let summary = document
            .summary
            .map(|reply| reply.compile(count))
            .transpose()?;
        Ok(Self {
            model: document.model,
            responses,
            select: document.select,
            summary,
            by_model,
            server,
        })
    }
}

impl Document {
    /// The server's own traits, taken out of the document and checked (TK-U-046..048).
    fn server(&mut self) -> io::Result<ServerTraits> {
        ServerTraits::new(
            std::mem::take(&mut self.ollama),
            std::mem::take(&mut self.refuses_stream_options),
            std::mem::take(&mut self.decision_engines),
        )
    }
}

impl ServerTraits {
    /// The scenario's `ollama` and `refuses_stream_options`, checked: up to 32 models each, named in 1–128 bytes, and each
    /// Ollama context above 0 (TK-U-046, TK-U-047).
    fn new(
        ollama: BTreeMap<String, u32>,
        refuses: Vec<String>,
        engines: BTreeMap<String, EngineInput>,
    ) -> io::Result<Self> {
        if ollama.len() > 32
            || unusable_names(ollama.keys())
            || ollama.values().any(|context| *context == 0)
        {
            return Err(invalid(
                "ollama names up to 32 models, each 1–128 bytes, with a context above 0",
            ));
        }
        if refuses.len() > 32 || unusable_names(refuses.iter()) {
            return Err(invalid(
                "refuses_stream_options names up to 32 models, each 1–128 bytes",
            ));
        }
        Ok(Self {
            ollama,
            refuses_stream_options: refuses.into_iter().collect(),
            usage_refusal: usage_refusal(),
            engines: engines_of(engines)?,
        })
    }
}

/// The scenario's decision engines, checked: up to 8, each named in lowercase letters, digits and dashes (1-32,
/// starting with a letter or digit), and exactly one of a stop reason (1-200 characters, no control character) or
/// `pass: true` (TK-U-048).
fn engines_of(engines: BTreeMap<String, EngineInput>) -> io::Result<BTreeMap<String, Engine>> {
    if engines.len() > 8 {
        return Err(invalid("decision_engines names up to 8 engines"));
    }
    engines
        .into_iter()
        .map(|(name, input)| {
            let named = !name.is_empty()
                && name.len() <= 32
                && name.starts_with(|c: char| c.is_ascii_lowercase() || c.is_ascii_digit())
                && name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
            if !named {
                return Err(invalid("a decision engine is named in 1-32 lowercase letters, digits and dashes"));
            }
            match (input.stop, input.pass) {
                (Some(reason), None)
                    if (1..=200).contains(&reason.chars().count()) && !reason.chars().any(char::is_control) =>
                {
                    Ok((name, Engine::Stop(reason)))
                }
                (None, Some(true)) => Ok((name, Engine::Pass)),
                _ => Err(invalid(
                    "a decision engine is exactly one of a stop reason (1-200 characters) or pass: true",
                )),
            }
        })
        .collect()
}

/// A refused `stream_options`, as OpenAI-compatible servers that don't take it answer (TK-U-047).
fn usage_refusal() -> ByModel {
    ByModel::Status {
        code: 400,
        body: json!({"error": {
            "message": "Unrecognized request argument supplied: stream_options",
            "type": "invalid_request_error",
            "param": "stream_options",
            "code": null,
        }})
        .to_string(),
        retry_after: None,
    }
}

/// Whether any model name is empty or over 128 bytes (`ollama`, `refuses_stream_options`).
fn unusable_names<'a>(mut names: impl Iterator<Item = &'a String>) -> bool {
    names.any(|name| name.is_empty() || name.len() > 128)
}

/// The usage a reply made for one request reports (the route check's and a model name's).
const FIXED_USAGE: TokenCounts = TokenCounts {
    prompt: 20,
    completion: 5,
    cached: None,
    reasoning: None,
};

/// A reply's pacing and selection, apart from its events.
#[derive(Default)]
struct Timing {
    delay_ms: u64,
    wait_ms: u64,
    when: Option<String>,
    once: bool,
}

impl Reply {
    fn compile(self, turn: usize) -> io::Result<Response> {
        let mut templated = false;
        for event in &self.events {
            if let Output::ToolCall { arguments, .. } = event {
                templated |= has_placeholder(arguments).map_err(invalid)?;
            }
        }
        let template = templated.then(|| {
            Arc::new(Template {
                reply: self.clone(),
                turn,
            })
        });
        let mut response = self.compile_fixed(turn)?;
        response.template = template;
        Ok(response)
    }

    fn compile_fixed(self, turn: usize) -> io::Result<Response> {
        if self.delay_ms > 1000 {
            return Err(invalid("scenario delay exceeds 1000 ms"));
        }
        if self.wait_ms > MAX_WAIT_MS {
            return Err(invalid("scenario wait exceeds 600000 ms"));
        }
        if self.events.is_empty() || self.events.len() > 128 {
            return Err(invalid("response requires 1–128 events"));
        }
        let cut = self.finish.is_some();
        if !cut
            && self
                .events
                .iter()
                .any(|event| matches!(event, Output::ToolCall { cut: true, .. }))
        {
            return Err(invalid("a cut tool call needs \"finish\": \"length\""));
        }
        let mut events = self
            .events
            .into_iter()
            .map(Output::script)
            .collect::<io::Result<Vec<_>>>()?;
        if cut {
            events.push(Scripted::Finish(FinishReason::Length));
        }
        let timing = Timing {
            delay_ms: self.delay_ms,
            wait_ms: self.wait_ms,
            when: self.when,
            once: self.once,
        };
        compile(events, &self.usage, timing, turn)
    }
}

/// A reply's events, with its usage appended for the wires that report it, rendered on every wire.
fn compile(
    events: Vec<Scripted>,
    usage: &TokenCounts,
    timing: Timing,
    turn: usize,
) -> io::Result<Response> {
    let mut script = Script::new(events);
    let plain = render_turn(&script, turn)?;
    script.events.push(Scripted::Usage(Usage {
        prompt_tokens: usage.prompt,
        completion_tokens: usage.completion,
        cached_tokens: usage.cached,
        reasoning_tokens: usage.reasoning,
    }));
    let with_usage = render_turn(&script, turn)?;
    let responses = render_responses_turn(&script, turn)?;
    let messages = render_messages_turn(&script, turn)?;
    if [&with_usage, &responses, &messages]
        .into_iter()
        .any(|frames| {
            frames.len() > 512
                || frames.iter().map(|frame| frame.bytes.len()).sum::<usize>() > 2 * 1024 * 1024
        })
    {
        return Err(invalid("response exceeds wire record or byte limit"));
    }
    Ok(Response {
        plain: plain.into(),
        usage: with_usage.into(),
        responses: responses.into(),
        messages: messages.into(),
        delay: Duration::from_millis(timing.delay_ms),
        wait: Duration::from_millis(timing.wait_ms),
        when: timing.when,
        once: timing.once,
        template: None,
    })
}

/// A reply with `seq_of` placeholders, compiled per request (TK-U-042).
pub(super) struct Template {
    reply: Reply,
    turn: usize,
}

impl Template {
    /// The reply for a request whose body is `body`: each `{"seq_of": {"type": T, "from": F}}` argument becomes the
    /// latest seq the request's history names as `seq="N" type="T" from="F"` (a team read's block, ADR-0034).
    ///
    /// # Errors
    /// A placeholder the history doesn't answer, named, never a fallback seq; or a reply that won't compile.
    pub(super) fn resolve(&self, body: &Value) -> Result<Response, String> {
        let mut texts = Vec::new();
        strings(body, &mut texts);
        let mut reply = self.reply.clone();
        for event in &mut reply.events {
            if let Output::ToolCall { arguments, .. } = event {
                fill(arguments, &texts)?;
            }
        }
        reply.compile(self.turn).map_err(|error| error.to_string())
    }
}

/// Every string value in `value`, at any depth.
fn strings<'a>(value: &'a Value, out: &mut Vec<&'a str>) {
    match value {
        Value::String(text) => out.push(text),
        Value::Array(items) => items.iter().for_each(|item| strings(item, out)),
        Value::Object(members) => members.values().for_each(|member| strings(member, out)),
        _ => {}
    }
}

/// The `(type, from)` of a `seq_of` placeholder; `Err` when `value` has the key but not that shape.
fn placeholder(value: &Value) -> Result<Option<(&str, &str)>, &'static str> {
    let Some(inner) = value
        .as_object()
        .filter(|members| members.len() == 1)
        .and_then(|members| members.get("seq_of"))
    else {
        return Ok(None);
    };
    let shape = "seq_of takes {\"type\": a message type, \"from\": a member name}, both nonempty";
    let members = inner
        .as_object()
        .filter(|members| members.len() == 2)
        .ok_or(shape)?;
    match (
        members.get("type").and_then(Value::as_str),
        members.get("from").and_then(Value::as_str),
    ) {
        (Some(kind), Some(from)) if !kind.is_empty() && !from.is_empty() => Ok(Some((kind, from))),
        _ => Err(shape),
    }
}

/// Whether `value` holds a placeholder at any depth, each one well formed.
fn has_placeholder(value: &Value) -> Result<bool, &'static str> {
    if placeholder(value)?.is_some() {
        return Ok(true);
    }
    let mut found = false;
    match value {
        Value::Array(items) => {
            for item in items {
                found |= has_placeholder(item)?;
            }
        }
        Value::Object(members) => {
            for member in members.values() {
                found |= has_placeholder(member)?;
            }
        }
        _ => {}
    }
    Ok(found)
}

/// Replaces each placeholder in `value` with its seq from `texts`.
fn fill(value: &mut Value, texts: &[&str]) -> Result<(), String> {
    if let Some((kind, from)) = placeholder(value).map_err(str::to_owned)? {
        let seq = latest_seq(texts, kind, from).ok_or_else(|| {
            format!("seq_of: the conversation's history names no {kind} from {from}")
        })?;
        *value = Value::from(seq);
        return Ok(());
    }
    match value {
        Value::Array(items) => items.iter_mut().try_for_each(|item| fill(item, texts)),
        Value::Object(members) => members
            .values_mut()
            .try_for_each(|member| fill(member, texts)),
        _ => Ok(()),
    }
}

/// The largest N among `seq="N" type="{kind}" from="{from}"` in `texts`.
fn latest_seq(texts: &[&str], kind: &str, from: &str) -> Option<u64> {
    let after = format!("\" type=\"{kind}\" from=\"{from}\"");
    texts
        .iter()
        .flat_map(|text| text.match_indices(&after).map(move |(at, _)| &text[..at]))
        .filter_map(|before| {
            let digits = before.rsplit_once("seq=\"")?.1;
            if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
                return None;
            }
            digits.parse().ok()
        })
        .max()
}

impl ModelInput {
    fn compile(self) -> io::Result<ByModel> {
        let finish = self.finish.is_some();
        match (self.status, self.text, self.tool_call) {
            (Some(code), None, None) if (400..=599).contains(&code) && !finish => {
                Ok(ByModel::Status {
                    code,
                    body: self.body.unwrap_or_default(),
                    retry_after: self.retry_after,
                })
            }
            (None, Some(text), None)
                if !text.is_empty() && self.body.is_none() && self.retry_after.is_none() =>
            {
                Ok(ByModel::Text { text, finish })
            }
            (None, None, Some(call)) if self.body.is_none() && self.retry_after.is_none() => {
                if call
                    .name
                    .as_deref()
                    .is_some_and(|name| name.is_empty() || name.len() > 128)
                    || call.arguments.len() > 64 * 1024
                {
                    return Err(invalid(
                        "a model's tool call needs a 1–128 byte name and at most 64 KiB of arguments",
                    ));
                }
                Ok(ByModel::ToolCall {
                    name: call.name,
                    arguments: call.arguments,
                    finish,
                })
            }
            _ => Err(invalid(
                "a by_model entry is exactly one of an error status (400–599), a text or a tool call",
            )),
        }
    }
}

impl Output {
    fn script(self) -> io::Result<Scripted> {
        match self {
            Self::Text { text } if !text.is_empty() => Ok(Scripted::Text(text)),
            Self::Reasoning { text } if !text.is_empty() => Ok(Scripted::Reasoning(text)),
            Self::ToolCall {
                name,
                arguments,
                split_into,
                cut,
            } => {
                if name.is_empty() || name.len() > 128 || !arguments.is_object() {
                    return Err(invalid("tool call requires a name and an argument object"));
                }
                if !(1..=64).contains(&split_into) {
                    return Err(invalid("tool fragment count must be 1–64"));
                }
                let mut arguments_json = arguments.to_string();
                if cut {
                    // The first half, at a character boundary: incomplete JSON, as a cut reply leaves it.
                    let half = arguments_json
                        .char_indices()
                        .map(|(index, _)| index)
                        .find(|index| *index >= arguments_json.len() / 2)
                        .unwrap_or(arguments_json.len());
                    arguments_json.truncate(half);
                }
                Ok(Scripted::ToolCall {
                    name,
                    arguments_json,
                    split_into,
                })
            }
            _ => Err(invalid("text and reasoning deltas must not be empty")),
        }
    }
}
