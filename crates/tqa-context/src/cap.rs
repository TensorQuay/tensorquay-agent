//! UTF-8 head/tail previews backed by private, atomically replaced spill files.
use crate::ContextError;
#[cfg(unix)]
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};
use tqa_types::{ConversationId, ToolCallId};

/// The smallest cap the window imposes on a tool result: the head, the tail and the spill notice always fit, and it
/// equals [`cap_tool_result`]'s own lower bound (CTX-E-004).
pub const MIN_RESULT_BYTES: usize = 1024;
/// The share (numerator, denominator) of the input capacity still free when a result is recorded that the result
/// may take, so parallel results and the next request still fit (CTX-E-004).
pub const MAX_RESULT_WINDOW_SHARE: (u64, u64) = (1, 2);

/// The byte cap for a tool result recorded when `needed` of `capacity` input tokens are already taken: the smaller
/// of `limit` and the free share in bytes (the inverse of the byte estimator), and never below
/// [`MIN_RESULT_BYTES`].
#[must_use]
pub fn window_result_cap(limit: usize, needed: u64, capacity: u32) -> usize {
    let (numerator, denominator) = MAX_RESULT_WINDOW_SHARE;
    let share = u64::from(capacity).saturating_sub(needed) * numerator / denominator;
    // The estimator counts ceil(bytes * 2 / 7) tokens, so this many bytes cost at most `share` tokens.
    let bytes = usize::try_from(share.saturating_mul(7) / 2).unwrap_or(usize::MAX);
    limit.min(bytes).max(MIN_RESULT_BYTES)
}

/// A bounded preview and the optional complete spill file.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CappedResult {
    /// Unchanged text or a head/marker/tail preview within the byte limit.
    pub content: String,
    /// The full spill path when truncation occurred.
    pub spilled: Option<PathBuf>,
}

/// Caps tool text without splitting UTF-8; spills only when needed.
/// The caller supplies a trusted directory ending in `.tqa/<conversation>/spill`.
/// This performs bounded synchronous I/O and should run in the runtime's blocking worker.
///
/// # Errors
/// Rejects limits below 1024, invalid spill suffixes, and storage failures.
pub fn cap_tool_result(
    call_id: &ToolCallId,
    text: &str,
    max_bytes: usize,
    spill_dir: &Path,
) -> Result<CappedResult, ContextError> {
    if max_bytes < MIN_RESULT_BYTES {
        return Err(ContextError::InvalidLimit);
    }
    if text.len() <= max_bytes {
        return Ok(CappedResult {
            content: text.into(),
            spilled: None,
        });
    }
    let conversation = conversation(spill_dir)?;
    let relative = format!(".tqa/{conversation}/spill/{call_id}.txt");
    let marker = format!(
        "\n[output truncated: {} bytes total; full output saved to {relative}]\n",
        text.len()
    );
    let available = max_bytes - marker.len();
    let mut head_end = available.div_ceil(2);
    while !text.is_char_boundary(head_end) {
        head_end -= 1;
    }
    let mut tail_start = text.len() - available / 2;
    while !text.is_char_boundary(tail_start) {
        tail_start += 1;
    }
    let content = format!("{}{marker}{}", &text[..head_end], &text[tail_start..]);
    let path = spill_dir.join(format!("{call_id}.txt"));
    write_spill(spill_dir, &path, text)?;
    Ok(CappedResult {
        content,
        spilled: Some(path),
    })
}
fn conversation(directory: &Path) -> Result<ConversationId, ContextError> {
    let parent = directory.parent().ok_or(ContextError::InvalidSpillPath)?;
    if directory.file_name().is_none_or(|name| name != "spill")
        || parent
            .parent()
            .and_then(Path::file_name)
            .is_none_or(|name| name != ".tqa")
    {
        return Err(ContextError::InvalidSpillPath);
    }
    parent
        .file_name()
        .and_then(|name| name.to_str())
        .and_then(|name| name.parse().ok())
        .ok_or(ContextError::InvalidSpillPath)
}
fn write_spill(directory: &Path, path: &Path, text: &str) -> Result<(), ContextError> {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    builder.mode(0o700);
    builder.create(directory)?;
    let stage = directory.join(format!(
        ".spill-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    options.mode(0o600);
    let mut file = options.open(&stage)?;
    let result = (|| {
        file.write_all(text.as_bytes())?;
        file.sync_data()?;
        fs::rename(&stage, path)
    })();
    if result.is_err() {
        fs::remove_file(&stage)?;
    }
    Ok(result?)
}
