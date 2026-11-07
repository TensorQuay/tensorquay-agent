//! TOOL-U-005: large streaming buffers are serialized and permit waits are cancellable.
use super::*;

pub(in crate::builtin::grep) struct LargeProbe {
    events: Sender<Event>,
    gate: (Mutex<bool>, Condvar),
    active: AtomicUsize,
    maximum: AtomicUsize,
}
impl LargeProbe {
    pub(super) fn new(events: Sender<Event>) -> Self {
        Self {
            events,
            gate: (Mutex::new(true), Condvar::new()),
            active: AtomicUsize::new(0),
            maximum: AtomicUsize::new(0),
        }
    }
    pub(in crate::builtin::grep) fn scan(&self) -> Scan<'_> {
        let active = self.active.fetch_add(1, Ordering::SeqCst) + 1;
        self.maximum.fetch_max(active, Ordering::SeqCst);
        let scan = Scan(self);
        let _ = self.events.send(Event::LargeScanning);
        let guard = self
            .gate
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        // Real worker coordination is bounded so a regression cannot strand the test.
        let (guard, timeout) = self
            .gate
            .1
            .wait_timeout_while(guard, Duration::from_secs(10), |released| !*released)
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        assert!(!timeout.timed_out() || *guard, "large-file gate timed out");
        scan
    }
    pub(in crate::builtin::grep) fn waiting(&self) {
        let _ = self.events.send(Event::LargeWaiting);
    }
    fn hold(&self) {
        *self
            .gate
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = false;
    }
    fn release(&self) {
        *self
            .gate
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = true;
        self.gate.1.notify_all();
    }
}
pub(in crate::builtin::grep) struct Scan<'a>(&'a LargeProbe);
impl Drop for Scan<'_> {
    fn drop(&mut self) {
        self.0.active.fetch_sub(1, Ordering::SeqCst);
    }
}

fn large_context(directory: &tempfile::TempDir) -> Result<ToolContext, ToolError> {
    for index in 0..8 {
        // Sparse binary files exercise metadata classification without allocating fixture buffers.
        let file = fs::File::create(directory.path().join(format!("{index:04}")))?;
        file.set_len(super::super::super::engine::LARGE_FILE_BYTES + 1)?;
    }
    let mut cx = context(directory)?;
    cx.limits.search_threads = 8;
    Ok(cx)
}

fn observed_workers(events: &Receiver<Event>) -> Result<(usize, usize), Box<dyn Error>> {
    let mut scanning = 0;
    let mut waiting = 0;
    while scanning + waiting < 8 {
        match events.recv_timeout(Duration::from_secs(5))? {
            Event::LargeScanning => scanning += 1,
            Event::LargeWaiting => waiting += 1,
            _ => {}
        }
    }
    Ok((scanning, waiting))
}

#[test]
fn tool_u_005_parallel_large_file_scans_share_one_permit() -> TestResult {
    let directory = tempfile::tempdir()?;
    let cx = large_context(&directory)?;
    let (probe, events) = Probe::new(false);
    probe.large.hold();
    let (observed, output) = thread::scope(|scope| {
        let worker = scope.spawn(|| search(&cx, 8, &probe));
        let observed = observed_workers(&events);
        probe.large.release();
        (observed, worker.join())
    });
    assert_eq!(
        probe.large.maximum.load(Ordering::SeqCst),
        1,
        "large-file scans overlapped"
    );
    assert_eq!(observed?, (1, 7));
    assert!(
        output
            .map_err(|_| "search panicked")??
            .search
            .ok_or("status")?
            .complete
    );
    assert_eq!(probe.large.active.load(Ordering::SeqCst), 0);
    assert_eq!(probe.started.load(Ordering::SeqCst), 8);
    assert_eq!(probe.finished.load(Ordering::SeqCst), 8);
    Ok(())
}

#[test]
fn tool_u_005_parallel_cancel_while_waiting_for_large_file_joins_workers() -> TestResult {
    let directory = tempfile::tempdir()?;
    let cx = large_context(&directory)?;
    let (probe, events) = Probe::new(false);
    probe.large.hold();
    thread::scope(|scope| -> TestResult {
        let worker = scope.spawn(|| search(&cx, 8, &probe));
        let observed = observed_workers(&events);
        cx.cancel.cancel();
        // Waiters must leave without waiting for the held scanner to release its permit.
        let finished = finished_waiters(&events);
        probe.large.release();
        let result = worker.join().map_err(|_| "search panicked")?;
        assert_eq!(observed?, (1, 7));
        assert_eq!(finished, 7, "permit wait did not observe cancellation");
        assert_eq!(result, Err(ToolError::Cancelled));
        assert_eq!(probe.started.load(Ordering::SeqCst), 8);
        assert_eq!(probe.finished.load(Ordering::SeqCst), 8);
        assert_eq!(probe.large.active.load(Ordering::SeqCst), 0);
        Ok(())
    })
}

fn finished_waiters(events: &Receiver<Event>) -> usize {
    let mut finished = 0;
    // Observe real worker exits with a bound; the held scanner cannot release the permit yet.
    let deadline = Instant::now() + Duration::from_secs(2);
    while finished < 7 {
        match events.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
            Ok(Event::WorkerFinished) => finished += 1,
            Ok(_) => {}
            Err(_) => break,
        }
    }
    finished
}

#[test]
fn tool_u_005_parallel_elapsed_limit_while_waiting_for_large_file_joins_workers() -> TestResult {
    let directory = tempfile::tempdir()?;
    let mut cx = large_context(&directory)?;
    let clock = FakeClock::new();
    cx.clock = Arc::new(clock.clone());
    let (probe, events) = Probe::new(false);
    probe.large.hold();
    thread::scope(|scope| -> TestResult {
        let worker = scope.spawn(|| search(&cx, 8, &probe));
        let observed = observed_workers(&events);
        let advanced = clock.advance(cx.limits.search_max_elapsed);
        let finished = finished_waiters(&events);
        probe.large.release();
        let output = worker.join().map_err(|_| "search panicked")??;
        advanced?;
        assert_eq!(observed?, (1, 7));
        assert_eq!(finished, 7, "permit wait did not observe elapsed limit");
        assert_eq!(
            output.search.ok_or("status")?.reason,
            Some(SearchStopReason::Time)
        );
        assert_eq!(probe.started.load(Ordering::SeqCst), 8);
        assert_eq!(probe.finished.load(Ordering::SeqCst), 8);
        assert_eq!(probe.large.active.load(Ordering::SeqCst), 0);
        Ok(())
    })
}
