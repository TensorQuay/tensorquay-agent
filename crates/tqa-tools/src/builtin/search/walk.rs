//! Parallel ignore-library traversal with explicit workspace filters and bounds.
mod filters;
mod ignores;
use crate::{SearchStopReason, ToolContext, ToolError};
use filters::Filters;
use ignore::{WalkBuilder, WalkState, types::TypesBuilder};
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Instant,
};
use tokio_util::sync::CancellationToken;

// Explicit worker bound avoids host-dependent implicit concurrency.
const WALK_THREADS: usize = 4;
pub(crate) struct Options<'a> {
    pub(crate) path: &'a str,
    pub(crate) glob: Option<&'a str>,
    pub(crate) file_type: Option<&'a str>,
}
pub(crate) struct Walked {
    pub(crate) paths: Vec<PathBuf>,
    pub(crate) reason: Option<SearchStopReason>,
}
#[derive(Default)]
struct State {
    paths: Vec<PathBuf>,
    entries: usize,
    reason: Option<SearchStopReason>,
    error: Option<ToolError>,
}
struct Guard {
    cx: ToolContext,
    cancel: CancellationToken,
    started: Instant,
    filters: Filters,
    state: Mutex<State>,
}

pub(crate) fn collect(
    cx: &ToolContext,
    options: &Options<'_>,
    cancel: &CancellationToken,
    started: Instant,
) -> Result<Walked, ToolError> {
    let filters = Filters::new(&cx.workspace, options)?;
    ignores::validate(cx.workspace.path(), cx.limits.ignore_file_bytes, cancel)?;
    let guard = Arc::new(Guard {
        cx: cx.clone(),
        cancel: cancel.clone(),
        started,
        filters,
        state: Mutex::new(State::default()),
    });
    let mut builder = WalkBuilder::new(cx.workspace.path());
    builder
        .current_dir(cx.workspace.path())
        .parents(false)
        .git_global(false)
        .require_git(false)
        .git_ignore(true)
        .git_exclude(true)
        .ignore(true)
        .hidden(false)
        .follow_links(false)
        .threads(WALK_THREADS)
        .add_custom_ignore_filename(".tqaignore");
    if let Some(file_type) = options.file_type {
        let mut types = TypesBuilder::new();
        types.add_defaults().select(file_type);
        builder.types(types.build().map_err(|_| {
            ToolError::invalid("/file_type", "unknown file type; use a ripgrep type name")
        })?);
    }
    let filter_guard = Arc::clone(&guard);
    builder.filter_entry(move |entry| match filter_guard.accept(entry) {
        Ok(accepted) => accepted,
        Err(error) => {
            filter_guard.fail(error);
            false
        }
    });
    builder.build_parallel().run(|| {
        let guard = Arc::clone(&guard);
        Box::new(move |entry| guard.visit(entry))
    });
    let mut state = guard
        .state
        .lock()
        .map_err(|_| ToolError::Io("workspace traversal worker failed".into()))?;
    if let Some(error) = state.error.take() {
        return Err(error);
    }
    state
        .paths
        .sort_unstable_by(|a, b| a.as_os_str().cmp(b.as_os_str()));
    Ok(Walked {
        paths: std::mem::take(&mut state.paths),
        reason: state.reason,
    })
}
impl Guard {
    fn accept(&self, entry: &ignore::DirEntry) -> Result<bool, ToolError> {
        super::super::file_io::cancelled(&self.cancel)?;
        {
            let mut state = self
                .state
                .lock()
                .map_err(|_| ToolError::Io("workspace traversal worker failed".into()))?;
            if state.error.is_some() || state.reason.is_some() {
                return Ok(false);
            }
            if self.cx.clock.now().saturating_duration_since(self.started)
                >= self.cx.limits.search_max_elapsed
            {
                state.reason = Some(SearchStopReason::Time);
                return Ok(false);
            }
            if state.entries == self.cx.limits.glob_entries {
                state.reason = Some(SearchStopReason::Files);
                return Ok(false);
            }
            state.entries += 1;
        }
        if !self.filters.matches(entry)? {
            return Ok(false);
        }
        if entry.file_type().is_some_and(|kind| kind.is_dir()) {
            ignores::validate(entry.path(), self.cx.limits.ignore_file_bytes, &self.cancel)?;
        }
        Ok(true)
    }
    fn fail(&self, error: ToolError) {
        if let Ok(mut state) = self.state.lock() {
            state.error.get_or_insert(error);
        }
    }
    fn visit(&self, entry: Result<ignore::DirEntry, ignore::Error>) -> WalkState {
        let Ok(entry) = entry else {
            self.fail(ToolError::Io("workspace traversal failed".into()));
            return WalkState::Quit;
        };
        if entry.error().is_some() {
            self.fail(ToolError::invalid("/ignore", "invalid ignore file"));
            return WalkState::Quit;
        }
        let Ok(mut state) = self.state.lock() else {
            return WalkState::Quit;
        };
        if state.reason.is_some() || state.error.is_some() {
            return WalkState::Quit;
        }
        if entry.file_type().is_some_and(|kind| kind.is_file()) {
            state.paths.push(entry.into_path());
        }
        WalkState::Continue
    }
}
