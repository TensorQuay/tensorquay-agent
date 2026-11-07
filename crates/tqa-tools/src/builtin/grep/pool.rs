//! A bounded look-ahead window permits parallel I/O while the caller merges by path.
use super::{
    Args,
    engine::{Engine, Scanned},
    large_files::LargeFiles,
    sink::Capture,
};
use crate::{
    ToolContext, ToolError,
    builtin::{
        file_io,
        search::{budget::Budget, page::Page},
    },
};
use grep_regex::RegexMatcher;
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{Condvar, Mutex, MutexGuard},
    thread,
    time::Instant,
};
use tokio_util::sync::CancellationToken;

// Bound completed and in-flight results together, even when the first file is slow.
const MAX_PENDING_FILES: usize = 256;
// Automatic concurrency is conservative; explicitly configured pools may be larger.
const AUTO_THREADS: usize = 8;

pub(super) struct Query<'a> {
    pub(super) args: &'a Args,
    pub(super) matcher: &'a RegexMatcher,
    pub(super) cx: &'a ToolContext,
    pub(super) cancel: &'a CancellationToken,
    pub(super) started: Instant,
    #[cfg(test)]
    pub(super) probe: Option<&'a tests::Probe>,
}
struct State {
    next: usize,
    merged: usize,
    allowance: u64,
    running: usize,
    failed: bool,
    results: BTreeMap<usize, Result<Scanned, ToolError>>,
}
struct Pool<'a> {
    query: &'a Query<'a>,
    paths: &'a [PathBuf],
    capture: Capture,
    cancel: CancellationToken,
    state: Mutex<State>,
    changed: Condvar,
    large_files: LargeFiles,
}
fn worker_error() -> ToolError {
    ToolError::Io("search worker failed".into())
}
fn threads(configured: u16) -> usize {
    if configured == 0 {
        thread::available_parallelism()
            .map_or(1, std::num::NonZero::get)
            .min(AUTO_THREADS)
    } else {
        usize::from(configured)
    }
}

pub(super) fn search(
    query: &Query<'_>,
    paths: &[PathBuf],
    page: &mut Page,
    budget: &mut Budget<'_>,
) -> Result<u64, ToolError> {
    let pool = Pool {
        query,
        paths,
        capture: Capture::new(page),
        cancel: query.cancel.child_token(),
        state: Mutex::new(State {
            next: 0,
            merged: 0,
            allowance: query.cx.limits.search_max_bytes,
            running: 0,
            failed: false,
            results: BTreeMap::new(),
        }),
        changed: Condvar::new(),
        large_files: LargeFiles::default(),
    };
    thread::scope(|scope| {
        let stop = Stop(&pool);
        let mut handles = vec![];
        let mut spawn_error = None;
        for _ in 0..threads(query.cx.limits.search_threads).min(paths.len()) {
            pool.lock()?.running += 1;
            let pool = &pool;
            match thread::Builder::new().spawn_scoped(scope, move || pool.work()) {
                Ok(handle) => handles.push(handle),
                Err(error) => {
                    pool.lock()?.running -= 1;
                    spawn_error = Some(ToolError::from(error));
                    break;
                }
            }
        }
        let result = if let Some(error) = spawn_error {
            Err(error)
        } else {
            pool.merge(page, budget)
        };
        drop(stop);
        let mut joined = Ok(());
        for handle in handles {
            if !matches!(handle.join(), Ok(Ok(()))) {
                joined = Err(worker_error());
            }
        }
        file_io::cancelled(query.cancel)?;
        joined?;
        result
    })
}
struct Stop<'a>(&'a Pool<'a>);
impl Drop for Stop<'_> {
    fn drop(&mut self) {
        self.0.cancel.cancel();
        // Serialize the wake-up with a worker's predicate check and entry into the wait.
        let _state = self
            .0
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        self.0.changed.notify_all();
    }
}
impl Pool<'_> {
    fn lock(&self) -> Result<MutexGuard<'_, State>, ToolError> {
        self.state.lock().map_err(|_| worker_error())
    }
    fn claim(&self) -> Result<Option<(usize, u64)>, ToolError> {
        let mut state = self.lock()?;
        loop {
            if self.cancel.is_cancelled()
                || state.next == self.paths.len()
                || state.next as u64 == self.query.cx.limits.search_max_files
            {
                return Ok(None);
            }
            if state.next - state.merged < MAX_PENDING_FILES {
                let index = state.next;
                state.next += 1;
                return Ok(Some((index, state.allowance)));
            }
            #[cfg(test)]
            if let Some(probe) = self.query.probe {
                probe.window_full();
            }
            state = self.changed.wait(state).map_err(|_| worker_error())?;
        }
    }
    fn work(&self) -> Result<(), ToolError> {
        let mut owner = Worker {
            pool: self,
            failed: true,
        };
        #[cfg(test)]
        if let Some(probe) = self.query.probe {
            probe
                .started
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }
        let mut engine = Engine::new(self.query, self.capture, &self.cancel, &self.large_files);
        while let Some((index, allowance)) = self.claim()? {
            #[cfg(test)]
            if let Some(probe) = self.query.probe {
                probe.before_file(index);
            }
            let result = engine.file(&self.paths[index], allowance);
            let mut state = self.lock()?;
            state.results.insert(index, result);
            #[cfg(test)]
            let count = state.results.len();
            drop(state);
            #[cfg(test)]
            if let Some(probe) = self.query.probe {
                probe.buffered(count);
            }
            self.changed.notify_all();
        }
        owner.failed = false;
        Ok(())
    }
    fn take(&self, index: usize) -> Result<Scanned, ToolError> {
        let mut state = self.lock()?;
        loop {
            file_io::cancelled(self.query.cancel)?;
            if let Some(result) = state.results.remove(&index) {
                return result;
            }
            if state.failed || state.running == 0 {
                return Err(worker_error());
            }
            #[cfg(test)]
            if let Some(probe) = self.query.probe {
                probe.waiting();
            }
            state = self.changed.wait(state).map_err(|_| worker_error())?;
        }
    }
    fn merge(&self, page: &mut Page, budget: &mut Budget<'_>) -> Result<u64, ToolError> {
        let mut binaries = 0;
        for index in 0..self.paths.len() {
            if !budget.file()? {
                break;
            }
            let scanned = self.take(index)?;
            if !budget.account(scanned.bytes) {
                break;
            }
            if let Some(reason) = scanned.reason {
                budget.reason = Some(reason);
                break;
            }
            if scanned.sink.binary {
                binaries += 1;
            } else if !scanned.sink.append(page)? {
                break;
            }
            let mut state = self.lock()?;
            state.merged = index + 1;
            state.allowance = self.query.cx.limits.search_max_bytes - budget.bytes;
            drop(state);
            self.changed.notify_all();
        }
        Ok(binaries)
    }
}
struct Worker<'a> {
    pool: &'a Pool<'a>,
    failed: bool,
}
impl Drop for Worker<'_> {
    fn drop(&mut self) {
        // Recover the lock only to wake the owner on failure; it still returns an error.
        let mut state = self
            .pool
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.running -= 1;
        state.failed |= self.failed;
        drop(state);
        #[cfg(test)]
        if let Some(probe) = self.pool.query.probe {
            probe.worker_finished();
        }
        self.pool.changed.notify_all();
    }
}

#[cfg(test)]
pub(super) mod tests;
