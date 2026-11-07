//! The untrusted-content marker (ADR-0014 point 10): what a tool returns from outside the conversation (a web page, a
//! channel message) goes in a delimited block the model is told is data, never instructions.
//!
//! The closing tag carries an id derived from the source, the attributes and the body, so a body can't end the block:
//! it would have to contain its own hash. A body can contain text that looks like another opening tag, or a closing
//! tag with some other id; only the closing tag with the block's id ends it. The same inputs always give the same
//! block, so tool output stays deterministic.

use crate::ToolError;
use sha2::{Digest, Sha256};

/// The block's tag name.
const TAG: &str = "untrusted-content";

/// Wraps `body` in an untrusted-content block with its `source` (for example `web` or `channel`) and `attributes`, in
/// the order given. The body is kept verbatim; attribute values are escaped.
///
/// # Errors
/// Refuses a source or an attribute name outside `[a-z_]{1,32}`, so a caller can't add an attribute by accident.
pub fn wrap(source: &str, attributes: &[(&str, &str)], body: &str) -> Result<String, ToolError> {
    marked(TAG, source, attributes, body)
}

/// Wraps a signed-in person's words, as an agent team's `person_answer` or `person_message` reaches the lead (ADR-0034,
/// PROTO-U-023): marked as a person's, never as an agent's untrusted content, and closed the same way, so a body can't
/// end the block.
///
/// # Errors
/// As [`wrap`].
pub fn person(attributes: &[(&str, &str)], body: &str) -> Result<String, ToolError> {
    marked(PERSON_TAG, "person", attributes, body)
}

/// A signed-in person's block's tag name.
const PERSON_TAG: &str = "signed-in-person";

fn marked(
    tag: &str,
    source: &str,
    attributes: &[(&str, &str)],
    body: &str,
) -> Result<String, ToolError> {
    if !valid_name(source) || attributes.iter().any(|(name, _)| !valid_name(name)) {
        return Err(ToolError::invalid(
            "",
            "untrusted-content names are 1 to 32 lower-case letters or underscores",
        ));
    }
    let id = block_id(source, attributes, body);
    let mut open = format!("<{tag} source=\"{source}\"");
    for (name, value) in attributes {
        open.push(' ');
        open.push_str(name);
        open.push_str("=\"");
        open.push_str(&escape(value));
        open.push('"');
    }
    Ok(format!("{open} id=\"{id}\">\n{body}\n</{tag} id=\"{id}\">"))
}

fn valid_name(name: &str) -> bool {
    (1..=32).contains(&name.len())
        && name
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte == b'_')
}

/// An attribute value that can't end its quotes or the tag, or break the line.
fn escape(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '&' => escaped.push_str("&amp;"),
            '"' => escaped.push_str("&quot;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '\n' => escaped.push_str("&#10;"),
            '\r' => escaped.push_str("&#13;"),
            other => escaped.push(other),
        }
    }
    escaped
}

/// The first 16 hex digits of a hash over the length-prefixed source, attributes and body.
fn block_id(source: &str, attributes: &[(&str, &str)], body: &str) -> String {
    let mut hash = Sha256::new();
    let mut part = |bytes: &[u8]| {
        hash.update((bytes.len() as u64).to_be_bytes());
        hash.update(bytes);
    };
    part(source.as_bytes());
    for (name, value) in attributes {
        part(name.as_bytes());
        part(value.as_bytes());
    }
    part(body.as_bytes());
    hash.finalize()
        .iter()
        .take(8)
        .fold(String::with_capacity(16), |mut id, byte| {
            id.push(char::from_digit(u32::from(byte >> 4), 16).unwrap_or('0'));
            id.push(char::from_digit(u32::from(byte & 0x0f), 16).unwrap_or('0'));
            id
        })
}
