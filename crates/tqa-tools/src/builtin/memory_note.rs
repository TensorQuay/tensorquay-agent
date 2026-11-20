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

/// Where a note's previous version is kept for one Undo, relative to the workspace root: hidden, so the index and
/// the agent's listing of its notes never count it.
pub const UNDO_DIRECTORY: &str = ".memory/.undo";

/// Before a note directly under `.memory/` is replaced: its current content is copied to [`UNDO_DIRECTORY`] under the
/// same name, so one Undo can put it back; a note about to be created gets any stale copy removed, so its Undo
/// removes it. Other paths (elsewhere in the workspace, digests under `.memory/tasks/`) are left alone.
///
/// # Errors
/// The copy's I/O failure: a note isn't replaced when its previous version couldn't be kept.
pub fn keep_previous(root: &Path, path: &Path) -> std::io::Result<()> {
    let Some(name) = note_name(root, path) else {
        return Ok(());
    };
    let undo = root.join(UNDO_DIRECTORY);
    let kept = undo.join(name);
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_file() => {
            std::fs::create_dir_all(&undo)?;
            std::fs::copy(path, &kept).map(|_| ())
        }
        _ => match std::fs::remove_file(&kept) {
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(error),
            _ => Ok(()),
        },
    }
}

/// The file name of a note directly under `.memory/` (a regular `*.md`, not hidden); `None` for any other path.
#[must_use]
pub fn note_name<'a>(root: &Path, path: &'a Path) -> Option<&'a std::ffi::OsStr> {
    let relative = path.strip_prefix(root).ok()?;
    let mut parts = relative.components();
    let first = parts.next()?;
    let name = parts.next()?;
    if parts.next().is_some() || first.as_os_str() != MEMORY_DIRECTORY {
        return None;
    }
    let name = name.as_os_str();
    let text = name.to_str()?;
    let markdown = Path::new(text)
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("md"));
    (markdown && !text.starts_with('.')).then_some(name)
}

#[cfg(test)]
#[path = "memory_note_tests.rs"]
mod tests;
