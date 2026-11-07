//! The gateway's route check (its fixed probe conversation, tqa-gateway `relay/probe.rs`), answered by the fake itself
//! with the check's own nonce (TK-U-037), so a stack's check says Verified whatever the scenario. The check is known by
//! its one tool, `probe_echo` or `probe_lines` (or none, on a route without native tools), and its fixed user message;
//! a task never offers those tools, and a request that offers one with any other message goes to the scenario.
use super::scenario::Answer;
use serde_json::Value;

const ECHO: &str = "probe_echo";
const LINES: &str = "probe_lines";
const ECHO_PREFIX: &str = "Call probe_echo with value ";
const LINES_PREFIX: &str =
    "Call probe_lines once. Set value to these two lines, with the line break between them:\n";
/// What the check hears after its tool's result, or instead of a call on a route without native tools.
const PASSED: &str = "The connectivity test passed.";

/// The answer to a route check request, or `None` for any other request.
pub(super) fn recognise(body: &Value) -> Option<Answer> {
    let tools = tool_names(body);
    let first = super::first_user_text(body);
    if tools == [LINES] {
        let value = first.strip_prefix(LINES_PREFIX)?;
        let (one, two) = value.split_once('\n')?;
        return (nonce(one) && one == two).then(|| Answer::Call {
            name: LINES,
            value: value.to_owned(),
        });
    }
    if !(tools.is_empty() || tools == [ECHO]) {
        return None;
    }
    let value = first.strip_prefix(ECHO_PREFIX)?.strip_suffix('.')?;
    if !nonce(value) {
        return None;
    }
    if tools.is_empty() || has_tool_result(body) {
        Some(Answer::Text(PASSED))
    } else {
        Some(Answer::Call {
            name: ECHO,
            value: value.to_owned(),
        })
    }
}

/// The check's nonce: a short run of letters and digits (the gateway sends eight hex digits).
fn nonce(value: &str) -> bool {
    (1..=64).contains(&value.len()) && value.bytes().all(|byte| byte.is_ascii_alphanumeric())
}

/// The offered tools' names on the Chat (`function.name`), Responses and Messages (`name`) wires.
fn tool_names(body: &Value) -> Vec<&str> {
    body["tools"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default()
        .iter()
        .filter_map(|tool| {
            tool["function"]["name"]
                .as_str()
                .or_else(|| tool["name"].as_str())
        })
        .collect()
}

/// Whether the history holds a tool's result: a Chat `tool` message, a Responses `function_call_output` item, or a
/// Messages `tool_result` part.
fn has_tool_result(body: &Value) -> bool {
    let items = body["messages"]
        .as_array()
        .or_else(|| body["input"].as_array())
        .map(Vec::as_slice)
        .unwrap_or_default();
    items.iter().any(|item| {
        item["role"] == "tool"
            || item["type"] == "function_call_output"
            || item["content"]
                .as_array()
                .is_some_and(|parts| parts.iter().any(|part| part["type"] == "tool_result"))
    })
}
