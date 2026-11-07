//! The fixed JSON repairs of ADR-0007, applied only outside strings and only when the result parses.
use serde_json::{Map, Value};
use tqa_types::FallbackProblem;

/// Reads one JSON object, trying the text as written first and then the repaired text: single-quoted
/// strings become double-quoted (rule 2), bare keys are quoted (rule 2) and trailing commas before
/// `}` or `]` are dropped (rule 1).
pub(super) fn object(text: &str) -> Option<Map<String, Value>> {
    let parse = |candidate: &str| match serde_json::from_str::<Value>(candidate.trim()) {
        Ok(Value::Object(map)) => Some(map),
        _ => None,
    };
    parse(text).or_else(|| parse(&trailing_commas(&bare_keys(&double_quotes(text)))))
}

/// What is wrong with a block [`object`] can't read, from the text as written (the repairs only ever help): JSON that
/// isn't an object, or where the parser stopped, as a cut (the text ended first) or a syntax error, by line and column
/// counted from the block's first line, blank ones included (the parser skips leading space and counts its lines).
pub(super) fn problem(text: &str) -> FallbackProblem {
    match serde_json::from_str::<Value>(text) {
        Ok(_) => FallbackProblem::NotObject,
        Err(error) => FallbackProblem::NotJson {
            cut_off: error.is_eof(),
            line: u32::try_from(error.line()).unwrap_or(u32::MAX),
            column: u32::try_from(error.column()).unwrap_or(u32::MAX),
        },
    }
}

/// Rewrites `'...'` strings outside double-quoted strings as JSON strings.
fn double_quotes(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        match c {
            '"' => {
                out.push('"');
                copy_string(&mut chars, &mut out);
            }
            '\'' => copy_single(&mut chars, &mut out),
            other => out.push(other),
        }
    }
    out
}

/// Quotes identifier keys that follow `{` or `,` and precede `:`.
fn bare_keys(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    let mut expect_key = false;
    while let Some(c) = chars.next() {
        match c {
            '"' => {
                out.push('"');
                copy_string(&mut chars, &mut out);
                expect_key = false;
            }
            '{' | ',' => {
                out.push(c);
                expect_key = true;
            }
            c if expect_key && (c.is_ascii_alphabetic() || c == '_') => {
                let key = identifier(c, &mut chars);
                // Only a name followed by `:` is a key; `true` or `null` in an array stays a value.
                if chars.clone().find(|next| !next.is_whitespace()) == Some(':') {
                    out.push('"');
                    out.push_str(&key);
                    out.push('"');
                } else {
                    out.push_str(&key);
                }
                expect_key = false;
            }
            c if c.is_whitespace() => out.push(c),
            other => {
                out.push(other);
                expect_key = false;
            }
        }
    }
    out
}

/// Drops a comma whose next non-space character closes an object or array.
fn trailing_commas(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' => {
                out.push('"');
                copy_string(&mut chars, &mut out);
            }
            ',' => {
                // The lookahead skips only whitespace, so the scan stays linear.
                let next = chars.clone().find(|next| !next.is_whitespace());
                if !matches!(next, Some('}' | ']')) {
                    out.push(',');
                }
            }
            other => out.push(other),
        }
    }
    out
}

/// Rewrites the rest of a single-quoted string as a double-quoted one.
fn copy_single(chars: &mut impl Iterator<Item = char>, out: &mut String) {
    out.push('"');
    while let Some(inner) = chars.next() {
        match inner {
            '\'' => break,
            '"' => out.push_str("\\\""),
            '\\' => out.extend([Some('\\'), chars.next()].into_iter().flatten()),
            other => out.push(other),
        }
    }
    out.push('"');
}

/// Reads an identifier that starts with `first`.
fn identifier(first: char, chars: &mut std::iter::Peekable<std::str::Chars<'_>>) -> String {
    let mut key = String::from(first);
    while let Some(next) = chars.next_if(|next| next.is_ascii_alphanumeric() || *next == '_') {
        key.push(next);
    }
    key
}

/// Copies the rest of a double-quoted string, including its closing quote and escapes.
fn copy_string(chars: &mut impl Iterator<Item = char>, out: &mut String) {
    while let Some(c) = chars.next() {
        out.push(c);
        match c {
            '\\' => {
                if let Some(escaped) = chars.next() {
                    out.push(escaped);
                }
            }
            '"' => return,
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::object;

    #[test]
    fn repairs_keep_string_content_intact() {
        let repaired = object("{name: 'a, }', 'q': \"it's\", list: [1, true, null,],}");
        let value = serde_json::Value::Object(repaired.unwrap_or_default());
        assert_eq!(
            value,
            serde_json::json!({"name": "a, }", "q": "it's", "list": [1, true, null]})
        );
        assert!(object("{\"a\": }").is_none());
        assert!(object("[1, 2]").is_none());
    }
}
