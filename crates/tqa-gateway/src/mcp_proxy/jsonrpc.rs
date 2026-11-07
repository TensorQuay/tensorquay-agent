//! What the MCP proxy lets through (ADR-0031 decision 2): one JSON-RPC message or a bounded batch, only the tool
//! methods, only the connector's allowed tools; and the headers it forwards each way. Pure, so every rule is a unit
//! test (GW-U-038).
use serde_json::{Map, Value, json};

use crate::connectors::ToolFilter;

/// The largest request body and the largest answer the proxy passes on (the stdio line bound, ADR-0011).
pub(crate) const MAX_MESSAGE_BYTES: usize = 1 << 20;
/// The most messages in one batch.
pub(crate) const MAX_BATCH: usize = 32;
/// The methods a runtime may send to a connector; resources, prompts, completion, logging, sampling and elicitation
/// never pass.
const METHODS: [&str; 6] = [
    "initialize",
    "notifications/initialized",
    "ping",
    "tools/list",
    "tools/call",
    "notifications/cancelled",
];
/// Request headers forwarded to the remote server; everything else (credentials, cookies, hop-by-hop) is dropped.
const FORWARDED_REQUEST_HEADERS: [&str; 4] = [
    "content-type",
    "accept",
    "mcp-protocol-version",
    "mcp-session-id",
];
/// Answer headers passed back to the runtime.
const FORWARDED_ANSWER_HEADERS: [&str; 2] = ["content-type", "mcp-session-id"];
/// JSON-RPC: the method does not exist or is not available.
const METHOD_NOT_FOUND: i64 = -32601;
/// JSON-RPC: invalid params (a tool the connector does not offer).
const INVALID_PARAMS: i64 = -32602;

/// A request body the proxy will not forward at all.
#[derive(Debug, Eq, PartialEq, thiserror::Error)]
pub(crate) enum Unusable {
    /// Over [`MAX_MESSAGE_BYTES`].
    #[error("the body is larger than one MCP message may be")]
    TooLarge,
    /// Not JSON-RPC 2.0, an empty or oversized batch, or a message without a string method or valid id.
    #[error("the body is not a JSON-RPC 2.0 message or batch")]
    Invalid,
}

/// A parsed request body.
#[derive(Debug)]
pub(crate) struct Messages {
    messages: Vec<Map<String, Value>>,
    batch: bool,
}

/// What to do with a parsed body.
#[derive(Debug, Eq, PartialEq)]
pub(crate) enum Verdict {
    /// Every message may pass.
    Forward,
    /// Refused without contacting the remote: the JSON-RPC answer for the requests among them (`None` when every
    /// message was a notification).
    Refuse(Option<Value>),
}

/// Parses one message or a batch, bounded.
///
/// # Errors
/// [`Unusable`] for an oversized or malformed body.
pub(crate) fn parse(bytes: &[u8]) -> Result<Messages, Unusable> {
    if bytes.len() > MAX_MESSAGE_BYTES {
        return Err(Unusable::TooLarge);
    }
    let value: Value = serde_json::from_slice(bytes).map_err(|_| Unusable::Invalid)?;
    let (items, batch) = match value {
        Value::Array(items) if !items.is_empty() && items.len() <= MAX_BATCH => (items, true),
        Value::Object(_) => (vec![value], false),
        _ => return Err(Unusable::Invalid),
    };
    let messages = items
        .into_iter()
        .map(|item| match item {
            Value::Object(message) if valid(&message) => Ok(message),
            _ => Err(Unusable::Invalid),
        })
        .collect::<Result<_, _>>()?;
    Ok(Messages { messages, batch })
}

/// JSON-RPC 2.0, with a string or integer id when present. A message without a method (a response to a request the
/// server sent, such as sampling) is valid JSON-RPC but is never allowed through (see [`check`]).
fn valid(message: &Map<String, Value>) -> bool {
    let version = message.get("jsonrpc") == Some(&json!("2.0"));
    let id = message
        .get("id")
        .is_none_or(|id| id.is_string() || id.is_i64() || id.is_u64());
    let method = message.get("method").is_none_or(Value::is_string);
    version && id && method
}

impl Messages {
    /// How many messages the body carries: what the connector rate charges.
    pub(crate) fn count(&self) -> usize {
        self.messages.len()
    }

    /// The ids of the `tools/list` requests, whose answers are filtered.
    pub(crate) fn tools_list_ids(&self) -> Vec<Value> {
        self.messages
            .iter()
            .filter(|message| method(message) == Some("tools/list"))
            .filter_map(|message| message.get("id").cloned())
            .collect()
    }

    /// The id of the single `tools/call` request this body is, if it is one (what a cancellation names).
    pub(crate) fn single_call_id(&self) -> Option<Value> {
        match self.messages.as_slice() {
            [message] if !self.batch && method(message) == Some("tools/call") => {
                message.get("id").cloned()
            }
            _ => None,
        }
    }

    /// The methods, for the metric label: the first message's method, or `batch`.
    pub(crate) fn label(&self) -> &'static str {
        if self.batch {
            return "batch";
        }
        self.messages
            .first()
            .and_then(|message| method(message))
            .and_then(|name| METHODS.iter().find(|allowed| **allowed == name).copied())
            .unwrap_or("other")
    }
}

fn method(message: &Map<String, Value>) -> Option<&str> {
    message.get("method").and_then(Value::as_str)
}

/// Decides a body: every message must be an allowed method, and a `tools/call` must name an allowed tool.
pub(crate) fn check(messages: &Messages, filter: Option<&ToolFilter>) -> Verdict {
    let refusals: Vec<(Option<&Value>, i64, &str)> = messages
        .messages
        .iter()
        .filter_map(|message| {
            let id = message.get("id");
            match method(message) {
                Some(name) if !METHODS.contains(&name) => Some((
                    id,
                    METHOD_NOT_FOUND,
                    "method not available through this connector",
                )),
                None => Some((
                    id,
                    METHOD_NOT_FOUND,
                    "answers to server requests are not accepted",
                )),
                Some("tools/call") if !tool_allowed(message, filter) => Some((
                    id,
                    INVALID_PARAMS,
                    "tool not available through this connector",
                )),
                Some(_) => None,
            }
        })
        .collect();
    if refusals.is_empty() {
        return Verdict::Forward;
    }
    // One refused message refuses the body: the proxy never forwards part of a batch.
    let answers: Vec<Value> = messages
        .messages
        .iter()
        .filter_map(|message| message.get("id").cloned().map(|id| (id, message)))
        .map(|(id, message)| {
            let (code, text) = refusals
                .iter()
                .find(|(refused, _, _)| *refused == message.get("id"))
                .map_or(
                    (METHOD_NOT_FOUND, "refused with the rest of the batch"),
                    |(_, code, text)| (*code, *text),
                );
            json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": text}})
        })
        .collect();
    Verdict::Refuse(match (answers.len(), messages.batch) {
        (0, _) => None,
        (_, true) => Some(Value::Array(answers)),
        (_, false) => answers.into_iter().next(),
    })
}

fn tool_allowed(message: &Map<String, Value>, filter: Option<&ToolFilter>) -> bool {
    let Some(name) = message
        .get("params")
        .and_then(|params| params.get("name"))
        .and_then(Value::as_str)
    else {
        return false;
    };
    allowed(name, filter)
}

/// Whether the filter offers the tool.
fn allowed(name: &str, filter: Option<&ToolFilter>) -> bool {
    match filter {
        None => true,
        Some(ToolFilter {
            allow: Some(allow), ..
        }) => allow.iter().any(|tool| tool == name),
        Some(ToolFilter {
            deny: Some(deny), ..
        }) => !deny.iter().any(|tool| tool == name),
        Some(_) => false,
    }
}

/// Removes the tools the filter does not offer from the answers to `tools/list` requests (by id), in a single answer
/// or a batch; any other answer is untouched.
pub(crate) fn filter_answer(answer: &mut Value, list_ids: &[Value], filter: Option<&ToolFilter>) {
    if filter.is_none() || list_ids.is_empty() {
        return;
    }
    let filter_one = |message: &mut Value| {
        let is_list = message.get("id").is_some_and(|id| list_ids.contains(id));
        if let (true, Some(Value::Array(tools))) = (
            is_list,
            message
                .get_mut("result")
                .and_then(|result| result.get_mut("tools")),
        ) {
            tools.retain(|tool| {
                tool.get("name")
                    .and_then(Value::as_str)
                    .is_some_and(|name| allowed(name, filter))
            });
        }
    };
    match answer {
        Value::Array(messages) => messages.iter_mut().for_each(filter_one),
        message => filter_one(message),
    }
}

/// Whether a request header from the runtime is forwarded (never a credential, cookie or hop-by-hop header).
pub(crate) fn forward_request_header(name: &str) -> bool {
    FORWARDED_REQUEST_HEADERS
        .iter()
        .any(|allowed| name.eq_ignore_ascii_case(allowed))
}

/// Whether an answer header from the remote is passed back (never a cookie or an authentication challenge).
pub(crate) fn forward_answer_header(name: &str) -> bool {
    FORWARDED_ANSWER_HEADERS
        .iter()
        .any(|allowed| name.eq_ignore_ascii_case(allowed))
}

#[cfg(test)]
#[path = "jsonrpc_tests.rs"]
mod tests;
