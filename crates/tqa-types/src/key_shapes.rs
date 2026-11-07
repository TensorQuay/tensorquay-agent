//! Secret names that look like a key themselves (SEC-033): a key pasted where a name belongs. The shapes are data,
//! `key_shapes.json`, the one source the web reads too, so the gateway and the page can't diverge; see its `$comment`
//! for the rule. Kept conservative: a refused real name is worse than a missed key.
use std::sync::OnceLock;

use serde::Deserialize;

/// The shapes' source, shared with the web.
pub const KEY_SHAPES_JSON: &str = include_str!("key_shapes.json");

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct Shapes {
    #[serde(rename = "$comment")]
    _comment: String,
    prefixes: Vec<KeyPrefix>,
    runs: Vec<Run>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct KeyPrefix {
    prefix: String,
    length: usize,
    #[serde(default)]
    characters: Characters,
    #[serde(default)]
    change_percent: usize,
}

#[derive(Clone, Copy, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Characters {
    /// Letters, digits, `-` and `_`.
    #[default]
    Token,
    /// Upper-case letters and digits.
    UpperDigit,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Run {
    length: usize,
    classes: usize,
    change_percent: usize,
}

/// The shapes, read once. A source that doesn't parse leaves nothing key-shaped (the unit tests keep it parsing).
fn shapes() -> &'static Shapes {
    static SHAPES: OnceLock<Shapes> = OnceLock::new();
    SHAPES.get_or_init(|| serde_json::from_str(KEY_SHAPES_JSON).unwrap_or_default())
}

/// A letter's or digit's class: digit, upper-case or lower-case; none for anything else.
fn class(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(0),
        b'A'..=b'Z' => Some(1),
        b'a'..=b'z' => Some(2),
        _ => None,
    }
}

/// Whether `run` (letters and digits only) changes class at at least `percent` % of its neighbouring places.
fn changes_often(run: &[u8], percent: usize) -> bool {
    let places = run.len().saturating_sub(1);
    let changes = run
        .windows(2)
        .filter(|pair| class(pair[0]) != class(pair[1]))
        .count();
    changes * 100 >= percent * places
}

/// How many of the three classes `run` holds.
fn classes(run: &[u8]) -> usize {
    let mut seen = [false; 3];
    for class in run.iter().filter_map(|byte| class(*byte)) {
        seen[usize::from(class)] = true;
    }
    seen.iter().filter(|seen| **seen).count()
}

fn prefixed(name: &str, prefix: &KeyPrefix) -> bool {
    let Some(rest) = name.strip_prefix(prefix.prefix.as_str()) else {
        return false;
    };
    let body: Vec<u8> = rest
        .bytes()
        .take_while(|byte| match prefix.characters {
            Characters::Token => byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'),
            Characters::UpperDigit => byte.is_ascii_uppercase() || byte.is_ascii_digit(),
        })
        .collect();
    let letters: Vec<u8> = body
        .iter()
        .copied()
        .filter(u8::is_ascii_alphanumeric)
        .collect();
    body.len() >= prefix.length && changes_often(&letters, prefix.change_percent)
}

/// Whether `name` (with or without `secrets/`) looks like a key rather than a name: a credential's own prefix followed
/// by a key's body, or a long run of letters and digits that changes character class as often as a random one does
/// (`key_shapes.json`). A shared secret may not be named so, and no route, connector or engine may name one (SEC-033).
#[must_use]
pub fn is_key_shaped_name(name: &str) -> bool {
    let name = name.strip_prefix("secrets/").unwrap_or(name);
    let shapes = shapes();
    if shapes.prefixes.iter().any(|prefix| prefixed(name, prefix)) {
        return true;
    }
    name.as_bytes()
        .split(|byte| !byte.is_ascii_alphanumeric())
        .any(|run| {
            shapes.runs.iter().any(|shape| {
                run.len() >= shape.length
                    && classes(run) >= shape.classes
                    && changes_often(run, shape.change_percent)
            })
        })
}

#[cfg(test)]
#[path = "key_shapes_tests.rs"]
mod tests;
