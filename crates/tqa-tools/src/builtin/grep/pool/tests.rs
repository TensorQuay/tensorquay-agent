//! TOOL-U-005: observe ordering, back-pressure and joined cancellation without sleeps.
mod large_files;
mod shutdown;

use super::*;
use crate::{SearchStopReason, ToolLimits, ToolOutput, WorkspaceRoot};
use serde_json::json;
use std::{
    error::Error,
    fs,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc::{self, Receiver, Sender},
    },
    time::{Duration, Instant},
};
use tqa_testkit::{FakeClock, FakeSleeper};
use tqa_types::Clock;

type TestResult = Result<(), Box<dyn Error>>;

pub(super) enum Event {
    Buffered(usize),
    WindowFull,
    Waiting,
    LargeScanning,
    LargeWaiting,
    WorkerFinished,
}

pub(in crate::builtin::grep) struct Probe {
    events: Sender<Event>,
    gate: (Mutex<bool>, Condvar),
    pub(super) started: AtomicUsize,
    pub(super) finished: AtomicUsize,
    pub(super) maximum: AtomicUsize,
    armed: Arc<AtomicBool>,
    hold_window: bool,
    pub(in crate::builtin::grep) large: large_files::LargeProbe,
}
impl Probe {
    fn new(hold_first: bool) -> (Self, Receiver<Event>) {
        let (events, receiver) = mpsc::channel();
        (
            Self {
                large: large_files::LargeProbe::new(events.clone()),
                events,
                gate: (Mutex::new(!hold_first), Condvar::new()),
                started: AtomicUsize::new(0),
                finished: AtomicUsize::new(0),
                maximum: AtomicUsize::new(0),
                armed: Arc::new(AtomicBool::new(false)),
                hold_window: false,
            },
            receiver,
        )
    }
    pub(super) fn before_file(&self, index: usize) {
        if index == 0 {
            let guard = self
                .gate
                .0
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let (guard, timeout) = self
                .gate
                .1
                .wait_timeout_while(guard, Duration::from_secs(10), |released| !*released)
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            assert!(
                !timeout.timed_out() || *guard,
                "first-file test gate timed out"
            );
        }
        self.armed.store(true, Ordering::SeqCst);
    }
    fn release(&self) {
        *self
            .gate
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = true;
        self.gate.1.notify_all();
    }
    pub(super) fn buffered(&self, count: usize) {
        self.maximum.fetch_max(count, Ordering::SeqCst);
        let _ = self.events.send(Event::Buffered(count));
    }
    pub(super) fn window_full(&self) {
        let _ = self.events.send(Event::WindowFull);
        if self.hold_window {
            self.before_file(0);
        }
    }
    pub(super) fn waiting(&self) {
        let _ = self.events.send(Event::Waiting);
    }
    pub(super) fn worker_finished(&self) {
        self.finished.fetch_add(1, Ordering::SeqCst);
        let _ = self.events.send(Event::WorkerFinished);
    }
}

fn context(directory: &tempfile::TempDir) -> Result<ToolContext, ToolError> {
    let clock = FakeClock::new();
    Ok(ToolContext {
        workspace: WorkspaceRoot::open(directory.path())?,
        cancel: CancellationToken::new(),
        clock: Arc::new(clock.clone()),
        sleeper: Arc::new(FakeSleeper::new(clock)),
        env_allow: vec![],
        limits: ToolLimits::RECOMMENDED,
    })
}

fn search(cx: &ToolContext, count: usize, probe: &Probe) -> Result<ToolOutput, ToolError> {
    let args = crate::builtin::arguments(json!({"pattern":"hit"}))?;
    let matcher = super::super::matcher(&args)?;
    let mut page = super::super::page(&args, cx)?;
    let mut budget = Budget::new(cx, &cx.cancel, cx.clock.now());
    let query = Query {
        args: &args,
        matcher: &matcher,
        cx,
        cancel: &cx.cancel,
        started: budget.started,
        probe: Some(probe),
    };
    let paths = (0..count)
        .map(|index| cx.workspace.path().join(format!("{index:04}")))
        .collect::<Vec<_>>();
    let binaries = super::search(&query, &paths, &mut page, &mut budget)?;
    page.finish(&budget, None, binaries)
}

fn run_held(
    cx: &ToolContext,
    count: usize,
    window: bool,
) -> Result<(ToolOutput, usize), Box<dyn Error>> {
    let (probe, events) = Probe::new(true);
    std::thread::scope(|scope| {
        let work = scope.spawn(|| search(cx, count, &probe));
        let observed = loop {
            match events.recv_timeout(Duration::from_secs(5)) {
                Ok(Event::WindowFull) if window => break Ok(()),
                Ok(Event::Buffered(size)) if !window || size > MAX_PENDING_FILES => break Ok(()),
                Ok(_) => {}
                Err(error) => break Err(error),
            }
        };
        probe.release();
        let result = work.join().map_err(|_| "search panicked")?;
        observed?;
        let output = result?;
        assert_eq!(
            probe.started.load(Ordering::SeqCst),
            probe.finished.load(Ordering::SeqCst)
        );
        assert!(probe.started.load(Ordering::SeqCst) > 1);
        Ok((output, probe.maximum.load(Ordering::SeqCst)))
    })
}

#[test]
fn tool_u_005_parallel_completion_order_never_changes_path_order() -> TestResult {
    let directory = tempfile::tempdir()?;
    for index in 0..20 {
        fs::write(directory.path().join(format!("{index:04}")), "hit\n")?;
    }
    let mut cx = context(&directory)?;
    cx.limits.search_threads = 1;
    let (probe, _) = Probe::new(false);
    let baseline = search(&cx, 20, &probe)?;
    for threads in [2, 8] {
        cx.limits.search_threads = threads;
        let (parallel, _) = run_held(&cx, 20, false)?;
        assert_eq!(parallel, baseline);
    }
    Ok(())
}

#[test]
fn tool_u_005_parallel_reorder_buffer_never_exceeds_lookahead() -> TestResult {
    let directory = tempfile::tempdir()?;
    let count = MAX_PENDING_FILES * 2;
    for index in 0..count {
        fs::write(directory.path().join(format!("{index:04}")), "hit\n")?;
    }
    let mut cx = context(&directory)?;
    cx.limits.search_threads = 8;
    let (_, maximum) = run_held(&cx, count, true)?;
    assert!(
        maximum <= MAX_PENDING_FILES,
        "buffered {maximum} file results"
    );
    assert!(maximum > 1, "the test must exercise reordering");
    Ok(())
}

struct InterruptClock {
    start: Instant,
    armed: Arc<AtomicBool>,
    checks: AtomicUsize,
    cancel: Option<CancellationToken>,
}
impl Clock for InterruptClock {
    fn now(&self) -> Instant {
        if self.armed.load(Ordering::SeqCst) {
            let checks = self.checks.fetch_add(1, Ordering::SeqCst);
            if let Some(cancel) = &self.cancel {
                cancel.cancel();
            } else if checks > 0 {
                return self.start + Duration::from_secs(6);
            }
        }
        self.start
    }
}

#[test]
fn tool_u_005_parallel_cancellation_inside_a_read_joins_every_worker() -> TestResult {
    let directory = tempfile::tempdir()?;
    fs::write(directory.path().join("0000"), "x".repeat(8 * 1024 * 1024))?;
    let mut cx = context(&directory)?;
    let (probe, events) = Probe::new(true);
    let clock = Arc::new(InterruptClock {
        start: Instant::now(),
        armed: Arc::clone(&probe.armed),
        checks: AtomicUsize::new(0),
        cancel: Some(cx.cancel.clone()),
    });
    cx.clock = clock.clone();
    let result = std::thread::scope(|scope| -> Result<_, Box<dyn Error>> {
        let work = scope.spawn(|| search(&cx, 1, &probe));
        let waiting = events.recv_timeout(Duration::from_secs(5));
        probe.release();
        let result = work.join().map_err(|_| "search panicked")?;
        assert!(matches!(waiting?, Event::Waiting));
        Ok(result)
    })?;
    assert_eq!(result, Err(ToolError::Cancelled));
    assert_eq!(
        probe.started.load(Ordering::SeqCst),
        probe.finished.load(Ordering::SeqCst)
    );
    assert!(probe.started.load(Ordering::SeqCst) > 0);
    assert!(
        clock.checks.load(Ordering::SeqCst) <= 2,
        "worker kept reading after cancellation"
    );
    Ok(())
}

#[test]
fn tool_u_005_parallel_time_cut_omits_the_unfinished_file() -> TestResult {
    let directory = tempfile::tempdir()?;
    fs::write(directory.path().join("0000"), "hit\n".repeat(30_000))?;
    for threads in [1, 2, 8] {
        let mut cx = context(&directory)?;
        cx.limits.search_threads = threads;
        let (probe, _) = Probe::new(false);
        cx.clock = Arc::new(InterruptClock {
            start: Instant::now(),
            armed: Arc::clone(&probe.armed),
            checks: AtomicUsize::new(0),
            cancel: None,
        });
        let output = search(&cx, 1, &probe)?;
        assert!(output.text.starts_with("[incomplete:"));
        assert_eq!(
            output.search.ok_or("status")?.reason,
            Some(SearchStopReason::Time)
        );
        assert_eq!(
            probe.started.load(Ordering::SeqCst),
            probe.finished.load(Ordering::SeqCst)
        );
    }
    Ok(())
}
