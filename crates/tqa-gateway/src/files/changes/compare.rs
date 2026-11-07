//! What changed between two manifests: one entry per added, modified or deleted file with line counts, and a unified
//! diff of one file from the stored copies. Binary and uncopied files are listed without a diff. Line diffs are
//! bounded by a deadline (answers-068 Q8), after which `similar` returns a coarser but valid diff.
use super::blobs::Blobs;
use super::manifest::{Manifest, Record};
use serde::Serialize;
use similar::{ChangeTag, TextDiff};
use std::{collections::BTreeSet, time::Duration};

#[cfg(test)]
#[path = "compare_tests.rs"]
mod tests;

const DIFF_DEADLINE: Duration = Duration::from_secs(2);
const CONTEXT_LINES: usize = 3;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Status {
    Added,
    Modified,
    Deleted,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct FileChange {
    pub path: String,
    pub status: Status,
    pub added: u64,
    pub removed: u64,
    /// One side is not text: listed as changed without a diff.
    pub binary: bool,
    /// One side was not copied (over the text or copy bound): listed without a diff.
    pub too_large: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct FileDiff {
    pub path: String,
    pub status: Status,
    /// The unified diff, when both sides are text with copies.
    pub unified: Option<String>,
    pub binary: bool,
    pub too_large: bool,
    /// The diff text was cut at the bound.
    pub truncated: bool,
}

/// One side of a change.
enum Side {
    Missing,
    Text(String),
    Binary,
    TooLarge,
}

/// Every changed file, in path order.
pub(crate) fn changes(before: &Manifest, after: &Manifest, blobs: &Blobs) -> Vec<FileChange> {
    let paths: BTreeSet<&String> = before.files.keys().chain(after.files.keys()).collect();
    paths
        .into_iter()
        .filter_map(|path| change(before, after, blobs, path).map(|(change, _)| change))
        .collect()
}

/// The unified diff of one changed file, or `None` when `path` did not change.
pub(crate) fn unified(
    before: &Manifest,
    after: &Manifest,
    blobs: &Blobs,
    path: &str,
    max_bytes: usize,
) -> Option<FileDiff> {
    let (change, sides) = change(before, after, blobs, path)?;
    let mut diff = FileDiff {
        path: change.path,
        status: change.status,
        unified: None,
        binary: change.binary,
        too_large: change.too_large,
        truncated: false,
    };
    if let Some((old, new)) = sides {
        let text = TextDiff::configure()
            .timeout(DIFF_DEADLINE)
            .diff_lines(old.as_str(), new.as_str())
            .unified_diff()
            .context_radius(CONTEXT_LINES)
            .header(&format!("a/{path}"), &format!("b/{path}"))
            .to_string();
        let (text, truncated) = cut(text, max_bytes);
        diff.unified = Some(text);
        diff.truncated = truncated;
    }
    Some(diff)
}

/// Whether `path` is in the workspace's uploads directory, where a new file is always the person's own upload (an
/// upload never overwrites); an edit to one, or its deletion, is still a change.
fn uploaded(path: &str) -> bool {
    path.strip_prefix(crate::files::upload::DIRECTORY)
        .is_some_and(|rest| rest.starts_with('/'))
}

/// The change at `path` and, when both sides are text, their contents.
fn change(
    before: &Manifest,
    after: &Manifest,
    blobs: &Blobs,
    path: &str,
) -> Option<(FileChange, Option<(String, String)>)> {
    let old = before.files.get(path);
    let new = after.files.get(path);
    let status = match (old, new) {
        // The person's own upload, even one that lands after the run: never the run's change.
        (None, Some(_)) if uploaded(path) => return None,
        (None, Some(_)) => Status::Added,
        (Some(_), None) => Status::Deleted,
        (Some(old), Some(new)) if differs(old, new) => Status::Modified,
        _ => return None,
    };
    let (old, new) = (side(old, blobs), side(new, blobs));
    let binary = matches!(old, Side::Binary) || matches!(new, Side::Binary);
    let too_large = !binary && (matches!(old, Side::TooLarge) || matches!(new, Side::TooLarge));
    let mut change = FileChange {
        path: path.to_owned(),
        status,
        added: 0,
        removed: 0,
        binary,
        too_large,
    };
    // Neither binary nor too large: both sides are text or missing.
    let texts = (!binary && !too_large).then(|| (text(old), text(new)));
    if let Some((old, new)) = &texts {
        let diff = TextDiff::configure()
            .timeout(DIFF_DEADLINE)
            .diff_lines(old.as_str(), new.as_str());
        for line in diff.iter_all_changes() {
            match line.tag() {
                ChangeTag::Insert => change.added += 1,
                ChangeTag::Delete => change.removed += 1,
                ChangeTag::Equal => {}
            }
        }
    }
    Some((change, texts))
}

fn differs(old: &Record, new: &Record) -> bool {
    match (&old.hash, &new.hash) {
        (Some(old), Some(new)) => old != new,
        _ => old.size != new.size || old.stamp != new.stamp,
    }
}

fn side(record: Option<&Record>, blobs: &Blobs) -> Side {
    match record {
        None => Side::Missing,
        Some(record) if record.copied => record
            .hash
            .as_deref()
            .and_then(|hash| blobs.get(hash).ok())
            .and_then(|bytes| String::from_utf8(bytes).ok())
            .map_or(Side::TooLarge, Side::Text),
        Some(record) if record.hash.is_some() && !record.text => Side::Binary,
        Some(_) => Side::TooLarge,
    }
}

fn text(side: Side) -> String {
    match side {
        Side::Text(text) => text,
        _ => String::new(),
    }
}

/// Cuts `text` to at most `max_bytes` on a character boundary.
fn cut(mut text: String, max_bytes: usize) -> (String, bool) {
    if text.len() <= max_bytes {
        return (text, false);
    }
    let mut end = max_bytes;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text.truncate(end);
    (text, true)
}
