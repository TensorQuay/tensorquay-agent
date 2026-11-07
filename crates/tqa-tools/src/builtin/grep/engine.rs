//! One reusable streaming searcher per worker; file classification precedes merging.
use super::{
    large_files::LargeFiles,
    pool::Query,
    sink::{Capture, FileSink},
};
use crate::{
    SearchStopReason, ToolError,
    builtin::{
        file_io,
        search::budget::{Budget, Reader},
    },
};
use grep_matcher::{LineTerminator, Matcher};
use grep_searcher::{BinaryDetection, Searcher, SearcherBuilder};
use std::path::Path;
use tokio_util::sync::CancellationToken;

// Small files fit in the initial streaming buffer. Reset after larger files so adaptive
// buffer growth cannot change the next file's read accounting across worker assignments.
const REUSABLE_FILE_BYTES: u64 = 8 * 1024;
// Larger inputs serialize buffer growth while ordinary source files remain parallel.
pub(super) const LARGE_FILE_BYTES: u64 = 8 * 1024 * 1024;

pub(super) struct Scanned {
    pub(super) sink: FileSink,
    pub(super) bytes: u64,
    pub(super) reason: Option<SearchStopReason>,
}

pub(super) struct Engine<'a> {
    query: &'a Query<'a>,
    searcher: Searcher,
    builder: SearcherBuilder,
    capture: Capture,
    cancel: &'a CancellationToken,
    large_files: &'a LargeFiles,
}
impl<'a> Engine<'a> {
    pub(super) fn new(
        query: &'a Query<'a>,
        capture: Capture,
        cancel: &'a CancellationToken,
        large_files: &'a LargeFiles,
    ) -> Self {
        let context = usize::from(query.args.context_lines.unwrap_or(0));
        let mut builder = SearcherBuilder::new();
        builder
            .line_terminator(
                query
                    .matcher
                    .line_terminator()
                    .unwrap_or(LineTerminator::byte(b'\n')),
            )
            .line_number(true)
            .before_context(context)
            .after_context(context)
            .binary_detection(BinaryDetection::quit(0))
            .bom_sniffing(false)
            .heap_limit(Some(
                usize::try_from(query.cx.limits.search_max_bytes.saturating_add(1))
                    .unwrap_or(usize::MAX),
            ));
        Self {
            query,
            searcher: builder.build(),
            builder,
            capture,
            cancel,
            large_files,
        }
    }
    pub(super) fn file(&mut self, path: &Path, allowance: u64) -> Result<Scanned, ToolError> {
        file_io::cancelled(self.cancel)?;
        let (file, metadata) = file_io::open_regular(path)?;
        let relative = path
            .strip_prefix(self.query.cx.workspace.path())
            .ok()
            .and_then(|path| path.to_str())
            .ok_or_else(|| ToolError::PathInvalid("expected a workspace UTF-8 path".into()))?;
        let mut budget = Budget::new(self.query.cx, self.cancel, self.query.started);
        // The claim's allowance includes only bytes unspent by the ordered merger.
        let spent = self.query.cx.limits.search_max_bytes - allowance;
        budget.bytes = spent;
        let mut sink = FileSink::new(&budget, self.query.args, relative, self.capture);
        // Reader never reads past the opened file's metadata length, even if it grows.
        // Hold the permit until the expanded searcher has been reset, on every result path.
        let _permit = if metadata.len() > LARGE_FILE_BYTES {
            let Some(permit) = self.large_files.acquire(
                &mut budget,
                #[cfg(test)]
                self.query.probe,
            )?
            else {
                return Ok(Scanned {
                    sink,
                    bytes: 0,
                    reason: budget.reason,
                });
            };
            Some(permit)
        } else {
            None
        };
        #[cfg(test)]
        let _scan = self
            .query
            .probe
            .filter(|_| metadata.len() > LARGE_FILE_BYTES)
            .map(|probe| probe.large.scan());
        let result = self.searcher.search_reader(
            self.query.matcher,
            Reader {
                file,
                remaining: metadata.len(),
                budget: &mut budget,
            },
            &mut sink,
        );
        if metadata.len() > REUSABLE_FILE_BYTES {
            self.searcher = self.builder.build();
        }
        file_io::cancelled(self.cancel)?;
        if sink.timed_out {
            budget.reason = Some(SearchStopReason::Time);
        }
        if budget.reason.is_none() {
            result.map_err(ToolError::from)?;
        }
        Ok(Scanned {
            sink,
            bytes: budget.bytes - spent,
            reason: budget.reason,
        })
    }
}
