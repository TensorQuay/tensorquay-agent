//! The TOML parse's refusal as a person can act on it (ENGINEERING-GUARDRAILS §1.5): the field by its path and why, in
//! serde's own words, or a syntax error's line and column; never a value from the file, which may be a secret. The
//! keys it names pass through the diagnostic redaction too (`tqa_types::redact`), so a credential pasted as a key is
//! masked.
use serde::de::DeserializeOwned;

use crate::GatewayError;

#[cfg(test)]
#[path = "toml_error_tests.rs"]
mod tests;

/// A reason longer than this is cut: an `expected one of` list can name every field of a table.
const MAX_REASON_BYTES: usize = 512;
/// A path longer than this, or with any other character, is not repeated.
const MAX_PATH_BYTES: usize = 256;

/// Parses `source` as `T`, refusing with the field's path and the reason, or the syntax error's position.
pub(super) fn parse<T: DeserializeOwned>(source: &str) -> Result<T, GatewayError> {
    let deserializer =
        toml::de::Deserializer::parse(source).map_err(|error| syntax(source, &error))?;
    serde_path_to_error::deserialize(deserializer).map_err(|error| {
        let path = error.path().to_string();
        GatewayError::ConfigurationField(tqa_types::redact(&format!(
            "{}: {}",
            printable(&path),
            reason(error.inner().message())
        )))
    })
}

/// A syntax error: its line and column, and the parser's description, which names only TOML's grammar.
fn syntax(source: &str, error: &toml::de::Error) -> GatewayError {
    let at = error.span().map_or(0, |span| span.start).min(source.len());
    let before = source.get(..at).unwrap_or_default();
    let line = before.matches('\n').count() + 1;
    let column = before
        .rsplit('\n')
        .next()
        .unwrap_or_default()
        .chars()
        .count()
        + 1;
    GatewayError::ConfigurationField(tqa_types::redact(&format!(
        "TOML syntax error at line {line}, column {column}: {}",
        bounded(&without_quoted(error.message()))
    )))
}

/// The path as `serde_path_to_error` writes it (`mcp.servers[0].name`), when it is made of plain names only. It
/// writes the document's root as `.` and a segment it can't name (through a flattened or untagged type) as `?`.
pub(super) fn printable(path: &str) -> &str {
    if path == "." {
        return "the top level";
    }
    if path
        .split('.')
        .any(|segment| segment == "?" || segment.starts_with("?["))
    {
        return "a field whose path is unknown";
    }
    let plain = !path.is_empty()
        && path.len() <= MAX_PATH_BYTES
        && path
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | '[' | ']'));
    if plain {
        path
    } else {
        "a field with an unprintable name"
    }
}

/// serde's message with every value it quotes left out; a message of another shape may hold one, so it is only
/// called invalid.
pub(super) fn reason(message: &str) -> String {
    let text = if let Some(rest) = message.strip_prefix("invalid type: ") {
        found("invalid type", rest)
    } else if let Some(rest) = message.strip_prefix("invalid value: ") {
        found("invalid value", rest)
    } else if let Some(rest) = message.strip_prefix("unknown field `") {
        after_name("unknown field", rest)
    } else if let Some(rest) = message.strip_prefix("unknown variant `") {
        after_name("unknown variant", rest)
    } else if is_plain(message, "missing field `")
        || is_plain(message, "duplicate field `")
        || message.starts_with("invalid length ")
        || message.starts_with("[sandbox.ceiling] ")
    {
        Some(message.to_owned())
    } else {
        None
    };
    bounded(&text.unwrap_or_else(|| String::from("invalid value")))
}

/// `invalid type: string "x", expected u64` as `invalid type: found a string, expected u64`.
fn found(kind: &str, rest: &str) -> Option<String> {
    const KINDS: [(&str, &str); 10] = [
        ("string ", "a string"),
        ("integer ", "an integer"),
        ("floating point ", "a floating point number"),
        ("boolean ", "a boolean"),
        ("character ", "a character"),
        ("sequence,", "an array"),
        ("map,", "a table"),
        ("unit value,", "an empty value"),
        ("byte array,", "bytes"),
        ("enum,", "an enum"),
    ];
    let (_, expected) = rest.rsplit_once(", expected ")?;
    let (_, what) = KINDS.iter().find(|(prefix, _)| rest.starts_with(prefix))?;
    Some(format!("{kind}: found {what}, expected {expected}"))
}

/// `unknown field `x`, expected ...` as `unknown field, expected ...`: the path names the field already. A type with
/// nothing to choose from says so instead (`there are no fields`), and that is kept too.
fn after_name(kind: &str, rest: &str) -> Option<String> {
    for nothing in ["there are no fields", "there are no variants"] {
        if rest.ends_with(&format!("`, {nothing}")) {
            return Some(format!("{kind}, {nothing}"));
        }
    }
    // The last: a quoted variant may itself hold the separator.
    let (_, expected) = rest.rsplit_once("`, expected ")?;
    Some(format!("{kind}, expected {expected}"))
}

/// A message naming one field of the schema, as serde's missing or duplicate field.
fn is_plain(message: &str, prefix: &str) -> bool {
    message
        .strip_prefix(prefix)
        .and_then(|rest| rest.strip_suffix('`'))
        .is_some_and(|name| name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'))
}

/// The text with anything in double quotes left out.
fn without_quoted(text: &str) -> String {
    text.split('"').step_by(2).collect::<Vec<_>>().join("\"…\"")
}

fn bounded(text: &str) -> String {
    if text.len() <= MAX_REASON_BYTES {
        return text.to_owned();
    }
    let mut end = MAX_REASON_BYTES - '…'.len_utf8();
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &text[..end])
}
