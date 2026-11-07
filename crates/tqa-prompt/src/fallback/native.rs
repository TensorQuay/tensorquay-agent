//! Calls written in a model's native markup instead of a fenced `tqa_call` block (PROMPT-U-008, Gate 5 on 29 Sep).
//! Where the server has no tool-call parser of its own, such markup arrives as text: the XML form
//! (`<function=name>` with `<parameter=key>value</parameter>`, inside `<tool_call>` or not) and the JSON form
//! (`<tool_call>{"name": …, "arguments": {…}}</tool_call>`, or `<function-call>` or `<function_call>` around the same:
//! qwen3:4b on Ollama, 30 Sep). All or nothing: each piece must be a known tool with valid arguments, or the reply stays
//! prose as it was.
use super::{FallbackCall, ToolCheck};
use serde_json::{Map, Value};

/// The wrappers around one call, opening and closing.
const WRAPPERS: [(&str, &str); 3] = [
    ("<tool_call>", "</tool_call>"),
    ("<function-call>", "</function-call>"),
    ("<function_call>", "</function_call>"),
];
const FUNCTION_OPEN: &str = "<function=";
const FUNCTION_CLOSE: &str = "</function>";
const PARAMETER_OPEN: &str = "<parameter=";
const PARAMETER_CLOSE: &str = "</parameter>";

/// Every tag the markup uses; a value that holds one would have been cut at it, so it is refused.
pub(super) const TAGS: [&str; 10] = [
    WRAPPERS[0].0,
    WRAPPERS[0].1,
    WRAPPERS[1].0,
    WRAPPERS[1].1,
    WRAPPERS[2].0,
    WRAPPERS[2].1,
    FUNCTION_OPEN,
    FUNCTION_CLOSE,
    PARAMETER_OPEN,
    PARAMETER_CLOSE,
];

/// The narration without the markup, and the calls it held; `None` when there is none, or any piece isn't a call.
/// Only a trailing run of markup outside fenced blocks is read: from its first piece on, only markup and whitespace
/// may follow, so markup quoted in an explanation or shown in a code block stays prose.
pub(super) fn recover(
    text: &str,
    check: &dyn Fn(&str, &Value) -> ToolCheck,
) -> Option<(String, Vec<FallbackCall>)> {
    let start = first_outside_fences(text)?;
    let mut calls = Vec::new();
    let mut rest = &text[start..];
    loop {
        rest = rest.trim_start();
        if rest.is_empty() {
            break;
        }
        let wrapper = WRAPPERS.iter().find(|(open, _)| rest.starts_with(open));
        let (call, used) = if let Some((open, close)) = wrapper {
            let end = rest.find(close)?;
            (wrapped(&rest[open.len()..end], check)?, end + close.len())
        } else if rest.starts_with(FUNCTION_OPEN) {
            let end = rest.find(FUNCTION_CLOSE)?;
            (
                function(&rest[..end + FUNCTION_CLOSE.len()], check)?,
                end + FUNCTION_CLOSE.len(),
            )
        } else {
            // Anything but more markup after the first piece: not a trailing run.
            return None;
        };
        calls.push(call);
        rest = &rest[used..];
    }
    Some((text[..start].to_owned(), calls))
}

/// Where the first piece of markup starts outside triple-backtick fences, if anywhere.
fn first_outside_fences(text: &str) -> Option<usize> {
    let mut fenced = false;
    let mut offset = 0;
    for line in text.split_inclusive('\n') {
        if line.trim_start().starts_with("```") {
            fenced = !fenced;
        } else if !fenced {
            let found = WRAPPERS
                .iter()
                .map(|(open, _)| line.find(open))
                .chain([line.find(FUNCTION_OPEN)])
                .flatten()
                .min();
            if let Some(at) = found {
                return Some(offset + at);
            }
        }
        offset += line.len();
    }
    None
}

/// Whether a reply plainly ends in call markup, outside fenced blocks: its last line closes a call, or a line starts
/// with a call's opening tag. Markup named mid-sentence, with prose after it, or shown in a fence, is not.
pub(super) fn looks_like_call(text: &str) -> bool {
    let mut fenced = false;
    let mut last = "";
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("```") {
            fenced = !fenced;
            continue;
        }
        if fenced || trimmed.is_empty() {
            continue;
        }
        let opens = WRAPPERS.iter().any(|(open, _)| trimmed.starts_with(open))
            || trimmed.starts_with(FUNCTION_OPEN);
        if opens {
            return true;
        }
        last = trimmed;
    }
    WRAPPERS
        .iter()
        .map(|(_, close)| *close)
        .chain([FUNCTION_CLOSE])
        .any(|close| last.ends_with(close))
}

/// The inside of a wrapper (`<tool_call>…</tool_call>` and the like): JSON with `name` and `arguments`, or one XML function.
fn wrapped(inner: &str, check: &dyn Fn(&str, &Value) -> ToolCheck) -> Option<FallbackCall> {
    let inner = inner.trim();
    if inner.starts_with(FUNCTION_OPEN) {
        return function(inner, check);
    }
    let value: Value = serde_json::from_str(inner).ok()?;
    let name = value.get("name")?.as_str()?;
    let arguments = value.get("arguments")?.clone();
    valid(name, arguments, check)
}

/// `<function=name>` with its parameters, up to and including `</function>`.
fn function(piece: &str, check: &dyn Fn(&str, &Value) -> ToolCheck) -> Option<FallbackCall> {
    let after = piece.strip_prefix(FUNCTION_OPEN)?;
    let close = after.find('>')?;
    let name = after[..close].trim();
    let mut body = after[close + 1..].strip_suffix(FUNCTION_CLOSE)?;
    let mut texts = Map::new();
    let mut typed = Map::new();
    while let Some(start) = body.find(PARAMETER_OPEN) {
        // Only whitespace between parameters.
        if !body[..start].trim().is_empty() {
            return None;
        }
        let after = &body[start + PARAMETER_OPEN.len()..];
        let close = after.find('>')?;
        let key = after[..close].trim().to_owned();
        let value_and_rest = &after[close + 1..];
        let end = value_and_rest.find(PARAMETER_CLOSE)?;
        let value = value_and_rest[..end].trim_matches('\n');
        if TAGS.iter().any(|tag| value.contains(tag)) {
            return None;
        }
        texts.insert(key.clone(), Value::String(value.to_owned()));
        typed.insert(
            key,
            serde_json::from_str(value).unwrap_or_else(|_| Value::String(value.to_owned())),
        );
        body = &value_and_rest[end + PARAMETER_CLOSE.len()..];
    }
    // Nothing but whitespace may follow the last parameter: a leftover closing tag means a value was cut short.
    if !body.trim().is_empty() {
        return None;
    }
    // Typed where the tool accepts it that way, otherwise every value as text.
    valid(name, Value::Object(typed), check).or_else(|| valid(name, Value::Object(texts), check))
}

fn valid(
    name: &str,
    arguments: Value,
    check: &dyn Fn(&str, &Value) -> ToolCheck,
) -> Option<FallbackCall> {
    (check(name, &arguments) == ToolCheck::Valid).then(|| FallbackCall {
        name: name.to_owned(),
        arguments,
    })
}
