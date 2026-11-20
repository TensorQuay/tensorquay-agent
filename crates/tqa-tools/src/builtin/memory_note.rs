//! Long-term memory's guard on saves (ADR-0041): a note under `.memory/` is kept across tasks and listed in every
//! run's index, so it may not hold a key or token. Every file tool writes through `atomic_write::replace`, which asks
//! here first. What a secret looks like is the product's one definition (`tqa_types::is_key_shaped_name`).
use std::path::Path;

/// The workspace's notes, relative to its root.
pub const MEMORY_DIRECTORY: &str = ".memory";
/// A candidate shorter than this is never a key (`key_shapes.json`'s shortest body).
const MIN_KEY_CHARS: usize = 20;

/// The reason the model is given when a note is refused.
pub const KEY_IN_NOTE: &str = "a memory note can't hold a key, token or password: notes are kept and shown to every \
later task. Save where the key is kept (its name, or the Keys page), never its value. A long random-looking id counts \
too: shorten a commit id to its first 9 characters";

/// Whether `text` holds something shaped like a key or token: a run of letters, digits, `-` and `_` that the product's
/// key shapes match.
#[must_use]
pub fn holds_key(text: &str) -> bool {
    text.split(|c: char| !(c.is_ascii_alphanumeric() || c == '-' || c == '_'))
        .any(|run| run.len() >= MIN_KEY_CHARS && tqa_types::is_key_shaped_name(run))
}

/// Why `content` may not be written to `path` (already resolved inside `root`); `None` when it may.
#[must_use]
pub fn refusal(root: &Path, path: &Path, content: &str) -> Option<&'static str> {
    let relative = path.strip_prefix(root).ok()?;
    let in_memory = relative
        .components()
        .next()
        .is_some_and(|first| first.as_os_str() == MEMORY_DIRECTORY);
    (in_memory && holds_key(content)).then_some(KEY_IN_NOTE)
}

#[cfg(test)]
#[path = "memory_note_tests.rs"]
mod tests;
