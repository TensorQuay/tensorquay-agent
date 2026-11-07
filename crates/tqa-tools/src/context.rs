//! Explicit execution resources and cooperative I/O boundaries.
use crate::{ToolError, WorkspaceRoot};
use std::{future::Future, sync::Arc, time::Duration};
use tokio_util::sync::CancellationToken;
use tqa_types::{Clock, Sleeper};

/// Hard read and search output ceiling, including continuation notices.
pub const READ_PAGE_BYTES: usize = 8 * 1024;
/// Maximum source file size accepted by the paged reader.
pub const MAX_READ_FILE_BYTES: u64 = 64 * 1024 * 1024;
/// Minimum space for page content and continuation metadata.
pub const MIN_PAGE_BYTES: usize = 512;
// Explicit pool sizes are bounded independently of the automatic eight-worker recommendation.
const MAX_SEARCH_THREADS: u16 = 64;

/// Explicit resource ceilings; callers choose these before dispatch.
#[derive(Clone, Copy, Debug)]
pub struct ToolLimits {
    /// Maximum shell run duration, including pipe collection.
    pub shell_timeout: Duration,
    /// Maximum shell response bytes, including markers.
    pub shell_output_bytes: usize,
    /// Maximum source and replacement bytes for an edit.
    pub file_read_bytes: usize,
    /// Maximum source lines in a read page.
    pub read_page_lines: usize,
    /// Read output bytes, including line numbers and notices.
    pub read_page_bytes: usize,
    /// Search output bytes, including notices.
    pub search_page_bytes: usize,
    /// Search workers: zero chooses available parallelism capped at eight; explicit values are 1-64.
    pub search_threads: u16,
    /// Maximum files examined by a search call.
    pub search_max_files: u64,
    /// Maximum file bytes examined by a search call.
    pub search_max_bytes: u64,
    /// Maximum elapsed traversal and search duration.
    pub search_max_elapsed: Duration,
    /// Maximum elapsed traversal duration of a glob call; shorter than a content search, since glob only lists
    /// paths (TOOL-E-016).
    pub glob_max_elapsed: Duration,
    /// Maximum glob paths returned.
    pub glob_results: usize,
    /// Maximum grep results returned in one page.
    pub grep_results: usize,
    /// Maximum duration of a filesystem tool call.
    pub file_timeout: Duration,
    /// Maximum visited directory entries per glob call.
    pub glob_entries: usize,
    /// Maximum bytes in each ignore file.
    pub ignore_file_bytes: usize,
}
impl ToolLimits {
    /// Recommended limits: 200 read lines, 8 KiB pages, 200000 files, 512 MiB
    /// scanned bytes, 5 seconds per content search and 2 seconds per glob; shell and edit caps are separate.
    pub const RECOMMENDED: Self = Self {
        shell_timeout: Duration::from_secs(30),
        shell_output_bytes: 1024 * 1024,
        file_read_bytes: 1024 * 1024,
        read_page_lines: 200,
        read_page_bytes: READ_PAGE_BYTES,
        search_page_bytes: READ_PAGE_BYTES,
        search_threads: 0,
        search_max_files: 200_000,
        search_max_bytes: 512 * 1024 * 1024,
        search_max_elapsed: Duration::from_secs(5),
        glob_max_elapsed: Duration::from_secs(2),
        glob_results: 1000,
        grep_results: 1000,
        file_timeout: Duration::from_secs(30),
        glob_entries: 250_000,
        ignore_file_bytes: 64 * 1024,
    };
    /// Rejects disabled resource bounds.
    ///
    /// # Errors
    /// Returns `InvalidArgs` for disabled bounds, pages outside 512-8192 or more than 64 workers.
    pub fn validate(&self) -> Result<(), ToolError> {
        if self.search_threads > MAX_SEARCH_THREADS {
            return Err(ToolError::invalid(
                "/limits/search_threads",
                "search_threads must be between 0 and 64",
            ));
        }
        if self.shell_timeout.is_zero()
            || self.file_timeout.is_zero()
            || self.search_max_elapsed.is_zero()
            || self.glob_max_elapsed.is_zero()
            || self.search_max_files == 0
            || self.search_max_bytes == 0
            || [
                self.shell_output_bytes,
                self.file_read_bytes,
                self.read_page_lines,
                self.glob_results,
                self.grep_results,
                self.glob_entries,
                self.ignore_file_bytes,
            ]
            .contains(&0)
        {
            return Err(ToolError::invalid("/limits", "limits must be positive"));
        }
        if !(MIN_PAGE_BYTES..=READ_PAGE_BYTES).contains(&self.read_page_bytes)
            || !(MIN_PAGE_BYTES..=READ_PAGE_BYTES).contains(&self.search_page_bytes)
        {
            return Err(ToolError::invalid(
                "/limits",
                "page bytes must be between 512 and 8192",
            ));
        }
        Ok(())
    }
}

/// Per-call capabilities, with no implicit host configuration.
#[derive(Clone)]
pub struct ToolContext {
    /// Canonical workspace boundary for filesystem tools and shell cwd.
    pub workspace: WorkspaceRoot,
    /// Cancellation observed before and during operations.
    pub cancel: CancellationToken,
    /// Monotonic clock used for deadlines.
    pub clock: Arc<dyn Clock>,
    /// Injected waits used by deadline and process cleanup paths.
    pub sleeper: Arc<dyn Sleeper>,
    /// Additional environment variable names copied into shell children.
    pub env_allow: Vec<String>,
    /// Explicit operation and traversal ceilings.
    pub limits: ToolLimits,
}
impl ToolContext {
    pub(crate) fn check(&self) -> Result<(), ToolError> {
        self.limits.validate()?;
        if self
            .clock
            .now()
            .checked_add(self.limits.file_timeout)
            .is_none()
            || self
                .clock
                .now()
                .checked_add(self.limits.shell_timeout)
                .is_none()
            || self
                .clock
                .now()
                .checked_add(self.limits.search_max_elapsed)
                .is_none()
        {
            return Err(ToolError::invalid("/limits", "deadline overflow"));
        }
        if self.cancel.is_cancelled() {
            return Err(ToolError::Cancelled);
        }
        Ok(())
    }
    pub(crate) async fn io<T>(
        &self,
        future: impl Future<Output = Result<T, ToolError>>,
    ) -> Result<T, ToolError> {
        self.check()?;
        tokio::select! {
            biased;
            () = self.cancel.cancelled() => Err(ToolError::Cancelled),
            () = self.sleeper.sleep(self.limits.file_timeout) => Err(ToolError::Timeout),
            result = tokio::time::timeout(self.limits.file_timeout, future) => result.map_err(|_| ToolError::Timeout)?,
        }
    }
}
